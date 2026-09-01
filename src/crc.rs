//! CRC-16/USB — the checksum ("token") used inside the OBSBOT V3 vendor frame.
//!
//! Parameters: poly 0xA001 (reflected 0x8005), init 0xFFFF, refin/refout true,
//! xorout 0xFFFF. Verified byte-for-byte against OBSBOT Center-generated frames
//! captured by Tiny4Linux, and live against a real Tiny 3 Lite.

/// Compute CRC-16/USB over `data`.
pub fn crc16_usb(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= b as u16;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xA001;
            } else {
                crc >>= 1;
            }
        }
    }
    crc ^ 0xFFFF
}

#[cfg(test)]
mod tests {
    use super::*;

    // The header token of the known-good Tiny4Linux "wake" frame:
    //   aa 25 a5 00 0c 00 [5f ef] 0a 02 c2 a0 ...
    // token is CRC over bytes[0:6] + 00 00 + bytes[8:12].
    #[test]
    fn wake_header_token() {
        let cov = [
            0xAA, 0x25, 0xA5, 0x00, 0x0C, 0x00, 0x00, 0x00, 0x0A, 0x02, 0xC2, 0xA0,
        ];
        assert_eq!(crc16_usb(&cov), 0xEF5F);
    }

    // Nested payload token of the same frame: over [len2 lo, len2 hi] + 00 00 + payload.
    #[test]
    fn wake_payload_token() {
        // len2 = 0x0004, payload = 00 00 00 00 (wake=0)
        let cov = [0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(crc16_usb(&cov), 0x07BE);
    }

    #[test]
    fn sleep_payload_token() {
        // sleep frame nested: len2=4, payload = 01 00 00 00
        let cov = [0x04, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00];
        assert_eq!(crc16_usb(&cov), 0xFBBF);
    }
}
