//! Tiny config file shared by `t3ctl` and `t3-wb-guard`.
//!
//! Format is a minimal `key = value` INI-ish file (no external deps). Lives at
//! `$XDG_CONFIG_HOME/obsbot-tiny3/config` (default `~/.config/obsbot-tiny3/config`).
//! Gimbal presets live alongside it as `presets/<name>`.

use crate::error::{Error, Result};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Target white-balance temperature the guard re-pins to (Kelvin).
    pub wb_temp: i32,
    pub power: String,
    pub tracking: crate::TrackMode,
    pub hdr: Option<bool>,
    pub auto_wb: bool,
}

impl Default for Config {
    fn default() -> Self {
        // 4000K is the tuned baseline for a green-walled office; auto WB drowns
        // in green rooms on this camera. Overridable in the config file.
        Config {
            wb_temp: 4000,
            power: "auto".into(),
            tracking: crate::TrackMode::Off,
            hdr: None,
            auto_wb: false,
        }
    }
}

pub fn config_dir() -> PathBuf {
    if let Ok(x) = std::env::var("XDG_CONFIG_HOME") {
        if !x.is_empty() {
            return PathBuf::from(x).join("obsbot-tiny3");
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    PathBuf::from(home).join(".config/obsbot-tiny3")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config")
}

fn parse(text: &str) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            m.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    m
}

impl Config {
    fn from_text(text: &str) -> Self {
        let mut c = Self::default();
        let m = parse(text);
        if let Some(v) = m.get("wb_temp").and_then(|s| s.parse::<i32>().ok()) {
            c.wb_temp = v.clamp(crate::controls::WB_TEMP_MIN, crate::controls::WB_TEMP_MAX);
        }
        if let Some(v) = m
            .get("power")
            .filter(|v| matches!(v.as_str(), "auto" | "awake" | "sleep"))
        {
            c.power = v.clone();
        }
        if let Some(v) = m
            .get("tracking")
            .and_then(|v| crate::TrackMode::from_str(v))
        {
            c.tracking = v;
        }
        c.hdr = m.get("hdr").and_then(|v| v.parse().ok());
        if let Some(v) = m.get("auto_wb").and_then(|v| v.parse().ok()) {
            c.auto_wb = v;
        }
        c
    }

    pub fn keep_awake(&self, active: bool) -> bool {
        self.power == "awake" || (self.power == "auto" && active)
    }

    /// Load config, falling back to defaults for anything missing/absent.
    ///
    /// Search order: `$XDG_CONFIG_HOME/obsbot-tiny3/config` (per-user), then
    /// `/etc/obsbot-tiny3/config` (system-wide, used by the root/udev context).
    /// The first file that exists wins; missing keys keep their defaults.
    pub fn load() -> Config {
        let mut c = Config::default();
        let candidates = [config_path(), PathBuf::from("/etc/obsbot-tiny3/config")];
        for path in candidates {
            if let Ok(text) = std::fs::read_to_string(&path) {
                c = Self::from_text(&text);
                break;
            }
        }
        c
    }
}

// --- gimbal presets (pan/tilt/zoom triples stored by name) ---

#[derive(Debug, Clone, Copy)]
pub struct Preset {
    pub pan_deg: f64,
    pub tilt_deg: f64,
    pub zoom: i32,
}

fn presets_dir() -> PathBuf {
    config_dir().join("presets")
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub fn save_preset(name: &str, p: Preset) -> Result<()> {
    if !valid_name(name) {
        return Err(Error::Usage(format!(
            "invalid preset name '{name}' (use letters, digits, - and _)"
        )));
    }
    let dir = presets_dir();
    std::fs::create_dir_all(&dir)?;
    let body = format!(
        "pan_deg = {}\ntilt_deg = {}\nzoom = {}\n",
        p.pan_deg, p.tilt_deg, p.zoom
    );
    std::fs::write(dir.join(name), body)?;
    Ok(())
}

pub fn load_preset(name: &str) -> Result<Preset> {
    if !valid_name(name) {
        return Err(Error::Usage(format!("invalid preset name '{name}'")));
    }
    let text = std::fs::read_to_string(presets_dir().join(name))
        .map_err(|_| Error::Config(format!("no preset named '{name}'")))?;
    let m = parse(&text);
    Ok(Preset {
        pan_deg: m.get("pan_deg").and_then(|s| s.parse().ok()).unwrap_or(0.0),
        tilt_deg: m
            .get("tilt_deg")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
        zoom: m.get("zoom").and_then(|s| s.parse().ok()).unwrap_or(0),
    })
}

pub fn list_presets() -> Vec<String> {
    let mut names = Vec::new();
    if let Ok(entries) = std::fs::read_dir(presets_dir()) {
        for e in entries.flatten() {
            names.push(e.file_name().to_string_lossy().to_string());
        }
    }
    names.sort();
    names
}

/// Serialize cooperating CLI/guard operations, including config read-modify-write.
/// Never hold this lock between guard ticks.
pub struct PolicyLock(std::fs::File);
impl PolicyLock {
    pub fn acquire() -> Result<Self> {
        use std::os::fd::AsRawFd;
        std::fs::create_dir_all(config_dir())?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(config_dir().join("control.lock"))?;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(Self(file))
    }
}
impl Drop for PolicyLock {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

/// Caller holds PolicyLock. Preserve unrelated keys and replace atomically.
pub fn save_setting(key: &str, value: &str) -> Result<()> {
    let path = config_path();
    let mut values = parse(&std::fs::read_to_string(&path).or_else(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            match std::fs::read_to_string("/etc/obsbot-tiny3/config") {
                Ok(s) => Ok(s),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
                Err(e) => Err(e),
            }
        } else {
            Err(e)
        }
    })?);
    values.insert(key.into(), value.into());
    let body: String = values.iter().map(|(k, v)| format!("{k} = {v}\n")).collect();
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, body)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_sleep_wins_over_active_calls() {
        for active in [true, false] {
            assert!(!Config::from_text("power = sleep").keep_awake(active));
            assert!(Config::from_text("power = awake").keep_awake(active));
            assert_eq!(Config::from_text("power = auto").keep_awake(active), active);
        }
    }
    #[test]
    fn legacy_config_and_invalid_values_are_safe() {
        let c = Config::from_text("wb_temp = 4000");
        assert!(!c.auto_wb);
        assert_eq!(c.tracking, crate::TrackMode::Off);
        assert_eq!(c.power, "auto");
        let c = Config::from_text("power = nonsense\ntracking = nonsense\nwb_temp = 100000");
        assert_eq!(c.power, "auto");
        assert_eq!(c.tracking, crate::TrackMode::Off);
        assert_eq!(c.wb_temp, 10000);
    }
    #[test]
    fn explicit_wb_auto_and_tracking_survive_reload() {
        let c =
            Config::from_text("auto_wb = true\ntracking = upper-body\nhdr = false\npower = awake");
        assert!(c.auto_wb);
        assert_eq!(c.tracking, crate::TrackMode::UpperBody);
        assert_eq!(c.hdr, Some(false));
        assert!(c.keep_awake(false));
    }
}
