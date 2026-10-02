//! Service-owned bounded wire payloads. Shared bridge framing stays host-owned.
#![forbid(unsafe_code)]
use super::{Error, FileSystem, MAX_CHUNK, Stage, WriteService};
use std::{collections::BTreeMap, io, path::PathBuf};

pub const BEGIN: u16 = 0x1000;
pub const APPEND: u16 = 0x1001;
pub const COMMIT: u16 = 0x1002;
pub const ABORT: u16 = 0x1003;
pub const PREPARE: u16 = 0x1004;
pub const VERSION: u16 = 1;
/// BEGIN/PREPARE only: native Windows UTF-16LE code units, including unpaired
/// surrogates. Every non-path operation and every response remains version 1.
pub const WINDOWS_PATH_VERSION: u16 = 2;
pub const MAX_PATH: usize = 128 * 1024;

pub struct Dispatch<F: FileSystem> {
    pub service: WriteService<F>,
    requests: BTreeMap<u64, (u64, u64)>,
    closed: bool,
}
impl<F: FileSystem> Dispatch<F> {
    pub fn new(files: F) -> Self {
        Self {
            service: WriteService::new(files),
            requests: BTreeMap::new(),
            closed: false,
        }
    }
    pub fn close(&mut self) {
        self.closed = true;
        self.requests.clear();
        self.service.abort_all();
    }
    pub fn cancel(&mut self, request_id: u64) {
        let tokens = self
            .requests
            .iter()
            .filter_map(|(token, (begin, last))| {
                if *begin == request_id || *last == request_id {
                    Some(*token)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        for token in tokens {
            self.service.abort(token);
            self.requests.remove(&token);
        }
    }
    /// The host retains the request payload when removing a queued request. Its
    /// request ID has not reached this dispatcher, so cancel the existing token
    /// directly. A canceled queued write can never install or delete artifacts.
    pub fn cancel_pending(&mut self, operation: u16, payload: &[u8]) -> bool {
        if !matches!(operation, APPEND | COMMIT | ABORT)
            || payload.get(..2) != Some(VERSION.to_le_bytes().as_slice())
        {
            return false;
        }
        let Some(bytes) = payload.get(2..10) else {
            return false;
        };
        let Ok(bytes) = bytes.try_into() else {
            return false;
        };
        let token = u64::from_le_bytes(bytes);
        self.requests.remove(&token);
        self.service.abort(token)
    }
    /// Response version:u16, status:u8, then token or typed I/O error. Diagnostics
    /// contain no path/content; filesystem failures are never a bridge fallback.
    pub fn request(&mut self, request_id: u64, operation: u16, payload: &[u8]) -> Vec<u8> {
        let result = self.execute(request_id, operation, payload);
        let mut out = VERSION.to_le_bytes().to_vec();
        match result {
            Ok(token) => {
                out.push(0);
                if let Some(token) = token {
                    out.extend_from_slice(&token.to_le_bytes());
                }
            }
            Err(e) => {
                out.push(1);
                out.push(stage_number(e.stage));
                out.push(kind_number(e.cause.kind()));
                out.extend_from_slice(&e.cause.raw_os_error().unwrap_or(0).to_le_bytes());
            }
        }
        out
    }
    fn execute(
        &mut self,
        request_id: u64,
        operation: u16,
        mut p: &[u8],
    ) -> std::result::Result<Option<u64>, Error> {
        if self.closed {
            return Err(Error {
                stage: Stage::OpenNew,
                cause: io::Error::new(io::ErrorKind::BrokenPipe, "safe file connection closed"),
            });
        }
        let invalid = || Error {
            stage: Stage::WriteNew,
            cause: io::Error::new(io::ErrorKind::InvalidInput, "invalid safe file request"),
        };
        let n16 = |p: &mut &[u8]| -> Option<u16> {
            let n = u16::from_le_bytes(p.get(..2)?.try_into().ok()?);
            *p = &p[2..];
            Some(n)
        };
        let n32 = |p: &mut &[u8]| -> Option<u32> {
            let n = u32::from_le_bytes(p.get(..4)?.try_into().ok()?);
            *p = &p[4..];
            Some(n)
        };
        let n64 = |p: &mut &[u8]| -> Option<u64> {
            let n = u64::from_le_bytes(p.get(..8)?.try_into().ok()?);
            *p = &p[8..];
            Some(n)
        };
        let version = n16(&mut p).ok_or_else(invalid)?;
        if request_id == 0
            || (version != VERSION
                && !(version == WINDOWS_PATH_VERSION && matches!(operation, BEGIN | PREPARE)))
        {
            return Err(invalid());
        }
        match operation {
            BEGIN | PREPARE => {
                let total = if operation == BEGIN {
                    Some(n64(&mut p).ok_or_else(invalid)?)
                } else {
                    None
                };
                let (allocation, disable_file_locking) = if operation == BEGIN {
                    let allocation = n64(&mut p).ok_or_else(invalid)?;
                    let flags = *p.first().ok_or_else(invalid)?;
                    p = &p[1..];
                    if flags > 1 {
                        return Err(invalid());
                    }
                    (allocation, flags == 1)
                } else {
                    (0, false)
                };
                let length = n32(&mut p).ok_or_else(invalid)? as usize;
                if length == 0 || length > MAX_PATH || p.len() != length {
                    return Err(invalid());
                }
                let path = decode_path(version, p).ok_or_else(invalid)?;
                if operation == PREPARE {
                    self.service.prepare(&path)?;
                    return Ok(None);
                }
                let total = match total {
                    Some(u64::MAX) => None,
                    Some(total) => Some(total),
                    None => return Err(invalid()),
                };
                let token =
                    self.service
                        .begin_options(&path, total, allocation, disable_file_locking)?;
                self.requests.insert(token, (request_id, request_id));
                Ok(Some(token))
            }
            APPEND => {
                let token = n64(&mut p).ok_or_else(invalid)?;
                let offset = n64(&mut p).ok_or_else(invalid)?;
                if p.len() > MAX_CHUNK {
                    return Err(invalid());
                }
                if let Some((_, last)) = self.requests.get_mut(&token) {
                    *last = request_id;
                } else {
                    return Err(invalid());
                }
                if let Err(e) = self.service.append(token, offset, p) {
                    self.service.abort(token);
                    self.requests.remove(&token);
                    return Err(e);
                }
                Ok(None)
            }
            COMMIT | ABORT => {
                let token = n64(&mut p).ok_or_else(invalid)?;
                let total = if operation == COMMIT && !p.is_empty() {
                    Some(n64(&mut p).ok_or_else(invalid)?)
                } else {
                    None
                };
                if !p.is_empty() {
                    return Err(invalid());
                }
                if self.requests.remove(&token).is_none() {
                    return Err(invalid());
                }
                if operation == COMMIT {
                    if let Some(total) = total {
                        self.service.finish(token, total)?;
                    } else {
                        self.service.commit(token)?;
                    }
                } else {
                    self.service.abort(token);
                }
                Ok(None)
            }
            _ => Err(invalid()),
        }
    }
}
fn decode_path(version: u16, bytes: &[u8]) -> Option<PathBuf> {
    let path = match version {
        VERSION => {
            let text = std::str::from_utf8(bytes).ok()?;
            if text.contains('\0') {
                return None;
            }
            PathBuf::from(text)
        }
        WINDOWS_PATH_VERSION => {
            #[cfg(windows)]
            {
                use std::{ffi::OsString, os::windows::ffi::OsStringExt};
                if !bytes.len().is_multiple_of(2) {
                    return None;
                }
                let units = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .copied()
                    .map(u16::from_le_bytes)
                    .collect::<Vec<_>>();
                if units.contains(&0) {
                    return None;
                }
                PathBuf::from(OsString::from_wide(&units))
            }
            #[cfg(not(windows))]
            {
                return None;
            }
        }
        _ => return None,
    };
    path.is_absolute().then_some(path)
}
fn stage_number(s: Stage) -> u8 {
    match s {
        Stage::EnsureParent => 1,
        Stage::OpenNew => 2,
        Stage::WriteNew => 3,
        Stage::SyncNew => 4,
        Stage::RemovePreviousBackup => 5,
        Stage::BackupOriginal => 6,
        Stage::InstallNew => 7,
        Stage::RemoveBackup => 8,
        Stage::SyncParent => 9,
        Stage::PreallocateNew => 10,
    }
}
fn kind_number(k: io::ErrorKind) -> u8 {
    match k {
        io::ErrorKind::NotFound => 1,
        io::ErrorKind::PermissionDenied => 2,
        io::ErrorKind::AlreadyExists => 3,
        io::ErrorKind::InvalidInput => 4,
        io::ErrorKind::WouldBlock => 5,
        io::ErrorKind::NotADirectory => 6,
        io::ErrorKind::IsADirectory => 7,
        _ => 255,
    }
}
