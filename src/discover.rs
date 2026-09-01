//! Find the camera's stable device node without opening it (opening wakes the
//! camera). We match the by-id symlink name, which is stable across
//! re-enumeration — the numeric `/dev/videoN` is not and must never be
//! hardcoded.

use crate::error::{Error, Result};
use std::path::PathBuf;

const BY_ID_DIR: &str = "/dev/v4l/by-id";

/// USB IDs of supported models (vendor is always Remo Tech 0x3564).
/// Used only for the sysfs power-state lookup; node discovery is by name.
pub const VENDOR_ID: &str = "3564";
pub const TINY3_LITE_PID: &str = "ff04";

/// Return the by-id path of the capture node (index0) for the first attached
/// OBSBOT Tiny 3 family camera. Matches "OBSBOT_Tiny_3" so Tiny 3, Tiny 3 Lite
/// and Tiny 3 SE all resolve. Does NOT open the device.
pub fn find_device() -> Result<String> {
    let entries = std::fs::read_dir(BY_ID_DIR).map_err(|e| {
        Error::DeviceNotFound(format!("{BY_ID_DIR} unreadable ({e}) — is the camera plugged in?"))
    })?;
    let mut candidates: Vec<String> = Vec::new();
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let lower = name.to_ascii_lowercase();
        if lower.contains("obsbot_tiny_3") && lower.ends_with("video-index0") {
            candidates.push(format!("{BY_ID_DIR}/{name}"));
        }
    }
    candidates.sort();
    candidates.into_iter().next().ok_or_else(|| {
        Error::DeviceNotFound(
            "no 'OBSBOT_Tiny_3*-video-index0' under /dev/v4l/by-id (camera unplugged, or asleep and \
             de-enumerated?)"
                .into(),
        )
    })
}

/// Resolve a by-id (or any) path to its real `/dev/videoN`, following symlinks.
pub fn real_node(path: &str) -> Result<String> {
    let p = std::fs::canonicalize(path)?;
    Ok(p.to_string_lossy().to_string())
}

/// Best-effort USB runtime power state ("active" / "suspended") for the device
/// backing `video_path`, read from sysfs WITHOUT opening the video node.
///
/// Walks /sys/class/video4linux/<node>/device up to the USB interface and then
/// to the usb_device that owns `power/runtime_status`. Returns None if it
/// cannot be determined. NOTE: "suspended" only means USB autosuspend engaged
/// (all interfaces idle) — it does NOT distinguish a vendor-sleep from a plain
/// idle camera. The physical LED is the only ground truth for vendor sleep.
pub fn usb_power_state(video_path: &str) -> Option<String> {
    let real = real_node(video_path).ok()?;
    let node = real.rsplit('/').next()?; // videoN
    let sys = PathBuf::from(format!("/sys/class/video4linux/{node}/device"));
    let mut dir = std::fs::canonicalize(&sys).ok()?;
    // Ascend until we find a directory that has power/runtime_status AND looks
    // like a usb_device (has a busnum file), or we run out of parents.
    for _ in 0..8 {
        let status = dir.join("power/runtime_status");
        if dir.join("busnum").exists() && status.exists() {
            return std::fs::read_to_string(status).ok().map(|s| s.trim().to_string());
        }
        dir = dir.parent()?.to_path_buf();
    }
    None
}
