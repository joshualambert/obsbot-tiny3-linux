//! The OBSBOT "V3" vendor frame carried on UVC extension-unit selector 0x02.
//!
//! Layout (60 bytes, zero-padded, little-endian):
//! ```text
//!  0      magic   0xAA
//!  1      flags   0x25 = SET (with nested payload) · 0x01 = header-only GET
//!  2..4   seq     u16   reply echoes it — match on this
//!  4..6   len     u16 = 0x000C  (header bytes 0..12 are covered by the header token)
//!  6..8   token   u16   CRC-16/USB over bytes[0:6] + 00 00 + bytes[8:12]
//!  8      sender  0x0A  (host)
//!  9      receiver      subsystem id (0x02 camera, 0x03 gimbal, 0x04 AI, 0x0D upgrade)
//! 10..12  cmd     u16   wire command id
//! -- nested payload segment, present only when there is a payload --
//! 12..14  len2    u16   payload length
//! 14..16  token2  u16   CRC-16/USB over bytes[12:14] + 00 00 + payload
//! 16..    payload
//! ```
//!
//! Confirmed live on a Tiny 3 Lite: a GET framed with flags 0x01 returns a reply
//! (flags 0x29, sender/receiver swapped); SET commands are fire-and-forget and
//! produce no mailbox reply.

use crate::crc::crc16_usb;

pub const MAGIC: u8 = 0xAA;
pub const FLAG_SET: u8 = 0x25;
pub const FLAG_GET: u8 = 0x01;
pub const SENDER_HOST: u8 = 0x0A;
pub const FRAME_LEN: usize = 60;
/// Max payload that fits after the 16-byte header+nested-segment prefix.
pub const MAX_PAYLOAD: usize = FRAME_LEN - 16; // 44
const HEADER_COVERED: u16 = 0x000C;

/// Build a 60-byte V3 frame ready to write to XU selector 0x02.
///
/// Panics if `payload` exceeds [`MAX_PAYLOAD`] (44) — a programmer error, since
/// payloads are fixed protocol constants, never user input. Every command in
/// this crate is well under the limit.
pub fn build(flags: u8, seq: u16, receiver: u8, cmd: u16, payload: &[u8]) -> [u8; FRAME_LEN] {
    assert!(
        payload.len() <= MAX_PAYLOAD,
        "V3 payload is {} bytes, max {MAX_PAYLOAD}",
        payload.len()
    );
    let mut f = [0u8; FRAME_LEN];
    f[0] = MAGIC;
    f[1] = flags;
    f[2..4].copy_from_slice(&seq.to_le_bytes());
    f[4..6].copy_from_slice(&HEADER_COVERED.to_le_bytes());
    f[8] = SENDER_HOST;
    f[9] = receiver;
    f[10..12].copy_from_slice(&cmd.to_le_bytes());

    // Header token: CRC over [0:6] + 00 00 (token field zeroed) + [8:12].
    let mut hcov = [0u8; 12];
    hcov[0..6].copy_from_slice(&f[0..6]);
    hcov[8..12].copy_from_slice(&f[8..12]);
    let token = crc16_usb(&hcov);
    f[6..8].copy_from_slice(&token.to_le_bytes());

    if !payload.is_empty() {
        let len2 = payload.len() as u16;
        f[12..14].copy_from_slice(&len2.to_le_bytes());
        // Nested token: CRC over [len2 bytes] + 00 00 (token2 zeroed) + payload.
        let mut ncov = Vec::with_capacity(4 + payload.len());
        ncov.extend_from_slice(&len2.to_le_bytes());
        ncov.extend_from_slice(&[0, 0]);
        ncov.extend_from_slice(payload);
        let token2 = crc16_usb(&ncov);
        f[14..16].copy_from_slice(&token2.to_le_bytes());
        f[16..16 + payload.len()].copy_from_slice(payload);
    }
    f
}

/// A parsed V3 reply.
#[derive(Debug, Clone)]
pub struct Reply {
    pub flags: u8,
    pub seq: u16,
    pub sender: u8,
    pub receiver: u8,
    pub cmd: u16,
    pub payload: Vec<u8>,
}

/// Parse a 60-byte buffer as a V3 frame, validating magic and both CRCs.
/// Returns None if it is not a well-formed frame (e.g. a zeroed no-reply).
pub fn parse(raw: &[u8]) -> Option<Reply> {
    if raw.len() < 12 || raw[0] != MAGIC {
        return None;
    }
    let len = u16::from_le_bytes([raw[4], raw[5]]);
    if len != HEADER_COVERED {
        return None;
    }
    let token = u16::from_le_bytes([raw[6], raw[7]]);
    let mut hcov = [0u8; 12];
    hcov[0..6].copy_from_slice(&raw[0..6]);
    hcov[8..12].copy_from_slice(&raw[8..12]);
    if crc16_usb(&hcov) != token {
        return None;
    }
    let mut reply = Reply {
        flags: raw[1],
        seq: u16::from_le_bytes([raw[2], raw[3]]),
        sender: raw[8],
        receiver: raw[9],
        cmd: u16::from_le_bytes([raw[10], raw[11]]),
        payload: Vec::new(),
    };
    if raw.len() >= 16 {
        let len2 = u16::from_le_bytes([raw[12], raw[13]]) as usize;
        if len2 > 0 {
            // A payload is claimed: it must fit (<=44, offset 16) and its CRC
            // must check out. If not, reject the WHOLE frame (return None)
            // rather than quietly yielding an empty payload — a caller polling
            // the mailbox should keep waiting for a valid reply, not accept a
            // corrupt one as if it had no data.
            if 16 + len2 > raw.len() || len2 > MAX_PAYLOAD {
                return None;
            }
            let token2 = u16::from_le_bytes([raw[14], raw[15]]);
            let mut ncov = Vec::with_capacity(4 + len2);
            ncov.extend_from_slice(&raw[12..14]);
            ncov.extend_from_slice(&[0, 0]);
            ncov.extend_from_slice(&raw[16..16 + len2]);
            if crc16_usb(&ncov) != token2 {
                return None;
            }
            reply.payload = raw[16..16 + len2].to_vec();
        }
    }
    Some(reply)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_wake_matches_capture() {
        // Tiny4Linux wake frame, seq 0x00A5. Compare the 20 meaningful bytes
        // (header + nested segment); the rest is zero padding.
        let f = build(FLAG_SET, 0x00A5, 0x02, 0xA0C2, &[0, 0, 0, 0]);
        let expect = hex("aa25a5000c005fef0a02c2a00400be0700000000");
        assert_eq!(&f[..20], &expect[..]);
        assert!(f[20..].iter().all(|&b| b == 0), "tail must be zero-padded");
    }

    #[test]
    fn build_sleep_matches_capture() {
        let f = build(FLAG_SET, 0x0042, 0x02, 0xA0C2, &[1, 0, 0, 0]);
        // header + nested, first 20 bytes are the interesting part.
        let expect = hex("aa2542000c00ea630a02c2a00400bffb01000000");
        assert_eq!(&f[..20], &expect[..]);
    }

    #[test]
    fn roundtrip_parse() {
        let f = build(FLAG_SET, 0x1234, 0x04, 0x6484, &[1, 2, 3, 4, 5, 6]);
        let r = parse(&f).expect("parse");
        assert_eq!(r.seq, 0x1234);
        assert_eq!(r.receiver, 0x04);
        assert_eq!(r.cmd, 0x6484);
        assert_eq!(r.payload, vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn rejects_zeroed() {
        assert!(parse(&[0u8; 60]).is_none());
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }
}
