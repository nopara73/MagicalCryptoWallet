//! Non-shipping fixture: actual shared Frame protocol + actual scanner Runtime.
//! Shared sources are pinned read-only exports from the tested master commit.
#[allow(dead_code)]
#[path = "../../../.artifacts/qr-scanning/shared-src/bridge.rs"]
mod bridge;
#[allow(dead_code)]
#[path = "../../../.artifacts/qr-scanning/shared-src/qr.rs"]
mod qr;
#[allow(dead_code)]
#[path = "../../src/scan_service/mod.rs"]
mod scan_service;
use bridge::Frame;
use scan_service::wire;
use std::io;
use std::sync::mpsc;

fn main() {
    let runtime = wire::Runtime::default();
    let reader = runtime.reader();
    let (send, receive) = mpsc::sync_channel(16);
    let input = std::thread::spawn(move || {
        let mut stdin = io::stdin().lock();
        while let Ok(Some(frame)) = Frame::read(&mut stdin) {
            if wire::handles(frame.operation) {
                if frame.kind == bridge::REQUEST
                    && reader.register(frame.id, frame.operation).is_err()
                {
                    break;
                }
                if frame.kind == bridge::CANCEL {
                    reader.cancel(frame.id, frame.operation);
                    continue;
                }
            }
            if send.send(frame).is_err() {
                break;
            }
        }
        reader.disconnected();
    });
    let mut stdout = io::stdout().lock();
    let mut ready = false;
    while let Ok(frame) = receive.recv() {
        runtime.reap();
        let reply =
            if !ready && frame.kind == bridge::HELLO && frame.id == 0 && frame.operation == 0 {
                ready = true;
                frame.reply(Vec::new())
            } else if ready && frame.kind == bridge::REQUEST && wire::handles(frame.operation) {
                if frame.operation == wire::FINISH {
                    eprintln!("finish-start {}", frame.id);
                }
                match runtime.handle(frame.id, frame.operation, &frame.payload) {
                    Ok(payload) => frame.reply(payload),
                    Err(error) => frame.error(2, &error.to_string()),
                }
            } else {
                frame.error(3, "unsupported fixture request")
            };
        if reply.write(&mut stdout).is_err() {
            break;
        }
    }
    drop(runtime);
    drop(receive);
    let _ = input.join();
}
