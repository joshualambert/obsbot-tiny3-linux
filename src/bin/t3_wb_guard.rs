//! t3-wb-guard — keep the OBSBOT Tiny 3's manual white balance pinned.
//!
//! Chromium apps (Slack, Chrome, Electron) re-enable auto white balance every
//! time they open the camera, and this camera's auto WB drowns in green-walled
//! rooms. This daemon re-pins configured manual WB whenever an app flips it.
//!
//! Two firmware quirks shape the whole design:
//!
//! 1. Holding any open fd on the video node blocks USB autosuspend, so while
//!    the camera is idle this guard must hold ZERO file descriptors on it or the
//!    camera can never sleep. (Starting a capture stream also wakes the camera
//!    from vendor sleep — LED on, gimbal live.) So idle waiting uses an inotify
//!    IN_OPEN watch, which does NOT open the device. We only open the camera
//!    once an app is already holding it awake.
//!
//! 2. Write order is load-bearing: the last-written WB control becomes the
//!    active source, so temperature is written LAST (see controls::pin_white_balance).
//!
//! State machine:
//!   ABSENT  -> node missing: poll every 5s until it appears.
//!   IDLE    -> node present, no app holds it: hold no fd, block on inotify
//!              IN_OPEN. On open, settle 1.5s (let the app's stream handshake
//!              finish) then go ACTIVE.
//!   ACTIVE  -> an app holds the camera awake: re-pin WB if it went auto, poll
//!              every 2s, and return to IDLE when no app holds it any more.

use obsbot_tiny3::config::Config;
use obsbot_tiny3::controls;
use obsbot_tiny3::discover;
use obsbot_tiny3::ioctl::VideoFd;
use std::os::unix::io::RawFd;
use std::time::Duration;

const SETTLE: Duration = Duration::from_millis(1500);
const ACTIVE_POLL: Duration = Duration::from_secs(2);
const ABSENT_POLL: Duration = Duration::from_secs(5);
const IDLE_REVALIDATE: Duration = Duration::from_secs(60);

fn main() {
    let mut temp = Config::load().wb_temp;
    let mut device_override: Option<String> = None;
    let mut once = false;

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--temp" | "-t" => match args.next() {
                Some(s) => match s.parse() {
                    Ok(v) => temp = v,
                    Err(_) => {
                        eprintln!("t3-wb-guard: invalid --temp value '{s}' (expected Kelvin integer)");
                        std::process::exit(2);
                    }
                },
                None => {
                    eprintln!("t3-wb-guard: --temp requires a value");
                    std::process::exit(2);
                }
            },
            "--device" | "-d" => device_override = args.next(),
            "--once" => once = true,
            "-h" | "--help" => {
                println!(
                    "t3-wb-guard [--temp KELVIN] [--device PATH] [--once]\n\
                     Re-pins the OBSBOT Tiny 3 manual white balance when apps flip it to auto.\n\
                     Default temperature comes from ~/.config/obsbot-tiny3/config (wb_temp), else 4000K."
                );
                return;
            }
            other => {
                eprintln!("t3-wb-guard: unknown argument '{other}'");
                std::process::exit(2);
            }
        }
    }

    // Clamp rather than exit: this runs as a Restart=always service, so a bad
    // value in the config file must not turn into an infinite crash-restart
    // loop. Warn and carry on with the nearest in-range temperature.
    if !(controls::WB_TEMP_MIN..=controls::WB_TEMP_MAX).contains(&temp) {
        let clamped = temp.clamp(controls::WB_TEMP_MIN, controls::WB_TEMP_MAX);
        eprintln!(
            "t3-wb-guard: temperature {temp}K out of range {}..{}, using {clamped}K",
            controls::WB_TEMP_MIN, controls::WB_TEMP_MAX
        );
        temp = clamped;
    }

    log(&format!("starting; target white balance {temp}K"));

    if once {
        // One-shot: pin now and exit. Unlike the daemon loop this may open an
        // idle camera (waking it briefly), which is fine for a manual/cold-plug
        // pin — the caller asked for it.
        match resolve(&device_override) {
            Some((path, _real)) => {
                pin_if_auto(&path, temp, true);
            }
            None => log("no camera found for --once"),
        }
        return;
    }

    run(device_override, temp);
}

fn run(device_override: Option<String>, temp: i32) {
    loop {
        let (path, real) = match resolve(&device_override) {
            Some(v) => v,
            None => {
                sleep(ABSENT_POLL);
                continue;
            }
        };

        // If an app already holds the camera (e.g. guard restarted mid-stream),
        // skip the idle wait and pin immediately.
        if holders(&real).is_empty() {
            // IDLE: hold no fd; block until some app opens the camera.
            match wait_for_open(&real, IDLE_REVALIDATE) {
                OpenWait::Opened => sleep(SETTLE),
                // Device vanished or timed out: re-validate presence.
                OpenWait::Gone | OpenWait::Timeout => continue,
                OpenWait::Error(e) => {
                    log(&format!("inotify error ({e}); backing off"));
                    sleep(ABSENT_POLL);
                    continue;
                }
            }
        }

        // ACTIVE: an app holds the camera awake, so opening it to pin costs
        // nothing extra. Re-pin on flips, and return to idle when apps leave.
        // `force` carries a failed pin forward: if a re-pin half-completed
        // (auto turned off but the temperature write failed), auto-WB now reads
        // false and the normal is-auto check would skip it forever — so force
        // the next cycle to pin regardless until one succeeds.
        let mut force = false;
        loop {
            if holders(&real).is_empty() {
                break;
            }
            force = !pin_if_auto(&path, temp, force);
            sleep(ACTIVE_POLL);
        }
    }
}

/// Resolve the (by-id path, real /dev/videoN) pair, or None if absent.
fn resolve(device_override: &Option<String>) -> Option<(String, String)> {
    let path = match device_override {
        Some(p) => p.clone(),
        None => discover::find_device().ok()?,
    };
    let real = discover::real_node(&path).ok()?;
    Some((path, real))
}

/// Open the camera and, if auto WB is on (or `force`), re-pin manual WB.
/// Returns true if the white balance is known-good afterwards (already manual,
/// or successfully pinned), false if a failure leaves it uncertain.
///
/// Opens+closes the device. The daemon loop only calls this while an app
/// already holds the camera awake (opening an idle camera wakes it and defeats
/// autosuspend); the `--once` path deliberately calls it on a possibly-idle
/// camera to satisfy an explicit one-shot/cold-plug pin.
fn pin_if_auto(path: &str, temp: i32, force: bool) -> bool {
    let dev = match VideoFd::open(path) {
        Ok(d) => d,
        Err(e) => {
            log(&format!("open failed: {e}"));
            return false;
        }
    };
    // On a read error, assume auto (pin) rather than skip.
    let need = force || controls::is_auto_wb(&dev).unwrap_or(true);
    if !need {
        return true; // already manual; nothing to do
    }
    match controls::pin_white_balance(&dev, temp) {
        Ok(()) => {
            log(&format!("re-pinned white balance to {temp}K"));
            true
        }
        Err(e) => {
            log(&format!("re-pin failed: {e}"));
            false
        }
    }
    // dev dropped here -> fd closed.
}

/// List PIDs (excluding ourselves) holding an fd on `real`, by scanning /proc.
/// Scanning /proc opens no fd on the camera.
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

enum OpenWait {
    Opened,
    Gone,
    Timeout,
    Error(std::io::Error),
}

// inotify masks (linux/inotify.h)
const IN_OPEN: u32 = 0x0000_0020;
const IN_DELETE_SELF: u32 = 0x0000_0400;
const IN_IGNORED: u32 = 0x0000_8000;
const IN_CLOEXEC: libc::c_int = 0o2000000; // O_CLOEXEC

/// Block until an app opens `real`, or the device disappears, or `timeout`.
/// Holds no fd on the camera — only an inotify watch, which does not open it.
///
/// Arms the inotify watch BEFORE re-checking holders, closing the TOCTOU race:
/// the caller's earlier `holders()` scan takes milliseconds, and an app that
/// opens the device during that scan would fire IN_OPEN before the watch
/// existed and be missed. By watching first, then re-checking holders, any
/// open is caught either by the recheck (already open) or by the live watch.
fn wait_for_open(real: &str, timeout: Duration) -> OpenWait {
    let ino = unsafe { libc::inotify_init1(IN_CLOEXEC) };
    if ino < 0 {
        return OpenWait::Error(std::io::Error::last_os_error());
    }
    let cpath = match std::ffi::CString::new(real) {
        Ok(c) => c,
        Err(_) => {
            unsafe { libc::close(ino) };
            return OpenWait::Error(std::io::Error::from(std::io::ErrorKind::InvalidInput));
        }
    };
    let wd = unsafe { libc::inotify_add_watch(ino, cpath.as_ptr(), IN_OPEN | IN_DELETE_SELF) };
    if wd < 0 {
        let e = std::io::Error::last_os_error();
        unsafe { libc::close(ino) };
        // ENOENT/ENODEV: the node vanished between resolve and watch — treat as
        // gone and let the caller re-validate presence. Anything else (e.g.
        // ENOSPC: max_user_watches exhausted) is a real error to back off on,
        // not a spurious "device gone" that would busy-loop.
        return match e.raw_os_error() {
            Some(libc::ENOENT) | Some(libc::ENODEV) => OpenWait::Gone,
            _ => OpenWait::Error(e),
        };
    }

    // Watch is armed: now re-check holders. If an app grabbed the device during
    // the caller's scan-to-arm window, proceed straight to ACTIVE.
    if !holders(real).is_empty() {
        unsafe { libc::close(ino) };
        return OpenWait::Opened;
    }

    let result = poll_inotify(ino, timeout);
    unsafe { libc::close(ino) };
    result
}

fn poll_inotify(ino: RawFd, timeout: Duration) -> OpenWait {
    let mut pfd = libc::pollfd { fd: ino, events: libc::POLLIN, revents: 0 };
    let ms = timeout.as_millis().min(i32::MAX as u128) as libc::c_int;
    let r = unsafe { libc::poll(&mut pfd, 1, ms) };
    if r < 0 {
        let e = std::io::Error::last_os_error();
        if e.kind() == std::io::ErrorKind::Interrupted {
            return OpenWait::Timeout; // treat EINTR as a revalidate tick
        }
        return OpenWait::Error(e);
    }
    if r == 0 {
        return OpenWait::Timeout;
    }
    // Drain events; classify.
    let mut buf = [0u8; 4096];
    let n = unsafe { libc::read(ino, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
    if n <= 0 {
        return OpenWait::Timeout;
    }
    let n = n as usize;
    let mut off = 0usize;
    let header = std::mem::size_of::<libc::inotify_event>(); // 16 bytes
    // Drain the whole batch before classifying. Device-gone wins over an open
    // (a delete anywhere in the batch means the node is going away), so we scan
    // all events rather than returning on the first IN_OPEN.
    let mut saw_open = false;
    while off + header <= n {
        // Fields: wd(i32) mask(u32) cookie(u32) len(u32)
        let mask = u32::from_ne_bytes([buf[off + 4], buf[off + 5], buf[off + 6], buf[off + 7]]);
        let len = u32::from_ne_bytes([buf[off + 12], buf[off + 13], buf[off + 14], buf[off + 15]]) as usize;
        if mask & (IN_DELETE_SELF | IN_IGNORED) != 0 {
            return OpenWait::Gone;
        }
        if mask & IN_OPEN != 0 {
            saw_open = true;
        }
        off += header + len;
    }
    if saw_open {
        OpenWait::Opened
    } else {
        OpenWait::Timeout
    }
}

fn sleep(d: Duration) {
    std::thread::sleep(d);
}

/// Log to stderr (systemd journal captures it). No timestamp — journald adds one.
fn log(msg: &str) {
    eprintln!("t3-wb-guard: {msg}");
}
