//! `obsbot-tiny3` — a Linux control library for the OBSBOT Tiny 3 series webcam.
//!
//! The camera speaks the same UVC extension-unit vendor protocol as the Tiny 2
//! (bUnitID 2, framed "V3" frames on selector 2, raw TLVs on selector 6), which
//! this crate implements from scratch and has verified against a real Tiny 3
//! Lite. See `PROTOCOL.md` for the wire details and firmware quirks.
//!
//! The high-level entry point is [`device::Device`]. Note that **an open fd
//! blocks the camera from sleeping** — see the module docs.

pub mod config;
pub mod controls;
pub mod crc;
pub mod device;
pub mod discover;
pub mod error;
pub mod frame;
pub mod ioctl;

pub use device::{Device, Status, TrackMode};
pub use error::{Error, Result};
