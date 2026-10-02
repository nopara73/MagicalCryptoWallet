//! Bounded image staging for the temporary transport. This owns only ephemeral
//! gray pixels; camera acquisition, UI and wallet state retain their owners.
use super::{Control, Error, Result, matrix::Decoded, raster};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const MAX_BYTES: usize = 16_777_216;
pub const MAX_TRANSFERS: usize = 4;
pub const TRANSFER_LIFETIME: Duration = Duration::from_secs(10);
struct Image {
    width: usize,
    height: usize,
    stride: usize,
    size: usize,
    pixels: Vec<u8>,
    created: Instant,
}
#[derive(Default)]
pub struct Decoder {
    images: Mutex<HashMap<u64, Image>>,
}
fn invalid() -> Error {
    Error::Invalid("QR image transfer is invalid or has expired")
}

impl Decoder {
    pub fn begin(&self, id: u64, width: usize, height: usize, stride: usize) -> Result<()> {
        let size = stride.checked_mul(height).ok_or(Error::Capacity)?;
        if id == 0
            || width == 0
            || height == 0
            || width > 4096
            || height > 4096
            || stride < width
            || stride > 16384
            || width.checked_mul(height).is_none_or(|n| n > MAX_BYTES)
            || size > MAX_BYTES
        {
            return Err(invalid());
        }
        let mut images = self.images.lock().map_err(|_| invalid())?;
        images.retain(|_, i| i.created.elapsed() < TRANSFER_LIFETIME);
        if images.contains_key(&id) {
            return Err(invalid());
        }
        if images.len() >= MAX_TRANSFERS {
            return Err(Error::Capacity);
        }
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(size)
            .map_err(|_| Error::Capacity)?;
        images.insert(
            id,
            Image {
                width,
                height,
                stride,
                size,
                pixels,
                created: Instant::now(),
            },
        );
        Ok(())
    }
    pub fn append(&self, id: u64, offset: usize, bytes: &[u8]) -> Result<()> {
        let mut images = self.images.lock().map_err(|_| invalid())?;
        let i = images.get_mut(&id).ok_or_else(invalid)?;
        if i.created.elapsed() >= TRANSFER_LIFETIME
            || bytes.is_empty()
            || bytes.len() > 262144
            || offset != i.pixels.len()
            || offset.checked_add(bytes.len()).is_none_or(|n| n > i.size)
        {
            images.remove(&id);
            return Err(invalid());
        }
        i.pixels.extend_from_slice(bytes);
        Ok(())
    }
    pub fn finish(&self, id: u64, control: Control<'_>) -> Result<Option<Decoded>> {
        // Remove before decoding, so every failure/cancel releases the pixels.
        let i = self
            .images
            .lock()
            .map_err(|_| invalid())?
            .remove(&id)
            .ok_or_else(invalid)?;
        control.check()?;
        if i.created.elapsed() >= TRANSFER_LIFETIME {
            return Err(Error::Timeout);
        }
        if i.pixels.len() != i.size {
            return Err(invalid());
        }
        if i.width < 21 || i.height < 21 {
            return Ok(None);
        }
        match raster::decode_control(
            raster::Image {
                width: i.width,
                height: i.height,
                stride: i.stride,
                luminance: &i.pixels,
            },
            control,
        ) {
            Ok(d) => Ok(Some(d)),
            Err(Error::NoSymbol) => Ok(None),
            Err(e) => Err(e),
        }
    }
    pub fn abort(&self, id: u64) {
        if let Ok(mut images) = self.images.lock() {
            images.remove(&id);
        }
    }
    pub fn reap(&self) {
        if let Ok(mut images) = self.images.lock() {
            images.retain(|_, i| i.created.elapsed() < TRANSFER_LIFETIME);
        }
    }
    pub fn clear(&self) {
        if let Ok(mut images) = self.images.lock() {
            images.clear();
        }
    }
    #[cfg(test)]
    pub(crate) fn count(&self) -> usize {
        self.images.lock().unwrap().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    #[test]
    fn transfer_bounds_offsets_abort_and_cancel_release() {
        let decoder = Decoder::default();
        assert!(decoder.begin(0, 21, 21, 21).is_err());
        assert!(decoder.begin(1, usize::MAX, 21, 21).is_err());
        decoder.begin(1, 21, 21, 21).unwrap();
        assert!(decoder.append(1, 1, &[255]).is_err());
        assert_eq!(decoder.count(), 0);
        decoder.begin(1, 21, 21, 21).unwrap();
        decoder.append(1, 0, &[255; 441]).unwrap();
        let cancelled = AtomicBool::new(true);
        assert_eq!(
            decoder.finish(
                1,
                Control::new(&cancelled, Instant::now() + Duration::from_secs(1))
            ),
            Err(Error::Cancelled)
        );
        assert_eq!(decoder.count(), 0);
        for id in 1..=4 {
            decoder.begin(id, 1, 1, 1).unwrap();
        }
        assert_eq!(decoder.begin(5, 1, 1, 1), Err(Error::Capacity));
        decoder.abort(1);
        decoder.abort(1);
        assert_eq!(decoder.count(), 3);
        decoder.clear();
        assert_eq!(decoder.count(), 0);
    }
}
