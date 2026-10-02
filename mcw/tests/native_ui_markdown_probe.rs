//! Independent test driver; never packaged or registered as an application mode.
#[path = "../src/markdown/mod.rs"]
pub mod markdown;
use std::{fs, sync::atomic::AtomicBool};
fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    assert_eq!(args.len(), 3);
    let mut payload = vec![markdown::SCHEMA];
    payload.extend_from_slice(&fs::read(&args[1]).unwrap());
    fs::write(
        &args[2],
        markdown::dispatch(&payload, &AtomicBool::new(false)).unwrap(),
    )
    .unwrap();
}
