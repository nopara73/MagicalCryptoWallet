//! Cooperative interruption, including deterministic test-only observation.

use super::Error;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) const CHECK_INTERVAL: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Point {
    Entry,
    BeginInsert,
    BeginCommitted,
    Scan,
    ProjectAllocate,
    Project,
    LineInsert,
    ScanFinished,
    EncodeAllocate,
    EncodeLine,
    EncodeBytes,
    EncodeFinished,
}

pub(super) fn check(
    interrupted: Option<&AtomicBool>,
    point: Point,
    progress: usize,
) -> Result<(), Error> {
    if interrupted.is_some_and(|flag| flag.load(Ordering::Acquire)) {
        return Err(Error::Interrupted);
    }
    #[cfg(test)]
    OBSERVER.with(|observer| {
        if let Some(observe) = observer.borrow_mut().as_mut() {
            observe(point, progress);
        }
    });
    let _ = (point, progress);
    if interrupted.is_some_and(|flag| flag.load(Ordering::Acquire)) {
        return Err(Error::Interrupted);
    }
    Ok(())
}

#[cfg(test)]
type Observer = Box<dyn FnMut(Point, usize)>;
#[cfg(test)]
thread_local! {
    static OBSERVER: std::cell::RefCell<Option<Observer>> = const { std::cell::RefCell::new(None) };
}

/// Compiled only in tests. The callback observes an actual checkpoint in the
/// real controlled path; it may synchronize with a separate cancelling thread.
#[cfg(test)]
pub(crate) fn observe<T>(
    observer: impl FnMut(Point, usize) + 'static,
    run: impl FnOnce() -> T,
) -> T {
    struct Reset(Option<Observer>);
    impl Drop for Reset {
        fn drop(&mut self) {
            OBSERVER.with(|observer| *observer.borrow_mut() = self.0.take());
        }
    }
    let _reset = Reset(OBSERVER.with(|slot| slot.replace(Some(Box::new(observer)))));
    run()
}
