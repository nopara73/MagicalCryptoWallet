//! Development-only tests of the proposed actual application dispatcher.
use super::*;
use crate::safe_file_service::payload::{APPEND, BEGIN, COMMIT};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(1);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "mcw-synthetic-safe-host-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self) -> PathBuf {
        self.0.join("synthetic.wallet")
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn send(frame: Frame, files: &mut SafeFiles, ready: &mut bool, closing: &mut bool) -> Frame {
    let mut output = Vec::new();
    dispatch(&frame, &mut output, ready, &mut None, closing, &[], files).unwrap();
    Frame::read(&mut std::io::Cursor::new(output))
        .unwrap()
        .unwrap()
}
fn opened(
    path: &std::path::Path,
    files: &mut SafeFiles,
    ready: &mut bool,
    closing: &mut bool,
) -> u64 {
    send(
        Frame {
            kind: bridge::HELLO,
            id: 0,
            operation: 0,
            payload: Vec::new(),
        },
        files,
        ready,
        closing,
    );
    let text = path.to_str().unwrap().as_bytes();
    let mut payload = 1u16.to_le_bytes().to_vec();
    payload.extend_from_slice(&4u64.to_le_bytes());
    payload.extend_from_slice(&0u64.to_le_bytes());
    payload.push(0);
    payload.extend_from_slice(&(text.len() as u32).to_le_bytes());
    payload.extend_from_slice(text);
    let reply = send(
        Frame {
            kind: bridge::REQUEST,
            id: 10,
            operation: BEGIN,
            payload,
        },
        files,
        ready,
        closing,
    );
    assert_eq!(reply.kind, bridge::RESPONSE);
    assert_eq!(reply.payload[2], 0);
    u64::from_le_bytes(reply.payload[3..].try_into().unwrap())
}

#[test]
fn application_cancel_aborts_the_native_stream_without_installing_it() {
    let temp = Temp::new();
    let path = temp.file();
    fs::write(&path, b"original").unwrap();
    let mut files = SafeFiles::new(platform::safe_file::NativeFileSystem);
    let (mut ready, mut closing) = (false, false);
    let token = opened(&path, &mut files, &mut ready, &mut closing);
    let mut payload = 1u16.to_le_bytes().to_vec();
    payload.extend_from_slice(&token.to_le_bytes());
    payload.extend_from_slice(&0u64.to_le_bytes());
    payload.extend_from_slice(b"part");
    send(
        Frame {
            kind: bridge::REQUEST,
            id: 11,
            operation: APPEND,
            payload,
        },
        &mut files,
        &mut ready,
        &mut closing,
    );
    let mut output = Vec::new();
    dispatch(
        &Frame {
            kind: bridge::CANCEL,
            id: 11,
            operation: APPEND,
            payload: Vec::new(),
        },
        &mut output,
        &mut ready,
        &mut None,
        &mut closing,
        &[],
        &mut files,
    )
    .unwrap();
    assert!(output.is_empty());
    assert_eq!(files.service.active_count(), 0);
    let mut payload = 1u16.to_le_bytes().to_vec();
    payload.extend_from_slice(&token.to_le_bytes());
    let reply = send(
        Frame {
            kind: bridge::REQUEST,
            id: 12,
            operation: COMMIT,
            payload,
        },
        &mut files,
        &mut ready,
        &mut closing,
    );
    assert_eq!(reply.payload[2], 1);
    assert_eq!(fs::read(&path).unwrap(), b"original");
}

#[test]
fn application_shutdown_closes_streams_and_rejects_late_writes() {
    let temp = Temp::new();
    let path = temp.file();
    fs::write(&path, b"original").unwrap();
    let mut files = SafeFiles::new(platform::safe_file::NativeFileSystem);
    let (mut ready, mut closing) = (false, false);
    let token = opened(&path, &mut files, &mut ready, &mut closing);
    send(
        Frame {
            kind: bridge::REQUEST,
            id: 11,
            operation: bridge::SHUTDOWN,
            payload: Vec::new(),
        },
        &mut files,
        &mut ready,
        &mut closing,
    );
    assert!(closing);
    assert_eq!(files.service.active_count(), 0);
    let mut payload = 1u16.to_le_bytes().to_vec();
    payload.extend_from_slice(&token.to_le_bytes());
    let reply = send(
        Frame {
            kind: bridge::REQUEST,
            id: 12,
            operation: COMMIT,
            payload,
        },
        &mut files,
        &mut ready,
        &mut closing,
    );
    assert_eq!(reply.payload[2], 1);
    assert_eq!(fs::read(&path).unwrap(), b"original");
}
