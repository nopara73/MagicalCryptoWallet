//! First-party Model 2 decoder. Images and decoded strings are untrusted input;
//! the decoder has no camera, wallet, signing, payment or persistence authority.
#![forbid(unsafe_code)]

mod payload;
pub use payload::StructuredAppend;

pub mod matrix;
pub mod raster;

pub mod decoder;
pub mod text;
pub mod wire;

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    NoSymbol,
    Invalid(&'static str),
    Ambiguous,
    UnsupportedEncoding(u32),
    Cancelled,
    Timeout,
    Capacity,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::NoSymbol => "No readable QR code in this frame.",
            Self::Invalid(message) => message,
            Self::Ambiguous => "Multiple different QR codes are visible.",
            Self::UnsupportedEncoding(_) => "The QR character encoding is unsupported.",
            Self::Cancelled => "QR scanning was cancelled.",
            Self::Timeout => "QR decoding exceeded its time limit.",
            Self::Capacity => "The QR scan resource limit was exceeded.",
        };
        f.write_str(message)
    }
}
impl std::error::Error for Error {}

/// Cancellation is supplied by the application owner, never by the image.
/// The deadline bounds a complete decode attempt, including threshold/detection.
#[derive(Clone, Copy)]
pub struct Control<'a> {
    pub cancelled: &'a AtomicBool,
    pub deadline: Instant,
}
impl Control<'_> {
    pub fn new(cancelled: &AtomicBool, deadline: Instant) -> Control<'_> {
        Control {
            cancelled,
            deadline,
        }
    }
    pub fn check(self) -> Result<()> {
        if self.cancelled.load(Ordering::Acquire) {
            Err(Error::Cancelled)
        } else if Instant::now() >= self.deadline {
            Err(Error::Timeout)
        } else {
            Ok(())
        }
    }
}
