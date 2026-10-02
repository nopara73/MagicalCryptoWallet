//! Test-only, per-thread progress observer for synchronized dispatcher tests.
use std::cell::RefCell;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ProgressCheckpoint {
    /// Work already reserved by the parser, including the next inline step.
    pub charged_work: usize,
    /// Bytes of this inline input already consumed; always greater than zero.
    pub inline_byte_offset: usize,
}

type Observer = Box<dyn FnMut(ProgressCheckpoint)>;
thread_local! {
    static OBSERVER: RefCell<Option<Observer>> = const { RefCell::new(None) };
}

struct Restore(Option<Observer>);
impl Drop for Restore {
    fn drop(&mut self) {
        // Drop a replaced callback after releasing the RefCell borrow.
        let replaced = OBSERVER.with(|slot| slot.replace(self.0.take()));
        drop(replaced);
    }
}

/// Install an observer only on the calling dispatch thread, restoring any
/// previous observer on normal return or unwind. The callback may synchronize
/// with the real Inbox reader. This whole module is absent from production.
pub(crate) fn with_progress_observer<T>(
    observer: impl FnMut(ProgressCheckpoint) + 'static,
    action: impl FnOnce() -> T,
) -> T {
    let _restore = Restore(OBSERVER.with(|slot| slot.replace(Some(Box::new(observer)))));
    action()
}

pub(super) fn notify(checkpoint: ProgressCheckpoint) {
    let observer = OBSERVER.with(|slot| slot.borrow_mut().take());
    if let Some(observer) = observer {
        // Temporarily remove the callback so nested parsing in an observer
        // cannot recursively invoke it. Restore it even if the callback panics.
        let mut restore = Restore(Some(observer));
        if let Some(callback) = restore.0.as_mut() {
            callback(checkpoint);
        }
    }
}
