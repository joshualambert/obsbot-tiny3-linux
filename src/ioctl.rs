//! Raw ioctl layer over a `/dev/videoN` fd: UVC extension-unit queries and
//! standard V4L2 controls. No external V4L2 library — just libc.
//!
//! CRITICAL QUIRK: holding any open fd on the video node keeps the USB
//! interface busy and blocks runtime autosuspend, so the camera can never sleep
//! while a fd is held. (Starting a capture *stream* additionally wakes the
//! camera from vendor sleep — LED on, gimbal live.) Callers that must let the
//! camera sleep while idle (the WB guard) therefore construct no [`VideoFd`]
//! until an app is already holding the camera awake. See `t3-wb-guard`.

use crate::error::{Error, Result};
use std::os::unix::io::RawFd;

// --- ioctl request numbers (x86_64 / generic Linux; _IOC layout) ---
// _IOC(dir, type, nr, size) = (dir<<30)|(size<<16)|(type<<8)|nr
// UVCIOC_CTRL_QUERY = _IOWR('u', 0x21, struct uvc_xu_control_query {16 bytes})
const UVCIOC_CTRL_QUERY: libc::c_ulong = 0xC010_7521;
// VIDIOC_G_CTRL = _IOWR('V', 27, struct v4l2_control {8 bytes})
// nr 27 decimal = 0x1B; type 'V' = 0x56; size 8 → (3<<30)|(8<<16)|(0x56<<8)|0x1B
const VIDIOC_G_CTRL: libc::c_ulong = 0xC008_561B;
// VIDIOC_S_CTRL = _IOWR('V', 28, struct v4l2_control {8 bytes}); nr 28 = 0x1C
const VIDIOC_S_CTRL: libc::c_ulong = 0xC008_561C;

// UVC XU query selectors (uvcvideo.h)
pub const UVC_SET_CUR: u8 = 0x01;
pub const UVC_GET_CUR: u8 = 0x81;
#[allow(dead_code)]
pub const UVC_GET_LEN: u8 = 0x85;
#[allow(dead_code)]
pub const UVC_GET_INFO: u8 = 0x86;

/// struct uvc_xu_control_query — 16 bytes on 64-bit with natural alignment.
#[repr(C)]
struct UvcXuControlQuery {
    unit: u8,
    selector: u8,
    query: u8,
    _pad: u8,
    size: u16,
    _pad2: u16,
    data: *mut u8,
}

/// struct v4l2_control { __u32 id; __s32 value; }
#[repr(C)]
struct V4l2Control {
    id: u32,
    value: i32,
}

/// An open handle to a camera video node. Dropping it closes the fd.
pub struct VideoFd {
    fd: RawFd,
}

impl VideoFd {
    /// Open the node read/write. Resumes the USB device and, while the fd is
    /// held, blocks autosuspend (so the camera can't sleep). Drop to release.
    pub fn open(path: &str) -> Result<VideoFd> {
        let c = std::ffi::CString::new(path)
            .map_err(|_| Error::Usage(format!("bad device path: {path}")))?;
        // O_RDWR is required for control ioctls; no O_NONBLOCK needed.
        // O_CLOEXEC so this fd — whose mere existence keeps the camera awake —
        // never leaks into a child process.
        let fd = unsafe { libc::open(c.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
        if fd < 0 {
            return Err(Error::Io(std::io::Error::last_os_error()));
        }
        Ok(VideoFd { fd })
    }

    /// UVC XU query on `unit`/`selector`. `buf` is both the out-buffer for
    /// GETs and the payload for SETs; its length is the wLength.
    pub fn xu(&self, unit: u8, selector: u8, query: u8, buf: &mut [u8]) -> Result<()> {
        let size = u16::try_from(buf.len())
            .map_err(|_| Error::Usage(format!("XU buffer too large: {} bytes", buf.len())))?;
        let mut q = UvcXuControlQuery {
            unit,
            selector,
            query,
            _pad: 0,
            size,
            _pad2: 0,
            data: buf.as_mut_ptr(),
        };
        let r = unsafe { libc::ioctl(self.fd, UVCIOC_CTRL_QUERY as _, &mut q) };
        if r < 0 {
            return Err(Error::Io(std::io::Error::last_os_error()));
        }
        Ok(())
    }

    /// GET_CUR the 60-byte contents of an XU selector.
    pub fn xu_get(&self, unit: u8, selector: u8) -> Result<[u8; 60]> {
        let mut buf = [0u8; 60];
        self.xu(unit, selector, UVC_GET_CUR, &mut buf)?;
        Ok(buf)
    }

    /// SET_CUR a (<=60 byte) payload to an XU selector, zero-padded to 60.
    pub fn xu_set(&self, unit: u8, selector: u8, data: &[u8]) -> Result<()> {
        if data.len() > 60 {
            return Err(Error::Usage(format!(
                "XU payload {} bytes exceeds 60",
                data.len()
            )));
        }
        let mut buf = [0u8; 60];
        buf[..data.len()].copy_from_slice(data);
        self.xu(unit, selector, UVC_SET_CUR, &mut buf)
    }

    /// VIDIOC_G_CTRL — read a standard V4L2 control by CID.
    pub fn get_ctrl(&self, id: u32) -> Result<i32> {
        let mut c = V4l2Control { id, value: 0 };
        let r = unsafe { libc::ioctl(self.fd, VIDIOC_G_CTRL as _, &mut c) };
        if r < 0 {
            return Err(Error::Io(std::io::Error::last_os_error()));
        }
        Ok(c.value)
    }

    /// VIDIOC_S_CTRL — write a standard V4L2 control by CID.
    ///
    /// Call ORDER matters for white balance on this firmware: the last WB
    /// control written becomes the active source (see `controls::pin_white_balance`).
    pub fn set_ctrl(&self, id: u32, value: i32) -> Result<()> {
        let mut c = V4l2Control { id, value };
        let r = unsafe { libc::ioctl(self.fd, VIDIOC_S_CTRL as _, &mut c) };
        if r < 0 {
            return Err(Error::Io(std::io::Error::last_os_error()));
        }
        Ok(())
    }
}

impl Drop for VideoFd {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd) };
    }
}
