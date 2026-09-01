//! Error type for the crate.

use std::fmt;

#[derive(Debug)]
pub enum Error {
    /// No matching camera node was found under /dev/v4l/by-id/.
    DeviceNotFound(String),
    /// A filesystem or ioctl syscall failed.
    Io(std::io::Error),
    /// The vendor command sent but no matching reply arrived in the mailbox.
    NoReply(&'static str),
    /// A value was outside the control's valid range.
    OutOfRange { what: &'static str, min: i64, max: i64, got: i64 },
    /// The user asked for something we can't do.
    Usage(String),
    /// Config parse/write error.
    Config(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::DeviceNotFound(s) => write!(f, "no OBSBOT Tiny 3 camera found: {s}"),
            Error::Io(e) => write!(f, "{e}"),
            Error::NoReply(what) => write!(f, "no reply from camera for {what}"),
            Error::OutOfRange { what, min, max, got } => {
                write!(f, "{what} must be {min}..={max}, got {got}")
            }
            Error::Usage(s) => write!(f, "{s}"),
            Error::Config(s) => write!(f, "config error: {s}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
