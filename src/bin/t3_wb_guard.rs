//! Reconcile user settings and protect video/audio calls from vendor sleep.
//! Keep the old binary/unit name for existing installations.
use obsbot_tiny3::{
    config::{Config, PolicyLock},
    discover, Device,
};
use std::time::Duration;

fn main() {
    let mut device = None;
    let mut temp = None;
    let mut once = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--device" | "-d" => device = Some(args.next().unwrap_or_else(|| usage())),
            "--temp" | "-t" => {
                temp = Some(
                    args.next()
                        .unwrap_or_else(|| usage())
                        .parse::<i32>()
                        .unwrap_or_else(|_| usage()),
                )
            }
            "--once" => once = true,
            "--help" | "-h" => {
                println!("t3-wb-guard [--device PATH] [--temp KELVIN] [--once]\nEnforces saved tracking, HDR, WB and power; detects video and microphone use.\n--once pins WB only (legacy cold-plug behavior).");
                return;
            }
            _ => usage(),
        }
    }
    let mut held: Option<Device> = None;
    let mut last_config = None;
    let mut last_error = String::new();
    loop {
        let result = (|| -> obsbot_tiny3::Result<()> {
            let _lock = PolicyLock::acquire()?;
            let mut config = Config::load();
            if let Some(t) = temp {
                config.wb_temp = t.clamp(2000, 10000);
            }
            let path = match &device {
                Some(p) => p.clone(),
                None => discover::find_device()?,
            };
            let real = discover::real_node(&path)?;
            let active = !holders(&real).is_empty() || discover::audio_active(&path);
            let keep_awake = config.keep_awake(active);
            if !keep_awake {
                held = None;
            }
            let changed = last_config.as_ref() != Some(&config);
            // No device descriptor while idle. Explicit sleep still reconciles
            // periodically so a voice command cannot override a privacy choice.
            if !once && !keep_awake && !active && !changed && config.power != "sleep" {
                return Ok(());
            }
            let temporary;
            let dev = if keep_awake {
                if held.as_ref().map(|d| d.path()) != Some(path.as_str()) {
                    held = Some(Device::open_path(&path)?);
                }
                held.as_ref().unwrap()
            } else {
                temporary = Device::open_path(&path)?;
                &temporary
            };
            if once {
                return dev.set_wb_temp(config.wb_temp);
            }
            let mut status = dev.status()?;
            // Prevent timed sleep before it happens. Reactively waking the
            // camera every time this timer fires causes visible power cycling.
            // Recheck on every active tick to cover firmware/app/replug resets.
            if keep_awake && status.auto_sleep_seconds != 0 {
                dev.disable_auto_sleep()?;
                eprintln!(
                    "t3-wb-guard: disabled firmware auto-sleep (was {}s)",
                    status.auto_sleep_seconds
                );
            }
            if config.power == "sleep" && !status.asleep {
                dev.sleep()?;
                eprintln!("t3-wb-guard: restored requested sleep");
            } else if keep_awake && status.asleep {
                dev.wake()?;
                std::thread::sleep(Duration::from_millis(300));
                status = dev.status()?;
                eprintln!("t3-wb-guard: restored awake for requested power / active call");
            }
            if config.tracking != status.tracking {
                dev.set_tracking(config.tracking)?;
                eprintln!("t3-wb-guard: restored tracking {}", config.tracking.label());
            }
            if let Some(hdr) = config.hdr {
                if hdr != status.hdr {
                    dev.set_hdr(hdr)?;
                }
            }
            if config.auto_wb {
                if !status.auto_wb {
                    dev.set_wb_auto()?;
                }
            } else if status.auto_wb || status.wb_temp != config.wb_temp || changed {
                dev.set_wb_temp(config.wb_temp)?;
            }
            // Verify vendor SETs as well as UVC controls. A successful ioctl
            // only means transport success, not that firmware applied it.
            let actual = dev.status()?;
            if (keep_awake && (actual.asleep || actual.auto_sleep_seconds != 0))
                || (config.power == "sleep" && !actual.asleep)
                || actual.tracking != config.tracking
                || config.hdr.is_some_and(|h| h != actual.hdr)
                || actual.auto_wb != config.auto_wb
                || (!config.auto_wb && actual.wb_temp != config.wb_temp)
            {
                return Err(obsbot_tiny3::Error::Config(
                    "camera has not accepted requested settings; retrying".into(),
                ));
            }
            last_config = Some(config);
            Ok(())
        })();
        match result {
            Ok(()) => {
                last_error.clear();
            }
            Err(e) => {
                held = None;
                last_config = None;
                let message = e.to_string();
                if message != last_error {
                    eprintln!("t3-wb-guard: {message}");
                    last_error = message;
                }
                if once {
                    std::process::exit(1);
                }
            }
        }
        if once {
            return;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

fn usage() -> ! {
    eprintln!("t3-wb-guard: invalid arguments; see --help");
    std::process::exit(2)
}

/// Video holders excluding our own keep-awake fd and transient control clients.
fn holders(real: &str) -> Vec<u32> {
    let me = std::process::id();
    let mut pids = Vec::new();
    let entries = match std::fs::read_dir("/proc") {
        Ok(e) => e,
        Err(_) => return pids,
    };
    for e in entries.flatten() {
        let name = e.file_name();
        let pid: u32 = match name.to_string_lossy().parse() {
            Ok(p) => p,
            Err(_) => continue,
        };
        if pid == me {
            continue;
        }
        if std::fs::read_to_string(format!("/proc/{pid}/comm"))
            .is_ok_and(|s| matches!(s.trim(), "t3ctl" | "t3-wb-guard"))
        {
            continue;
        }
        let fd_dir = format!("/proc/{pid}/fd");
        if let Ok(fds) = std::fs::read_dir(&fd_dir) {
            for fd in fds.flatten() {
                if let Ok(target) = std::fs::read_link(fd.path()) {
                    if target.to_string_lossy() == real {
                        pids.push(pid);
                        break;
                    }
                }
            }
        }
    }
    pids
}
