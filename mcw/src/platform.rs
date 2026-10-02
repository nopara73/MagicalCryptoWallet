//! Native handles, bindings and unsafe code live exclusively under this module.
use std::{
    process::{Child, Command},
    sync::atomic::{AtomicBool, Ordering},
};
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;
#[cfg(all(windows, mcw_windows_runtime))]
mod windows_runtime;
static SHUTDOWN: AtomicBool = AtomicBool::new(false);
pub fn shutdown_requested() -> bool {
    SHUTDOWN.load(Ordering::Relaxed)
}
#[cfg(unix)]
pub use unix::{ChildLifetime, attach_console, configure_child, initialize, start_installer};
#[cfg(windows)]
pub use windows::{ChildLifetime, attach_console, configure_child, initialize, start_installer};
