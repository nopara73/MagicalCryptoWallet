use mcw::{
    platform::safe_file::NativeFileSystem,
    safe_file_service::{Boundary, FileSystem, MAX_CHUNK, Observer, WriteService},
};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(1);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "mcw-synthetic-safe-file-{}-{}",
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
fn with_suffix(path: &Path, s: &str) -> PathBuf {
    let mut p = path.as_os_str().to_owned();
    p.push(s);
    p.into()
}
fn existing_read(path: &Path) -> Option<Vec<u8>> {
    let old = with_suffix(path, ".old");
    let new = with_suffix(path, ".new");
    let selected = if path.is_file() && new.is_file() {
        path
    } else if old.is_file() && new.is_file() {
        &old
    } else if path.is_file() {
        path
    } else {
        return None;
    };
    Some(fs::read(selected).unwrap())
}
#[test]
fn exact_binary_large_write_and_artifact_cleanup() {
    let temp = Temp::new();
    let path = temp.file();
    let mut service = WriteService::new(NativeFileSystem);
    let content = (0..1_100_017).map(|i| (i % 251) as u8).collect::<Vec<_>>();
    service.write_bytes(&path, &content).unwrap();
    assert_eq!(fs::read(&path).unwrap(), content);
    assert!(!with_suffix(&path, ".new").exists());
    assert!(!with_suffix(&path, ".old").exists());
    service
        .write_bytes(&path, b"\xef\xbb\xbfsynthetic\0")
        .unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"\xef\xbb\xbfsynthetic\0");
    service.write_bytes(&path, b"").unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"");
}
struct StopAt(Boundary);
impl Observer for StopAt {
    fn reached(&mut self, b: Boundary) -> io::Result<()> {
        if b == self.0 {
            Err(io::Error::other("synthetic fault"))
        } else {
            Ok(())
        }
    }
}
#[test]
fn deterministic_interruption_preserves_existing_read_selection() {
    for boundary in [
        Boundary::ParentReady,
        Boundary::NewOpened,
        Boundary::ChunkWritten,
        Boundary::NewSynced,
        Boundary::PreviousBackupRemoved,
        Boundary::OriginalBackedUp,
        Boundary::NewInstalled,
        Boundary::BackupRemoved,
        Boundary::Completed,
    ] {
        let temp = Temp::new();
        let path = temp.file();
        fs::write(&path, b"old-complete").unwrap();
        fs::write(with_suffix(&path, ".old"), b"previous-backup").unwrap();
        let mut service = WriteService::new(NativeFileSystem);
        let mut observer = StopAt(boundary);
        let result = (|| {
            let id = service.begin_observed(&path, 12, &mut observer)?;
            service.append_observed(id, 0, b"new-complete", &mut observer)?;
            service.commit_observed(id, &mut observer)
        })();
        assert!(result.is_err());
        service.abort_all();
        let recovered = existing_read(&path).unwrap();
        assert!(
            recovered == b"old-complete" || recovered == b"new-complete",
            "{boundary:?}"
        );
    }
}
#[test]
fn abort_incomplete_commit_and_connection_drop_never_promote_partial_bytes() {
    let temp = Temp::new();
    let path = temp.file();
    fs::write(&path, b"old").unwrap();
    let mut service = WriteService::new(NativeFileSystem);
    let id = service.begin(&path, 9).unwrap();
    service.append(id, 0, b"part").unwrap();
    assert!(service.commit(id).is_err());
    assert_eq!(existing_read(&path).unwrap(), b"old");
    assert_eq!(service.active_count(), 0);
    let id = service.begin(&path, 9).unwrap();
    service.append(id, 0, b"part").unwrap();
    assert!(service.abort(id));
    assert_eq!(existing_read(&path).unwrap(), b"old");
    let id = service.begin(&path, 9).unwrap();
    service.append(id, 0, b"part").unwrap();
    drop(service);
    assert_eq!(existing_read(&path).unwrap(), b"old");
}
#[test]
fn destination_collision_does_not_overwrite_or_delete_a_directory() {
    let temp = Temp::new();
    let path = temp.file();
    fs::write(&path, b"original").unwrap();
    let old = with_suffix(&path, ".old");
    fs::create_dir(&old).unwrap();
    let mut service = WriteService::new(NativeFileSystem);
    assert!(service.write_bytes(&path, b"replacement").is_err());
    assert_eq!(fs::read(&path).unwrap(), b"original");
    assert!(old.is_dir());
}
#[test]
fn native_move_rejects_existing_destination_without_replacing_either_file() {
    let temp = Temp::new();
    let source = temp.0.join("source");
    let destination = temp.0.join("destination");
    fs::write(&source, b"source").unwrap();
    fs::write(&destination, b"destination").unwrap();
    assert!(
        NativeFileSystem
            .move_no_replace(&source, &destination)
            .is_err()
    );
    assert_eq!(fs::read(&source).unwrap(), b"source");
    assert_eq!(fs::read(&destination).unwrap(), b"destination");
}
#[test]
fn malformed_offsets_chunks_lengths_and_tokens_are_rejected() {
    let temp = Temp::new();
    let path = temp.file();
    fs::write(&path, b"original").unwrap();
    let mut service = WriteService::new(NativeFileSystem);
    let id = service.begin(&path, 4).unwrap();
    assert!(service.begin(&path, 4).is_err());
    assert!(service.append(id, 1, b"x").is_err());
    assert!(service.append(id, 0, b"oversized").is_err());
    assert!(service.append(id, 0, &vec![0; MAX_CHUNK + 1]).is_err());
    assert!(service.commit(id + 1).is_err());
    service.abort_all();
    assert_eq!(existing_read(&path).unwrap(), b"original");
}
#[test]
fn dispatcher_validates_frames_and_cancellation_closes_sessions() {
    use mcw::safe_file_service::payload::{APPEND, BEGIN, COMMIT, Dispatch};
    let temp = Temp::new();
    let path = temp.file();
    fs::write(&path, b"original").unwrap();
    let mut dispatch = Dispatch::new(NativeFileSystem);
    let text = path.to_str().unwrap().as_bytes();
    let mut begin = 1u16.to_le_bytes().to_vec();
    begin.extend_from_slice(&4u64.to_le_bytes());
    begin.extend_from_slice(&0u64.to_le_bytes());
    begin.push(0);
    begin.extend_from_slice(&(text.len() as u32).to_le_bytes());
    begin.extend_from_slice(text);
    for n in 0..begin.len() {
        assert_eq!(dispatch.request(1, BEGIN, &begin[..n])[2], 1);
    }
    let result = dispatch.request(1, BEGIN, &begin);
    assert_eq!(result[2], 0);
    let token = u64::from_le_bytes(result[3..].try_into().unwrap());
    let mut append = 1u16.to_le_bytes().to_vec();
    append.extend_from_slice(&token.to_le_bytes());
    append.extend_from_slice(&0u64.to_le_bytes());
    append.extend_from_slice(b"part");
    assert_eq!(dispatch.request(2, APPEND, &append), [1, 0, 0]);
    dispatch.cancel(2);
    assert_eq!(dispatch.service.active_count(), 0);
    let mut commit = 1u16.to_le_bytes().to_vec();
    commit.extend_from_slice(&token.to_le_bytes());
    assert_eq!(dispatch.request(3, COMMIT, &commit)[2], 1);
    assert_eq!(existing_read(&path).unwrap(), b"original");
}
#[test]
fn queued_append_commit_and_abort_cancellation_closes_the_existing_token() {
    use mcw::safe_file_service::payload::{ABORT, APPEND, BEGIN, COMMIT, Dispatch};
    for operation in [APPEND, COMMIT, ABORT] {
        let temp = Temp::new();
        let path = temp.file();
        fs::write(&path, b"original").unwrap();
        let mut dispatch = Dispatch::new(NativeFileSystem);
        let text = path.to_str().unwrap().as_bytes();
        let mut begin = 1u16.to_le_bytes().to_vec();
        begin.extend_from_slice(&(if operation == APPEND { 8u64 } else { 4 }).to_le_bytes());
        begin.extend_from_slice(&0u64.to_le_bytes());
        begin.push(0);
        begin.extend_from_slice(&(text.len() as u32).to_le_bytes());
        begin.extend_from_slice(text);
        let opened = dispatch.request(10, BEGIN, &begin);
        assert_eq!(opened[2], 0);
        let token = u64::from_le_bytes(opened[3..].try_into().unwrap());
        let mut append = 1u16.to_le_bytes().to_vec();
        append.extend_from_slice(&token.to_le_bytes());
        append.extend_from_slice(&0u64.to_le_bytes());
        append.extend_from_slice(b"part");
        assert_eq!(dispatch.request(11, APPEND, &append), [1, 0, 0]);
        let mut queued = 1u16.to_le_bytes().to_vec();
        queued.extend_from_slice(&token.to_le_bytes());
        if operation == APPEND {
            queued.extend_from_slice(&4u64.to_le_bytes());
            queued.extend_from_slice(b"tail");
        } else if operation == COMMIT {
            queued.extend_from_slice(&4u64.to_le_bytes());
        }
        dispatch.cancel(99); // The queued request ID has not been dispatched.
        assert_eq!(dispatch.service.active_count(), 1);
        assert!(!dispatch.cancel_pending(BEGIN, &queued));
        assert!(!dispatch.cancel_pending(operation, &queued[..9]));
        assert_eq!(dispatch.service.active_count(), 1);
        assert!(dispatch.cancel_pending(operation, &queued));
        assert!(!dispatch.cancel_pending(operation, &queued));
        assert_eq!(dispatch.service.active_count(), 0);
        assert_eq!(fs::read(&path).unwrap(), b"original");
        assert_eq!(fs::read(with_suffix(&path, ".new")).unwrap(), b"part");
        let mut commit = 1u16.to_le_bytes().to_vec();
        commit.extend_from_slice(&token.to_le_bytes());
        assert_eq!(dispatch.request(100, COMMIT, &commit)[2], 1);
    }
}
#[cfg(windows)]
#[test]
fn windows_utf16_path_requests_preserve_unpaired_units_and_reject_malformed_paths() {
    use mcw::safe_file_service::payload::{APPEND, BEGIN, COMMIT, Dispatch, PREPARE};
    use std::{
        ffi::OsString,
        os::windows::ffi::{OsStrExt, OsStringExt},
    };
    let temp = Temp::new();
    let mut name = "synthetic-".encode_utf16().collect::<Vec<_>>();
    name.extend_from_slice(&[0xd800, 45, 0xdc00]);
    name.extend(".wallet".encode_utf16());
    let path = temp.0.join(OsString::from_wide(&name));
    fs::write(&path, b"original").unwrap();
    fs::write(with_suffix(&path, ".old"), b"older").unwrap();
    let encoded = path
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    let mut prepare = 2u16.to_le_bytes().to_vec();
    prepare.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
    prepare.extend_from_slice(&encoded);
    let mut dispatch = Dispatch::new(NativeFileSystem);
    for length in 0..prepare.len() {
        assert_eq!(dispatch.request(1, PREPARE, &prepare[..length])[2], 1);
    }
    let mut odd = prepare.clone();
    odd.pop();
    odd[2..6].copy_from_slice(&((encoded.len() - 1) as u32).to_le_bytes());
    assert_eq!(dispatch.request(1, PREPARE, &odd)[2], 1);
    let mut nul = prepare.clone();
    nul[6..8].copy_from_slice(&[0, 0]);
    assert_eq!(dispatch.request(1, PREPARE, &nul)[2], 1);
    let relative = [2, 0, 2, 0, 0, 0, b'x', 0];
    assert_eq!(dispatch.request(1, PREPARE, &relative)[2], 1);
    assert_eq!(dispatch.request(1, APPEND, &prepare)[2], 1);
    assert_eq!(dispatch.request(2, PREPARE, &prepare), [1, 0, 0]);
    let mut begin = 2u16.to_le_bytes().to_vec();
    begin.extend_from_slice(&4u64.to_le_bytes());
    begin.extend_from_slice(&0u64.to_le_bytes());
    begin.push(0);
    begin.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
    begin.extend_from_slice(&encoded);
    let opened = dispatch.request(3, BEGIN, &begin);
    assert_eq!(opened[2], 0);
    let token = u64::from_le_bytes(opened[3..].try_into().unwrap());
    let mut append = 1u16.to_le_bytes().to_vec();
    append.extend_from_slice(&token.to_le_bytes());
    append.extend_from_slice(&0u64.to_le_bytes());
    append.extend_from_slice(b"data");
    assert_eq!(dispatch.request(4, APPEND, &append), [1, 0, 0]);
    let mut commit = 1u16.to_le_bytes().to_vec();
    commit.extend_from_slice(&token.to_le_bytes());
    commit.extend_from_slice(&4u64.to_le_bytes());
    assert_eq!(dispatch.request(5, COMMIT, &commit), [1, 0, 0]);
    assert_eq!(fs::read(&path).unwrap(), b"data");
    assert!(!with_suffix(&path, ".new").exists());
    assert!(!with_suffix(&path, ".old").exists());
}
#[test]
fn streaming_commit_requires_an_explicit_exact_final_length() {
    let temp = Temp::new();
    let path = temp.file();
    fs::write(&path, b"original").unwrap();
    let mut service = WriteService::new(NativeFileSystem);
    let token = service.begin_stream(&path).unwrap();
    service.append(token, 0, b"partial").unwrap();
    assert!(service.commit(token).is_err());
    assert_eq!(existing_read(&path).unwrap(), b"original");
    let token = service.begin_stream(&path).unwrap();
    service.append(token, 0, b"partial").unwrap();
    assert!(service.finish(token, 8).is_err());
    assert_eq!(existing_read(&path).unwrap(), b"original");
    let token = service.begin_stream(&path).unwrap();
    service.append(token, 0, b"complete").unwrap();
    service.finish(token, 8).unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"complete");
}
struct AllocationFailure;
impl FileSystem for AllocationFailure {
    type Stream = <NativeFileSystem as FileSystem>::Stream;
    fn ensure_parent(&self, path: &Path) -> io::Result<()> {
        NativeFileSystem.ensure_parent(path)
    }
    fn open_new(&self, path: &Path, disabled: bool) -> io::Result<Self::Stream> {
        NativeFileSystem.open_new(path, disabled)
    }
    fn preallocate(&self, _: &Self::Stream, _: u64) -> io::Result<()> {
        Err(io::Error::from_raw_os_error(if cfg!(windows) {
            112
        } else {
            28
        }))
    }
    fn sync_new(&self, stream: &Self::Stream) -> io::Result<()> {
        NativeFileSystem.sync_new(stream)
    }
    fn file_exists(&self, path: &Path) -> bool {
        NativeFileSystem.file_exists(path)
    }
    fn remove_file(&self, path: &Path) -> io::Result<()> {
        NativeFileSystem.remove_file(path)
    }
    fn move_no_replace(&self, source: &Path, destination: &Path) -> io::Result<()> {
        NativeFileSystem.move_no_replace(source, destination)
    }
    fn sync_parent(&self, path: &Path) -> io::Result<()> {
        NativeFileSystem.sync_parent(path)
    }
}
#[test]
fn fatal_preallocation_closes_and_removes_new_before_reporting_error() {
    let temp = Temp::new();
    let path = temp.file();
    fs::write(&path, b"original").unwrap();
    fs::write(with_suffix(&path, ".new"), b"stale-new").unwrap();
    fs::write(with_suffix(&path, ".old"), b"older").unwrap();
    let mut service = WriteService::new(AllocationFailure);
    let error = service
        .begin_options(&path, Some(8192), 8192, false)
        .unwrap_err();
    assert_eq!(error.stage, mcw::safe_file_service::Stage::PreallocateNew);
    assert_eq!(service.active_count(), 0);
    assert_eq!(fs::read(&path).unwrap(), b"original");
    assert_eq!(fs::read(with_suffix(&path, ".old")).unwrap(), b"older");
    assert!(!with_suffix(&path, ".new").exists());
}
#[test]
fn native_preallocation_reserves_without_exposing_zero_filled_content() {
    let temp = Temp::new();
    let new = with_suffix(&temp.file(), ".new");
    let stream = NativeFileSystem.open_new(&new, false).unwrap();
    NativeFileSystem.preallocate(&stream, 1_100_017).unwrap();
    assert_eq!(fs::metadata(&new).unwrap().len(), 0);
    drop(stream);
}
#[cfg(unix)]
#[test]
fn exclusive_lock_rejects_open_before_truncating_stale_new() {
    let temp = Temp::new();
    let path = temp.file();
    let new = with_suffix(&path, ".new");
    fs::write(&path, b"original").unwrap();
    fs::write(&new, b"stale-new").unwrap();
    let held = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&new)
        .unwrap();
    held.try_lock().unwrap();
    let mut service = WriteService::new(NativeFileSystem);
    assert!(service.begin(&path, 4).is_err());
    assert_eq!(fs::read(&new).unwrap(), b"stale-new");
    drop(held);
}
#[cfg(windows)]
#[test]
fn readonly_backup_preserves_dotnet_cleanup_failure_and_original_bytes() {
    let temp = Temp::new();
    let path = temp.file();
    let old = with_suffix(&path, ".old");
    fs::write(&path, b"original").unwrap();
    fs::write(&old, b"readonly-backup").unwrap();
    let mut permissions = fs::metadata(&old).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&old, permissions).unwrap();
    let mut service = WriteService::new(NativeFileSystem);
    let e = service.write_bytes(&path, b"replacement").unwrap_err();
    assert_eq!(e.stage, mcw::safe_file_service::Stage::RemovePreviousBackup);
    assert_eq!(fs::read(&path).unwrap(), b"original");
    assert_eq!(fs::read(&old).unwrap(), b"readonly-backup");
    assert_eq!(
        fs::read(with_suffix(&path, ".new")).unwrap(),
        b"replacement"
    );
}
