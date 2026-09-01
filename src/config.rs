//! Tiny config file shared by `t3ctl` and `t3-wb-guard`.
//!
//! Format is a minimal `key = value` INI-ish file (no external deps). Lives at
//! `$XDG_CONFIG_HOME/obsbot-tiny3/config` (default `~/.config/obsbot-tiny3/config`).
//! Gimbal presets live alongside it as `presets/<name>`.

use crate::error::{Error, Result};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub struct Config {
    /// Target white-balance temperature the guard re-pins to (Kelvin).
    pub wb_temp: i32,
}

impl Default for Config {
    fn default() -> Self {
        // 4000K is the tuned baseline for a green-walled office; auto WB drowns
        // in green rooms on this camera. Overridable in the config file.
        Config { wb_temp: 4000 }
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
                let m = parse(&text);
                if let Some(v) = m.get("wb_temp").and_then(|s| s.parse().ok()) {
                    c.wb_temp = v;
                }
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
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub fn save_preset(name: &str, p: Preset) -> Result<()> {
    if !valid_name(name) {
        return Err(Error::Usage(format!(
            "invalid preset name '{name}' (use letters, digits, - and _)"
        )));
    }
    let dir = presets_dir();
    std::fs::create_dir_all(&dir)?;
    let body = format!("pan_deg = {}\ntilt_deg = {}\nzoom = {}\n", p.pan_deg, p.tilt_deg, p.zoom);
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
        tilt_deg: m.get("tilt_deg").and_then(|s| s.parse().ok()).unwrap_or(0.0),
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
