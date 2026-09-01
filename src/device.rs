//! High-level device API: vendor commands (sleep/wake/tracking/gimbal) over the
//! framed V3 protocol and raw TLVs, plus the standard-UVC controls.
//!
//! Every method here that talks to the camera requires an open [`VideoFd`], and
//! **an open fd blocks the camera from sleeping** (it holds USB autosuspend
//! off). Hold a [`Device`] only while you intend the camera in use.

use crate::controls;
use crate::error::{Error, Result};
use crate::frame::{self, FLAG_GET, FLAG_SET};
use crate::ioctl::VideoFd;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;

// UVC extension unit (same as Tiny 2: bUnitID 2, GUID 9A1E7291-...).
const XU_UNIT: u8 = 0x02;
const SEL_CMD: u8 = 0x02; // framed V3 command channel + reply mailbox
const SEL_STATUS: u8 = 0x06; // 60-byte status block + raw-TLV write target

// Subsystem receiver ids.
const RCV_CAMERA: u8 = 0x02;
const RCV_GIMBAL: u8 = 0x03;
const RCV_AI: u8 = 0x04;
const RCV_UPGRADE: u8 = 0x0D;

// Wire command ids (little-endian on the wire).
const CMD_DEV_STATUS: u16 = 0xA0C2; // sleep/wake
const CMD_RECENTER: u16 = 0x00C3;
const CMD_GIMBAL_MOVE: u16 = 0x6444; // move-to-angle, 3x f32 motor degrees
const CMD_TRACK_SPEED: u16 = 0x0CC4; // payload [0]=standard [2]=sport
const CMD_GET_SN: u16 = 0x18C8;
const CMD_GET_UUID: u16 = 0x1808;

// Raw-TLV tags on selector 6.
const TLV_HDR: u8 = 0x01;
const TLV_FACE_AE: u8 = 0x03;
const TLV_FOV: u8 = 0x04;
const TLV_AI_TRACK: u8 = 0x16;

// Status-block byte offsets (decoded from Tiny4Linux, confirmed reacting live).
const ST_SLEEP: usize = 0x02;
const ST_HDR: usize = 0x06;
const ST_AI_CATEGORY: usize = 0x18;
const ST_AI_SUBMODE: usize = 0x1c;
const ST_TRACK_SPEED: usize = 0x21;

/// AI tracking mode. The wire encoding is `[0x16, 0x02, category, submode]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackMode {
    Off,
    /// Human tracking with a framing submode.
    Normal,
    UpperBody,
    CloseUp,
    Headless,
    LowerBody,
    Group,
    Hand,
    Whiteboard,
    Desk,
}

impl TrackMode {
    fn category_submode(self) -> (u8, u8) {
        match self {
            TrackMode::Off => (0, 0),
            TrackMode::Normal => (2, 0),
            TrackMode::UpperBody => (2, 1),
            TrackMode::CloseUp => (2, 2),
            TrackMode::Headless => (2, 3),
            TrackMode::LowerBody => (2, 4),
            TrackMode::Group => (1, 0),
            TrackMode::Hand => (3, 0),
            TrackMode::Whiteboard => (4, 0),
            TrackMode::Desk => (5, 0),
        }
    }

    pub fn from_str(s: &str) -> Option<TrackMode> {
        Some(match s {
            "off" => TrackMode::Off,
            "normal" | "on" => TrackMode::Normal,
            "upper" | "upper-body" | "upperbody" => TrackMode::UpperBody,
            "close" | "closeup" | "close-up" => TrackMode::CloseUp,
            "headless" => TrackMode::Headless,
            "lower" | "lower-body" | "lowerbody" => TrackMode::LowerBody,
            "group" => TrackMode::Group,
            "hand" => TrackMode::Hand,
            "whiteboard" => TrackMode::Whiteboard,
            "desk" => TrackMode::Desk,
            _ => return None,
        })
    }

    fn from_status(category: u8, submode: u8) -> TrackMode {
        match (category, submode) {
            (0, _) => TrackMode::Off,
            (2, 0) => TrackMode::Normal,
            (2, 1) => TrackMode::UpperBody,
            (2, 2) => TrackMode::CloseUp,
            (2, 3) => TrackMode::Headless,
            (2, 4) => TrackMode::LowerBody,
            (1, _) => TrackMode::Group,
            (3, _) => TrackMode::Hand,
            (4, _) => TrackMode::Whiteboard,
            (5, _) => TrackMode::Desk,
            _ => TrackMode::Normal,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TrackMode::Off => "off",
            TrackMode::Normal => "normal",
            TrackMode::UpperBody => "upper-body",
            TrackMode::CloseUp => "close-up",
            TrackMode::Headless => "headless",
            TrackMode::LowerBody => "lower-body",
            TrackMode::Group => "group",
            TrackMode::Hand => "hand",
            TrackMode::Whiteboard => "whiteboard",
            TrackMode::Desk => "desk",
        }
    }
}

/// Decoded snapshot of the camera's vendor status block plus key UVC controls.
#[derive(Debug, Clone)]
pub struct Status {
    pub asleep: bool,
    pub hdr: bool,
    pub tracking: TrackMode,
    pub tracking_sport: bool,
    pub auto_wb: bool,
    pub wb_temp: i32,
    pub pan_deg: f64,
    pub tilt_deg: f64,
    pub zoom: i32,
    pub auto_exposure: bool,
}

/// An open, awake camera.
pub struct Device {
    fd: VideoFd,
    path: String,
}

static SEQ: AtomicU16 = AtomicU16::new(0);

fn next_seq() -> u16 {
    // Seed once from the pid so concurrent CLI invocations rarely collide. Use
    // a CAS so a racing thread can't roll the counter back to the seed after
    // another thread has already advanced it.
    if SEQ.load(Ordering::Relaxed) == 0 {
        let seed = (std::process::id() as u16) | 0x0100;
        let _ = SEQ.compare_exchange(0, seed, Ordering::Relaxed, Ordering::Relaxed);
    }
    let v = SEQ.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    if v == 0 {
        1
    } else {
        v
    }
}

impl Device {
    /// Discover and open the first Tiny 3 family camera. Holds the fd (blocks sleep).
    pub fn open_default() -> Result<Device> {
        let path = crate::discover::find_device()?;
        Device::open_path(&path)
    }

    /// Open a specific node path. Holds the fd (blocks sleep).
    pub fn open_path(path: &str) -> Result<Device> {
        let fd = VideoFd::open(path)?;
        Ok(Device { fd, path: path.to_string() })
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn fd(&self) -> &VideoFd {
        &self.fd
    }

    // --- framed V3 transport ---

    /// Send a framed SET (fire-and-forget: no mailbox reply is expected).
    fn send_set(&self, receiver: u8, cmd: u16, payload: &[u8]) -> Result<()> {
        let seq = next_seq();
        let f = frame::build(FLAG_SET, seq, receiver, cmd, payload);
        self.fd.xu_set(XU_UNIT, SEL_CMD, &f)
    }

    /// Send a framed GET and poll the mailbox for a reply whose seq AND cmd
    /// match. The mailbox retains the previous reply, so matching both is
    /// required; latency exceeds a fixed sleep, so we poll.
    fn transact_get(&self, receiver: u8, cmd: u16, what: &'static str) -> Result<frame::Reply> {
        let seq = next_seq();
        let f = frame::build(FLAG_GET, seq, receiver, cmd, &[]);
        self.fd.xu_set(XU_UNIT, SEL_CMD, &f)?;
        for _ in 0..12 {
            std::thread::sleep(Duration::from_millis(50));
            let raw = self.fd.xu_get(XU_UNIT, SEL_CMD)?;
            if let Some(r) = frame::parse(&raw) {
                // Match seq AND cmd (the mailbox retains the previous reply),
                // and require it to actually be a device reply — a real reply
                // swaps sender/receiver, so sender is no longer the host. This
                // rejects an echoed request frame sitting in the mailbox.
                if r.seq == seq && r.cmd == cmd && r.sender != frame::SENDER_HOST {
                    return Ok(r);
                }
            }
        }
        Err(Error::NoReply(what))
    }

    // --- power ---

    /// Vendor sleep: LED off, gimbal parks. Verified on Tiny 3 Lite.
    pub fn sleep(&self) -> Result<()> {
        self.send_set(RCV_CAMERA, CMD_DEV_STATUS, &[1, 0, 0, 0])
    }

    /// Vendor wake: LED on, gimbal lifts back up. Verified on Tiny 3 Lite.
    pub fn wake(&self) -> Result<()> {
        self.send_set(RCV_CAMERA, CMD_DEV_STATUS, &[0, 0, 0, 0])
    }

    // --- identity ---

    /// 14-character device serial, via the Upgrade subsystem (safe GET).
    pub fn serial(&self) -> Result<String> {
        let r = self.transact_get(RCV_UPGRADE, CMD_GET_SN, "serial number")?;
        Ok(String::from_utf8_lossy(&r.payload).trim_end_matches('\0').to_string())
    }

    /// 24-byte device UUID as hex.
    pub fn uuid_hex(&self) -> Result<String> {
        let r = self.transact_get(RCV_UPGRADE, CMD_GET_UUID, "uuid")?;
        Ok(r.payload.iter().map(|b| format!("{b:02x}")).collect())
    }

    // --- AI tracking ---

    /// Set the AI tracking mode (or Off). Raw TLV on selector 6.
    pub fn set_tracking(&self, mode: TrackMode) -> Result<()> {
        let (category, submode) = mode.category_submode();
        self.fd.xu_set(XU_UNIT, SEL_STATUS, &[TLV_AI_TRACK, 0x02, category, submode])
    }

    /// Set tracking speed. EXPERIMENTAL on Tiny 3 (frame from Tiny 2 capture).
    /// `sport` = fast; otherwise standard.
    pub fn set_track_speed(&self, sport: bool) -> Result<()> {
        let v: u8 = if sport { 0x02 } else { 0x00 };
        self.send_set(RCV_AI, CMD_TRACK_SPEED, &[v])
    }

    // --- gimbal ---

    /// Return the gimbal to mechanical center (pan 0°, tilt 0°).
    ///
    /// Implemented via the UVC pan/tilt controls rather than the vendor
    /// recenter frame (0x00C3) on purpose: uvcvideo caches pan/tilt readback
    /// from the last value it wrote, so a vendor recenter would leave `status`
    /// reporting the stale pre-recenter angle while the camera is physically
    /// centered. Writing pan=0/tilt=0 both moves the gimbal AND keeps the
    /// readback honest. The vendor frame is preserved in `recenter_vendor`.
    pub fn recenter(&self) -> Result<()> {
        self.fd.set_ctrl(controls::CID_PAN_ABSOLUTE, 0)?;
        self.fd.set_ctrl(controls::CID_TILT_ABSOLUTE, 0)
    }

    /// Vendor recenter/home frame. Moves the gimbal but desyncs the UVC
    /// pan/tilt readback cache — prefer [`recenter`]. Useful to also reset AI
    /// tracking drift.
    #[allow(dead_code)]
    pub fn recenter_vendor(&self) -> Result<()> {
        self.send_set(RCV_GIMBAL, CMD_RECENTER, &[0, 0, 0, 0, 0, 0])
    }

    /// Move the gimbal to an absolute motor angle (degrees). EXPERIMENTAL —
    /// prefer the UVC pan/tilt controls in `set_pan_deg`/`set_tilt_deg`.
    #[allow(dead_code)]
    pub fn gimbal_move(&self, roll: f32, pitch: f32, yaw: f32) -> Result<()> {
        let mut p = Vec::with_capacity(12);
        p.extend_from_slice(&roll.to_le_bytes());
        p.extend_from_slice(&pitch.to_le_bytes());
        p.extend_from_slice(&yaw.to_le_bytes());
        self.send_set(RCV_AI, CMD_GIMBAL_MOVE, &p)
    }

    /// Absolute pan via the standard UVC control. `deg` is clamped to ±130°.
    pub fn set_pan_deg(&self, deg: f64) -> Result<()> {
        self.fd.set_ctrl(controls::CID_PAN_ABSOLUTE, controls::deg_to_asec(deg, controls::PAN_MAX))
    }

    /// Absolute tilt via the standard UVC control. `deg` is clamped to ±90°.
    pub fn set_tilt_deg(&self, deg: f64) -> Result<()> {
        self.fd.set_ctrl(controls::CID_TILT_ABSOLUTE, controls::deg_to_asec(deg, controls::TILT_MAX))
    }

    /// Absolute zoom 0..=100 via the standard UVC control.
    pub fn set_zoom(&self, zoom: i32) -> Result<()> {
        if !(0..=100).contains(&zoom) {
            return Err(Error::OutOfRange { what: "zoom", min: 0, max: 100, got: zoom as i64 });
        }
        self.fd.set_ctrl(controls::CID_ZOOM_ABSOLUTE, zoom)
    }

    // --- image ---

    /// Pin manual white balance at `temp` Kelvin, in the firmware-safe order.
    pub fn set_wb_temp(&self, temp: i32) -> Result<()> {
        if !(controls::WB_TEMP_MIN..=controls::WB_TEMP_MAX).contains(&temp) {
            return Err(Error::OutOfRange {
                what: "white balance temperature (Kelvin)",
                min: controls::WB_TEMP_MIN as i64,
                max: controls::WB_TEMP_MAX as i64,
                got: temp as i64,
            });
        }
        controls::pin_white_balance(&self.fd, temp)
    }

    /// Restore auto white balance (the safe watchable fallback).
    pub fn set_wb_auto(&self) -> Result<()> {
        controls::white_balance_auto(&self.fd)
    }

    /// HDR/WDR on or off. Raw TLV on selector 6.
    pub fn set_hdr(&self, on: bool) -> Result<()> {
        self.fd.xu_set(XU_UNIT, SEL_STATUS, &[TLV_HDR, 0x01, on as u8])
    }

    /// Field of view: 0 wide(86°), 1 medium(78°), 2 narrow(65°).
    pub fn set_fov(&self, level: u8) -> Result<()> {
        if level > 2 {
            return Err(Error::OutOfRange { what: "fov level", min: 0, max: 2, got: level as i64 });
        }
        self.fd.xu_set(XU_UNIT, SEL_STATUS, &[TLV_FOV, 0x01, level])
    }

    /// Face-priority auto-exposure: false = global metering, true = face.
    /// Requires auto-exposure on.
    pub fn set_face_ae(&self, face: bool) -> Result<()> {
        self.fd.xu_set(XU_UNIT, SEL_STATUS, &[TLV_FACE_AE, 0x01, face as u8])
    }

    /// Auto exposure on/off (standard UVC menu control).
    pub fn set_auto_exposure(&self, auto: bool) -> Result<()> {
        let v = if auto { controls::EXPOSURE_AUTO_MODE } else { controls::EXPOSURE_MANUAL_MODE };
        self.fd.set_ctrl(controls::CID_EXPOSURE_AUTO, v)
    }

    /// Manual exposure time (device units, 1..=2500 ≈ 0.1ms..250ms). Sets
    /// manual mode first.
    pub fn set_exposure(&self, value: i32) -> Result<()> {
        if !(1..=2500).contains(&value) {
            return Err(Error::OutOfRange { what: "exposure", min: 1, max: 2500, got: value as i64 });
        }
        self.fd.set_ctrl(controls::CID_EXPOSURE_AUTO, controls::EXPOSURE_MANUAL_MODE)?;
        self.fd.set_ctrl(controls::CID_EXPOSURE_ABSOLUTE, value)
    }

    // --- status ---

    /// Read and decode the full status: vendor block + key UVC controls.
    pub fn status(&self) -> Result<Status> {
        let s = self.fd.xu_get(XU_UNIT, SEL_STATUS)?;
        let pan = self.fd.get_ctrl(controls::CID_PAN_ABSOLUTE).unwrap_or(0);
        let tilt = self.fd.get_ctrl(controls::CID_TILT_ABSOLUTE).unwrap_or(0);
        let zoom = self.fd.get_ctrl(controls::CID_ZOOM_ABSOLUTE).unwrap_or(0);
        let auto_wb = self.fd.get_ctrl(controls::CID_AUTO_WHITE_BALANCE).unwrap_or(0) != 0;
        let wb_temp = self.fd.get_ctrl(controls::CID_WHITE_BALANCE_TEMPERATURE).unwrap_or(0);
        let ae = self.fd.get_ctrl(controls::CID_EXPOSURE_AUTO).unwrap_or(0);
        Ok(Status {
            asleep: s[ST_SLEEP] != 0,
            hdr: s[ST_HDR] != 0,
            tracking: TrackMode::from_status(s[ST_AI_CATEGORY], s[ST_AI_SUBMODE]),
            tracking_sport: s[ST_TRACK_SPEED] == 2,
            auto_wb,
            wb_temp,
            pan_deg: pan as f64 / controls::ASEC_PER_DEG,
            tilt_deg: tilt as f64 / controls::ASEC_PER_DEG,
            zoom,
            auto_exposure: ae == controls::EXPOSURE_AUTO_MODE,
        })
    }
}
