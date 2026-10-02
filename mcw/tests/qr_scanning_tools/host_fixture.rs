//! Development-only: patched actual host + actual native child ownership.
//! Static CRT is test tooling only; this is never a Cargo or release target.
#[allow(dead_code)]
#[path = "../../src/scan_service/mod.rs"]
mod scan_service;
#[allow(dead_code)]
#[path = "../../../.artifacts/qr-scanning/shared-src/qr.rs"]
mod qr;
#[allow(dead_code)]
#[path = "../../../.artifacts/qr-scanning/shared-src/bridge.rs"]
mod bridge;
#[allow(dead_code)]
#[path = "../../../.artifacts/qr-scanning/host-source/mcw/src/app.rs"]
mod app;
#[allow(dead_code)]
#[path = "../../../.artifacts/qr-scanning/shared-src/platform.rs"]
mod platform;

fn main() {
    platform::attach_console();
    let args = std::env::args_os().skip(1).collect();
    match app::run(app::Mode::Gui, args) {
        Ok(status) => std::process::exit(status),
        Err(error) => { eprintln!("host fixture: {error}"); std::process::exit(1); }
    }
}
