//! t3ctl — command-line control for the OBSBOT Tiny 3 series webcam.

use obsbot_tiny3::config::{self, Preset};
use obsbot_tiny3::{controls, discover, Device, Error, Result, TrackMode};
use std::process::ExitCode;

const USAGE: &str = "\
t3ctl — control an OBSBOT Tiny 3 / Tiny 3 Lite / Tiny 3 SE on Linux

USAGE:
    t3ctl [--device PATH] [--json] <command>

POWER
    sleep                 Put the camera to sleep (LED off, gimbal parks)
    wake                  Wake the camera (LED on, gimbal lifts)
    toggle                Sleep if awake, wake if asleep

INFO
    status                Show current state (opens the device, which wakes it)
    info                  Serial number, UUID, device node
    power                 USB power state only (does NOT wake the camera)

AI TRACKING
    track on|off          Enable/disable tracking
    track <mode>          normal|upper|close|headless|lower|group|hand|whiteboard|desk
    track speed std|sport Tracking responsiveness (experimental)

GIMBAL
    recenter              Return gimbal to center (a.k.a. park/home)
    pan <deg>             Absolute pan  (-130..130)
    tilt <deg>            Absolute tilt (-90..90)
    zoom <0..100>         Absolute zoom
    preset save <name>    Save current pan/tilt/zoom as a named preset
    preset recall <name>  Move to a saved preset
    preset list           List saved presets

IMAGE
    wb auto               Auto white balance (safe fallback)
    wb temp <2000..10000> Manual white-balance temperature (Kelvin)
    wb pin                Pin manual WB to the configured target (config file)
    hdr on|off            HDR / WDR
    fov wide|medium|narrow
    exposure auto         Auto exposure
    exposure <1..2500>    Manual exposure time (0.1ms units)
    face-ae on|off        Face-priority auto exposure

MISC
    reset                 Safe defaults: wb auto, tracking off, recenter, wake
    -h, --help            This help

Notes: opening the camera wakes it. `power` and this help do not open it.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Error::Usage(msg)) => {
            eprintln!("t3ctl: {msg}\n\nRun `t3ctl --help` for usage.");
            ExitCode::from(2)
        }
        Err(e) => {
            eprintln!("t3ctl: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(mut args: Vec<String>) -> Result<()> {
    let mut device_path: Option<String> = None;
    let mut json = false;

    // Global options may precede the subcommand.
    while let Some(first) = args.first() {
        match first.as_str() {
            "-h" | "--help" | "help" => {
                print!("{USAGE}");
                return Ok(());
            }
            "--json" => {
                json = true;
                args.remove(0);
            }
            "--device" | "-d" => {
                args.remove(0);
                device_path = Some(
                    args.first()
                        .ok_or_else(|| Error::Usage("--device needs a PATH".into()))?
                        .clone(),
                );
                args.remove(0);
            }
            _ => break,
        }
    }

    let cmd = args.first().cloned().ok_or_else(|| Error::Usage("no command given".into()))?;
    let rest = &args[1..];

    // Commands that must NOT open the device (to avoid waking the camera).
    match cmd.as_str() {
        "power" => return cmd_power(device_path.as_deref(), json),
        _ => {}
    }

    let open = || -> Result<Device> {
        match &device_path {
            Some(p) => Device::open_path(p),
            None => Device::open_default(),
        }
    };

    match cmd.as_str() {
        "sleep" => open()?.sleep(),
        "wake" => open()?.wake(),
        "toggle" => {
            let dev = open()?;
            // Reading the vendor sleep bit does NOT wake the camera, so we can
            // decide without disturbing a sleeping camera. Print the resulting
            // state so a keybinding can surface it in a notification.
            let st = dev.status()?;
            if st.asleep {
                dev.wake()?;
                println!("awake");
            } else {
                dev.sleep()?;
                println!("asleep");
            }
            Ok(())
        }
        "status" => cmd_status(&open()?, json),
        "info" => cmd_info(&open()?, json),
        "track" => cmd_track(&open()?, rest),
        "recenter" | "park" | "home" => open()?.recenter(),
        "pan" => open()?.set_pan_deg(need_f64(rest, "pan degrees")?),
        "tilt" => open()?.set_tilt_deg(need_f64(rest, "tilt degrees")?),
        "zoom" => open()?.set_zoom(need_i32(rest, "zoom 0..100")?),
        "preset" => cmd_preset(&open()?, rest),
        "wb" => cmd_wb(&open()?, rest),
        "hdr" => open()?.set_hdr(need_on_off(rest, "hdr")?),
        "fov" => cmd_fov(&open()?, rest),
        "exposure" => cmd_exposure(&open()?, rest),
        "face-ae" => open()?.set_face_ae(need_on_off(rest, "face-ae")?),
        "reset" => cmd_reset(&open()?),
        other => return Err(Error::Usage(format!("unknown command '{other}'"))),
    }
}

fn cmd_power(device_path: Option<&str>, json: bool) -> Result<()> {
    let path = match device_path {
        Some(p) => p.to_string(),
        None => discover::find_device()?,
    };
    let state = discover::usb_power_state(&path).unwrap_or_else(|| "unknown".into());
    let node = discover::real_node(&path).unwrap_or_else(|_| path.clone());
    if json {
        println!("{{\"usb_power\":\"{state}\",\"node\":\"{node}\"}}");
    } else {
        println!("USB power state : {state}");
        println!("device node     : {node}");
        println!("(this is USB autosuspend, not the vendor sleep bit — the LED is the ground truth)");
    }
    Ok(())
}

fn cmd_status(dev: &Device, json: bool) -> Result<()> {
    let s = dev.status()?;
    if json {
        println!(
            "{{\"asleep\":{},\"tracking\":\"{}\",\"tracking_sport\":{},\"hdr\":{},\
             \"auto_wb\":{},\"wb_temp\":{},\"pan_deg\":{:.1},\"tilt_deg\":{:.1},\
             \"zoom\":{},\"auto_exposure\":{}}}",
            s.asleep, s.tracking.label(), s.tracking_sport, s.hdr, s.auto_wb, s.wb_temp,
            s.pan_deg, s.tilt_deg, s.zoom, s.auto_exposure
        );
    } else {
        println!("power       : {}", if s.asleep { "asleep" } else { "awake" });
        println!("tracking    : {}{}", s.tracking.label(), if s.tracking_sport { " (sport)" } else { "" });
        println!("hdr         : {}", onoff(s.hdr));
        println!("white bal.  : {}", if s.auto_wb { "auto".to_string() } else { format!("manual {}K", s.wb_temp) });
        println!("exposure    : {}", if s.auto_exposure { "auto" } else { "manual" });
        println!("gimbal      : pan {:.1}°, tilt {:.1}°, zoom {}", s.pan_deg, s.tilt_deg, s.zoom);
    }
    Ok(())
}

fn cmd_info(dev: &Device, json: bool) -> Result<()> {
    let serial = dev.serial().unwrap_or_else(|_| "(unavailable)".into());
    let uuid = dev.uuid_hex().unwrap_or_else(|_| "(unavailable)".into());
    if json {
        println!("{{\"serial\":\"{serial}\",\"uuid\":\"{uuid}\",\"node\":\"{}\"}}", dev.path());
    } else {
        println!("serial : {serial}");
        println!("uuid   : {uuid}");
        println!("node   : {}", dev.path());
    }
    Ok(())
}

fn cmd_track(dev: &Device, rest: &[String]) -> Result<()> {
    let arg = rest.first().ok_or_else(|| Error::Usage("track needs on|off|toggle|<mode>|speed".into()))?;
    if arg == "toggle" {
        let on = dev.status()?.tracking != TrackMode::Off;
        return if on {
            dev.set_tracking(TrackMode::Off)?;
            println!("off");
            Ok(())
        } else {
            dev.set_tracking(TrackMode::Normal)?;
            println!("on");
            Ok(())
        };
    }
    if arg == "speed" {
        let s = rest.get(1).map(String::as_str);
        return match s {
            Some("sport") | Some("fast") => dev.set_track_speed(true),
            Some("std") | Some("standard") | Some("normal") => dev.set_track_speed(false),
            _ => Err(Error::Usage("track speed std|sport".into())),
        };
    }
    let mode = TrackMode::from_str(arg)
        .ok_or_else(|| Error::Usage(format!("unknown tracking mode '{arg}'")))?;
    dev.set_tracking(mode)
}

fn cmd_preset(dev: &Device, rest: &[String]) -> Result<()> {
    match rest.first().map(String::as_str) {
        Some("list") => {
            let names = config::list_presets();
            if names.is_empty() {
                println!("(no presets saved)");
            } else {
                for n in names {
                    println!("{n}");
                }
            }
            Ok(())
        }
        Some("save") => {
            let name = rest.get(1).ok_or_else(|| Error::Usage("preset save <name>".into()))?;
            let s = dev.status()?;
            config::save_preset(name, Preset { pan_deg: s.pan_deg, tilt_deg: s.tilt_deg, zoom: s.zoom })?;
            println!("saved preset '{name}' (pan {:.1}°, tilt {:.1}°, zoom {})", s.pan_deg, s.tilt_deg, s.zoom);
            Ok(())
        }
        Some("recall") | Some("load") => {
            let name = rest.get(1).ok_or_else(|| Error::Usage("preset recall <name>".into()))?;
            let p = config::load_preset(name)?;
            dev.set_zoom(p.zoom)?;
            dev.set_pan_deg(p.pan_deg)?;
            dev.set_tilt_deg(p.tilt_deg)?;
            println!("recalled preset '{name}'");
            Ok(())
        }
        _ => Err(Error::Usage("preset save|recall|list".into())),
    }
}

fn cmd_wb(dev: &Device, rest: &[String]) -> Result<()> {
    match rest.first().map(String::as_str) {
        Some("auto") => dev.set_wb_auto(),
        Some("temp") => {
            let t = need_i32(&rest[1.min(rest.len())..], "white balance temperature")?;
            dev.set_wb_temp(t)
        }
        // Pin to the configured target temperature (single source of truth for
        // the udev cold-plug hook — reads ~/.config or /etc/obsbot-tiny3/config).
        Some("pin") => dev.set_wb_temp(config::Config::load().wb_temp),
        _ => Err(Error::Usage(format!(
            "wb auto | wb temp <{}..{}> | wb pin",
            controls::WB_TEMP_MIN, controls::WB_TEMP_MAX
        ))),
    }
}

fn cmd_fov(dev: &Device, rest: &[String]) -> Result<()> {
    let level = match rest.first().map(String::as_str) {
        Some("wide") => 0,
        Some("medium") | Some("med") => 1,
        Some("narrow") => 2,
        _ => return Err(Error::Usage("fov wide|medium|narrow".into())),
    };
    dev.set_fov(level)
}

fn cmd_exposure(dev: &Device, rest: &[String]) -> Result<()> {
    match rest.first().map(String::as_str) {
        Some("auto") => dev.set_auto_exposure(true),
        Some(v) => {
            let n: i32 = v.parse().map_err(|_| Error::Usage("exposure auto|<1..2500>".into()))?;
            dev.set_exposure(n)
        }
        None => Err(Error::Usage("exposure auto|<1..2500>".into())),
    }
}

fn cmd_reset(dev: &Device) -> Result<()> {
    dev.wake()?;
    dev.set_tracking(TrackMode::Off)?;
    dev.recenter()?;
    dev.set_wb_auto()?;
    println!("reset: awake, tracking off, recentered, auto WB");
    Ok(())
}

// --- small arg helpers ---

fn need_f64(rest: &[String], what: &str) -> Result<f64> {
    rest.first()
        .ok_or_else(|| Error::Usage(format!("expected {what}")))?
        .parse()
        .map_err(|_| Error::Usage(format!("{what} must be a number")))
}

fn need_i32(rest: &[String], what: &str) -> Result<i32> {
    rest.first()
        .ok_or_else(|| Error::Usage(format!("expected {what}")))?
        .parse()
        .map_err(|_| Error::Usage(format!("{what} must be an integer")))
}

fn need_on_off(rest: &[String], what: &str) -> Result<bool> {
    match rest.first().map(String::as_str) {
        Some("on") | Some("true") | Some("1") => Ok(true),
        Some("off") | Some("false") | Some("0") => Ok(false),
        _ => Err(Error::Usage(format!("{what} on|off"))),
    }
}

fn onoff(b: bool) -> &'static str {
    if b {
        "on"
    } else {
        "off"
    }
}
