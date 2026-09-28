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
        Error::DeviceNotFound(format!(
            "{BY_ID_DIR} unreadable ({e}) — is the camera plugged in?"
        ))
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
            return std::fs::read_to_string(status)
                .ok()
                .map(|s| s.trim().to_string());
        }
        dir = dir.parent()?.to_path_buf();
    }
    None
}

/// Match the ALSA card to the video's physical USB device, never a fixed card
/// number. Reading procfs status does not open a capture stream or record audio.
pub fn audio_active(video_path: &str) -> bool {
    let Ok(real) = real_node(video_path) else {
        return false;
    };
    let Some(node) = real.rsplit('/').next() else {
        return false;
    };
    let Ok(video) = std::fs::canonicalize(format!("/sys/class/video4linux/{node}/device")) else {
        return false;
    };
    let Some(usb) = video.ancestors().find(|p| p.join("idVendor").exists()) else {
        return false;
    };
    let Ok(cards) = std::fs::read_dir("/sys/class/sound") else {
        return false;
    };
    for card in cards.flatten() {
        let name = card.file_name().to_string_lossy().to_string();
        if !name
            .strip_prefix("card")
            .is_some_and(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
        {
            continue;
        }
        let Ok(device) = std::fs::canonicalize(card.path().join("device")) else {
            continue;
        };
        if !device.starts_with(usb) {
            continue;
        }
        let Ok(pcms) = std::fs::read_dir(format!("/proc/asound/{name}")) else {
            continue;
        };
        for pcm in pcms.flatten() {
            let n = pcm.file_name().to_string_lossy().to_string();
            if !n.starts_with("pcm") || !n.ends_with('c') {
                continue;
            }
            let Ok(subs) = std::fs::read_dir(pcm.path()) else {
                continue;
            };
            for sub in subs.flatten() {
                if std::fs::read_to_string(sub.path().join("status"))
                    .is_ok_and(|s| capture_running(&s))
                {
                    return true;
                }
            }
        }
    }
    false
}

fn capture_running(status: &str) -> bool {
    status.lines().any(|l| {
        l.split_once(':')
            .is_some_and(|(k, v)| k.trim() == "state" && v.trim() == "RUNNING")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn audio_activity_requires_running_capture() {
        assert!(capture_running("state: RUNNING\nowner_pid: 123"));
        for s in [
            "closed",
            "state: SUSPENDED",
            "state: PREPARED",
            "state: XRUN",
            "",
        ] {
            assert!(!capture_running(s));
        }
    }
}
