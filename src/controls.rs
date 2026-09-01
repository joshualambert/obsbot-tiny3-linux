//! Standard V4L2 (UVC) control IDs and the white-balance pin sequence.
//!
//! These are the plain UVC controls the Tiny 3 exposes (brightness, WB,
//! exposure, absolute pan/tilt/zoom, focus). Gimbal *positioning* works over
//! these; sleep/wake and AI tracking do not (those go through the vendor
//! protocol in `device.rs`).

use crate::error::Result;
use crate::ioctl::VideoFd;

// V4L2 control IDs (linux/v4l2-controls.h). Confirmed present on this unit.
pub const CID_BRIGHTNESS: u32 = 0x0098_0900;
pub const CID_CONTRAST: u32 = 0x0098_0901;
pub const CID_SATURATION: u32 = 0x0098_0902;
pub const CID_HUE: u32 = 0x0098_0903;
pub const CID_AUTO_WHITE_BALANCE: u32 = 0x0098_090c; // white_balance_automatic (bool)
pub const CID_GAIN: u32 = 0x0098_0913;
pub const CID_POWER_LINE_FREQUENCY: u32 = 0x0098_0918;
pub const CID_WHITE_BALANCE_TEMPERATURE: u32 = 0x0098_091a;
pub const CID_SHARPNESS: u32 = 0x0098_091b;
pub const CID_BACKLIGHT_COMPENSATION: u32 = 0x0098_091c;

pub const CID_EXPOSURE_AUTO: u32 = 0x009a_0901; // menu: 0 Auto, 1 Manual, 3 Aperture-priority
pub const CID_EXPOSURE_ABSOLUTE: u32 = 0x009a_0902;
pub const CID_PAN_ABSOLUTE: u32 = 0x009a_0908; // ±468000, step 3600 (1/3600°)
pub const CID_TILT_ABSOLUTE: u32 = 0x009a_0909; // ±324000, step 3600
pub const CID_FOCUS_ABSOLUTE: u32 = 0x009a_090a;
pub const CID_FOCUS_AUTO: u32 = 0x009a_090c; // focus_automatic_continuous (bool)
pub const CID_ZOOM_ABSOLUTE: u32 = 0x009a_090d; // 0..100

// EXPOSURE_AUTO menu values.
pub const EXPOSURE_AUTO_MODE: i32 = 0;
pub const EXPOSURE_MANUAL_MODE: i32 = 1;

// Gimbal range, in arc-seconds (V4L2 native units), and the degree conversion.
pub const PAN_MAX: i32 = 468_000; // +130°
pub const TILT_MAX: i32 = 324_000; // +90°
pub const ASEC_PER_DEG: f64 = 3600.0;

// White-balance temperature bounds on this firmware.
pub const WB_TEMP_MIN: i32 = 2000;
pub const WB_TEMP_MAX: i32 = 10000;

/// Pin manual white balance at `temp_kelvin`, in the ONLY safe order for this
/// firmware.
///
/// FIRMWARE QUIRK — last-written-control-wins white balance: the firmware
/// treats whichever WB control was written most recently as the active source.
/// So we write `white_balance_automatic=0`, pause, then
/// `white_balance_temperature` LAST, with nothing after it. Writing
/// red_balance/blue_balance after temperature would make 127/127 literal
/// channel gains → a saturated neon-green image. We never touch red/blue
/// balance; they must stay at their defaults (127/127). Control readbacks look
/// correct even while the image is broken, so this ordering — not readback — is
/// the guarantee.
pub fn pin_white_balance(dev: &VideoFd, temp_kelvin: i32) -> Result<()> {
    dev.set_ctrl(CID_AUTO_WHITE_BALANCE, 0)?;
    std::thread::sleep(std::time::Duration::from_millis(300));
    dev.set_ctrl(CID_WHITE_BALANCE_TEMPERATURE, temp_kelvin)?;
    // Nothing after this. On purpose.
    Ok(())
}

/// Read whether auto white balance is currently on (true = auto).
pub fn is_auto_wb(dev: &VideoFd) -> Result<bool> {
    Ok(dev.get_ctrl(CID_AUTO_WHITE_BALANCE)? != 0)
}

/// Restore the always-safe watchable fallback: auto white balance on.
pub fn white_balance_auto(dev: &VideoFd) -> Result<()> {
    dev.set_ctrl(CID_AUTO_WHITE_BALANCE, 1)
}

/// Convert degrees to the clamped arc-second value for a pan/tilt control.
pub fn deg_to_asec(deg: f64, max: i32) -> i32 {
    let raw = (deg * ASEC_PER_DEG).round() as i32;
    raw.clamp(-max, max)
}
