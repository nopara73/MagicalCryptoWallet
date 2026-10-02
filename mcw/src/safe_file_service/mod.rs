//! Retained SafeFile write sequence. Serialized bytes and wallet state belong to
//! the caller; this service owns only the temporary stream and file operations.
#![forbid(unsafe_code)]
pub mod payload;

use std::{
    collections::BTreeMap,
    io::{self, Write},
    path::{Path, PathBuf},
};

pub const MAX_CHUNK: usize = 256 * 1024;
pub const MAX_WRITES: usize = 16;

/// Native filesystem leaf. Rename must fail rather than replace a destination.
pub trait FileSystem {
    type Stream: Write;
    fn ensure_parent(&self, path: &Path) -> io::Result<()>;
    fn open_new(&self, path: &Path, disable_file_locking: bool) -> io::Result<Self::Stream>;
    fn preallocate(&self, stream: &Self::Stream, total: u64) -> io::Result<()>;
    fn sync_new(&self, stream: &Self::Stream) -> io::Result<()>;
    fn file_exists(&self, path: &Path) -> bool;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    fn move_no_replace(&self, source: &Path, destination: &Path) -> io::Result<()>;
    fn sync_parent(&self, path: &Path) -> io::Result<()>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    EnsureParent,
    OpenNew,
    WriteNew,
    SyncNew,
    RemovePreviousBackup,
    BackupOriginal,
    InstallNew,
    RemoveBackup,
    SyncParent,
    PreallocateNew,
}
#[derive(Debug)]
pub struct Error {
    pub stage: Stage,
    pub cause: io::Error,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "safe file write failed at {:?}: {}",
            self.stage, self.cause
        )
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
fn error(stage: Stage, cause: io::Error) -> Error {
    Error { stage, cause }
}
fn invalid(message: &'static str) -> Error {
    error(
        Stage::WriteNew,
        io::Error::new(io::ErrorKind::InvalidInput, message),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boundary {
    ParentReady,
    NewOpened,
    ChunkWritten,
    NewSynced,
    PreviousBackupRemoved,
    OriginalBackedUp,
    NewInstalled,
    BackupRemoved,
    Completed,
}
pub trait Observer {
    fn reached(&mut self, boundary: Boundary) -> io::Result<()>;
}
pub struct NoObserver;
impl Observer for NoObserver {
    fn reached(&mut self, _: Boundary) -> io::Result<()> {
        Ok(())
    }
}
fn observed(observer: &mut impl Observer, boundary: Boundary, stage: Stage) -> Result<()> {
    observer.reached(boundary).map_err(|e| error(stage, e))
}

struct Pending<S> {
    path: PathBuf,
    new: PathBuf,
    old: PathBuf,
    stream: S,
    total: Option<u64>,
    written: u64,
}
pub struct WriteService<F: FileSystem> {
    files: F,
    pending: BTreeMap<u64, Pending<F::Stream>>,
    next: u64,
}
fn suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut p = path.as_os_str().to_os_string();
    p.push(suffix);
    p.into()
}

impl<F: FileSystem> WriteService<F> {
    pub fn new(files: F) -> Self {
        Self {
            files,
            pending: BTreeMap::new(),
            next: 1,
        }
    }
    pub fn active_count(&self) -> usize {
        self.pending.len()
    }
    pub fn prepare(&self, path: &Path) -> Result<()> {
        self.files
            .ensure_parent(&suffix(path, ".new"))
            .map_err(|e| error(Stage::EnsureParent, e))
    }
    pub fn begin(&mut self, path: &Path, total: u64) -> Result<u64> {
        self.begin_observed(path, total, &mut NoObserver)
    }
    pub fn begin_observed(
        &mut self,
        path: &Path,
        total: u64,
        observer: &mut impl Observer,
    ) -> Result<u64> {
        self.open(path, Some(total), 0, false, observer)
    }
    /// Framework encoders can fail after opening .new. An explicitly finished
    /// stream preserves that order without treating partial bytes as complete.
    pub fn begin_stream(&mut self, path: &Path) -> Result<u64> {
        self.open(path, None, 0, false, &mut NoObserver)
    }
    pub fn begin_options(
        &mut self,
        path: &Path,
        total: Option<u64>,
        allocation: u64,
        disable_file_locking: bool,
    ) -> Result<u64> {
        if allocation > i64::MAX as u64 || (allocation != 0 && total != Some(allocation)) {
            return Err(invalid("safe file allocation mismatch"));
        }
        self.open(
            path,
            total,
            allocation,
            disable_file_locking,
            &mut NoObserver,
        )
    }
    fn open(
        &mut self,
        path: &Path,
        total: Option<u64>,
        allocation: u64,
        disable_file_locking: bool,
        observer: &mut impl Observer,
    ) -> Result<u64> {
        if self.pending.len() >= MAX_WRITES {
            return Err(invalid("safe file session capacity"));
        }
        if self.pending.values().any(|p| p.path == path) {
            return Err(error(
                Stage::OpenNew,
                io::Error::new(io::ErrorKind::WouldBlock, "safe file already being written"),
            ));
        }
        let token = self.next;
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| invalid("safe file token exhausted"))?;
        let new = suffix(path, ".new");
        let old = suffix(path, ".old");
        self.files
            .ensure_parent(&new)
            .map_err(|e| error(Stage::EnsureParent, e))?;
        observed(observer, Boundary::ParentReady, Stage::EnsureParent)?;
        let stream = self
            .files
            .open_new(&new, disable_file_locking)
            .map_err(|e| error(Stage::OpenNew, e))?;
        if allocation != 0
            && let Err(cause) = self.files.preallocate(&stream, allocation)
        {
            drop(stream);
            // .NET removes .new on a fatal preallocation failure and ignores
            // cleanup failure while preserving the allocation exception.
            let _ = self.files.remove_file(&new);
            return Err(error(Stage::PreallocateNew, cause));
        }
        observed(observer, Boundary::NewOpened, Stage::OpenNew)?;
        self.pending.insert(
            token,
            Pending {
                path: path.into(),
                new,
                old,
                stream,
                total,
                written: 0,
            },
        );
        Ok(token)
    }
    pub fn append(&mut self, token: u64, offset: u64, bytes: &[u8]) -> Result<()> {
        self.append_observed(token, offset, bytes, &mut NoObserver)
    }
    pub fn append_observed(
        &mut self,
        token: u64,
        offset: u64,
        bytes: &[u8],
        observer: &mut impl Observer,
    ) -> Result<()> {
        if bytes.len() > MAX_CHUNK {
            return Err(invalid("safe file chunk capacity"));
        }
        let p = self
            .pending
            .get_mut(&token)
            .ok_or_else(|| invalid("unknown safe file session"))?;
        let end = offset
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| invalid("safe file length overflow"))?;
        if offset != p.written || p.total.is_some_and(|total| end > total) {
            return Err(invalid("safe file offset or length mismatch"));
        }
        if let Err(cause) = p.stream.write_all(bytes) {
            self.pending.remove(&token);
            return Err(error(Stage::WriteNew, cause));
        }
        p.written = end;
        if let Err(e) = observed(observer, Boundary::ChunkWritten, Stage::WriteNew) {
            self.pending.remove(&token);
            return Err(e);
        }
        Ok(())
    }
    pub fn commit(&mut self, token: u64) -> Result<()> {
        self.commit_observed(token, &mut NoObserver)
    }
    pub fn commit_observed(&mut self, token: u64, observer: &mut impl Observer) -> Result<()> {
        self.complete(token, None, observer)
    }
    pub fn finish(&mut self, token: u64, total: u64) -> Result<()> {
        self.complete(token, Some(total), &mut NoObserver)
    }
    fn complete(
        &mut self,
        token: u64,
        declared: Option<u64>,
        observer: &mut impl Observer,
    ) -> Result<()> {
        // Remove the handle before any fallible step: errors never leave a token
        // that could accidentally commit a partial or already installed file.
        let p = self
            .pending
            .remove(&token)
            .ok_or_else(|| invalid("unknown safe file session"))?;
        let total = p
            .total
            .or(declared)
            .ok_or_else(|| invalid("safe file content incomplete"))?;
        if p.written != total || declared.is_some_and(|n| n != total) {
            return Err(invalid("safe file content incomplete"));
        }
        self.files
            .sync_new(&p.stream)
            .map_err(|e| error(Stage::SyncNew, e))?;
        drop(p.stream);
        self.files
            .sync_parent(&p.new)
            .map_err(|e| error(Stage::SyncParent, e))?;
        observed(observer, Boundary::NewSynced, Stage::SyncNew)?;
        if self.files.file_exists(&p.path) {
            if self.files.file_exists(&p.old) {
                self.files
                    .remove_file(&p.old)
                    .map_err(|e| error(Stage::RemovePreviousBackup, e))?;
                self.files
                    .sync_parent(&p.old)
                    .map_err(|e| error(Stage::SyncParent, e))?;
            }
            observed(
                observer,
                Boundary::PreviousBackupRemoved,
                Stage::RemovePreviousBackup,
            )?;
            self.files
                .move_no_replace(&p.path, &p.old)
                .map_err(|e| error(Stage::BackupOriginal, e))?;
            self.files
                .sync_parent(&p.old)
                .map_err(|e| error(Stage::SyncParent, e))?;
            observed(observer, Boundary::OriginalBackedUp, Stage::BackupOriginal)?;
        }
        self.files
            .move_no_replace(&p.new, &p.path)
            .map_err(|e| error(Stage::InstallNew, e))?;
        self.files
            .sync_parent(&p.path)
            .map_err(|e| error(Stage::SyncParent, e))?;
        observed(observer, Boundary::NewInstalled, Stage::InstallNew)?;
        if self.files.file_exists(&p.old) {
            self.files
                .remove_file(&p.old)
                .map_err(|e| error(Stage::RemoveBackup, e))?;
            self.files
                .sync_parent(&p.old)
                .map_err(|e| error(Stage::SyncParent, e))?;
        }
        observed(observer, Boundary::BackupRemoved, Stage::RemoveBackup)?;
        observed(observer, Boundary::Completed, Stage::RemoveBackup)
    }
    /// Abort/connection loss only closes temporary streams. It never promotes,
    /// deletes, or substitutes a file; the existing SafeFile reader decides.
    pub fn abort(&mut self, token: u64) -> bool {
        self.pending.remove(&token).is_some()
    }
    pub fn abort_all(&mut self) {
        self.pending.clear();
    }
    pub fn write_bytes(&mut self, path: &Path, bytes: &[u8]) -> Result<()> {
        let token = self.begin(path, bytes.len() as u64)?;
        for (index, chunk) in bytes.chunks(MAX_CHUNK).enumerate() {
            if let Err(e) = self.append(token, (index * MAX_CHUNK) as u64, chunk) {
                self.abort(token);
                return Err(e);
            }
        }
        self.commit(token)
    }
}
