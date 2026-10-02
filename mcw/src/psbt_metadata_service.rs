//! Versioned byte adapter for the typed PSBT metadata service. No bridge frames,
//! platform handles, wallet state, signing keys or network calls enter this layer.
#![forbid(unsafe_code)]
use crate::{
    psbt::{Limits, Psbt},
    psbt_metadata::{self, KeyAssociation, PreviousTransaction},
};

pub const ENRICH: u16 = 0x0600;
pub const INSPECT: u16 = 0x0601;
pub const VERSION: u16 = 1;
pub const MAX_REQUEST: usize = 32 * 1024 * 1024;
pub const BEGIN: u16 = 0x0602;
pub const APPEND: u16 = 0x0603;
pub const COMMIT: u16 = 0x0604;
pub const READ: u16 = 0x0605;
pub const ABORT: u16 = 0x0606;
pub const CHUNK: usize = 64 * 1024;
pub const MAX_SESSIONS: usize = 4;
pub const MAX_BUFFERED: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    UnsupportedOperation,
    InvalidRequest,
    InvalidMetadata,
    ResourceLimit,
}
impl Error {
    pub fn code(self) -> u16 {
        match self {
            Self::UnsupportedOperation => 3,
            Self::InvalidRequest => 1,
            Self::InvalidMetadata => 2,
            Self::ResourceLimit => 4,
        }
    }
    pub fn message(self) -> &'static str {
        match self {
            Self::UnsupportedOperation => "unsupported PSBT metadata operation",
            Self::InvalidRequest => "invalid PSBT metadata request",
            Self::InvalidMetadata => "invalid or inconsistent PSBT metadata",
            Self::ResourceLimit => "PSBT metadata resource limit exceeded",
        }
    }
}
impl From<psbt_metadata::Error> for Error {
    fn from(error: psbt_metadata::Error) -> Self {
        match error {
            psbt_metadata::Error::Limit => Self::ResourceLimit,
            _ => Self::InvalidMetadata,
        }
    }
}
type Result<T> = std::result::Result<T, Error>;
struct Reader<'a> {
    bytes: &'a [u8],
}
impl<'a> Reader<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8]> {
        let result = self.bytes.get(..length).ok_or(Error::InvalidRequest)?;
        self.bytes = &self.bytes[length..];
        Ok(result)
    }
    fn number(&mut self) -> Result<usize> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()) as usize)
    }
    fn count(&mut self, maximum: usize, minimum_bytes: usize) -> Result<usize> {
        let count = self.number()?;
        if count > maximum {
            return Err(Error::ResourceLimit);
        }
        if count > self.bytes.len() / minimum_bytes {
            return Err(Error::InvalidRequest);
        }
        Ok(count)
    }
    fn blob(&mut self, maximum: usize) -> Result<&'a [u8]> {
        let length = self.number()?;
        if length > maximum {
            return Err(Error::ResourceLimit);
        }
        self.take(length)
    }
    fn finish(self) -> Result<()> {
        if self.bytes.is_empty() {
            Ok(())
        } else {
            Err(Error::InvalidRequest)
        }
    }
}
fn write_number(output: &mut Vec<u8>, value: usize) -> Result<()> {
    let value = u32::try_from(value).map_err(|_| Error::ResourceLimit)?;
    write(output, &value.to_le_bytes())
}
fn write(output: &mut Vec<u8>, bytes: &[u8]) -> Result<()> {
    let length = output
        .len()
        .checked_add(bytes.len())
        .ok_or(Error::ResourceLimit)?;
    if length > MAX_REQUEST {
        return Err(Error::ResourceLimit);
    }
    output
        .try_reserve(bytes.len())
        .map_err(|_| Error::ResourceLimit)?;
    output.extend_from_slice(bytes);
    Ok(())
}
fn write_blob(output: &mut Vec<u8>, bytes: &[u8]) -> Result<()> {
    write_number(output, bytes.len())?;
    write(output, bytes)
}

/// Envelope: version:u16 LE, PSBT:length-u32 LE+bytes. Enrich then carries
/// smart:byte, origins:count-u32 (pubkey33, fingerprint4, path count-u32+
/// indexes-u32, script blob), parents:count-u32 (txid32, transaction blob).
/// Replies start with version:u16; enrich returns a PSBT blob; inspection returns
/// input count, each txid32/vout-u32/presence-byte/[witness-script blob], then
/// output count and script blobs. All payload bytes are private wallet metadata.
pub fn handle(operation: u16, request: &[u8]) -> Result<Vec<u8>> {
    if !matches!(operation, ENRICH | INSPECT) {
        return Err(Error::UnsupportedOperation);
    }
    if request.len() > MAX_REQUEST {
        return Err(Error::ResourceLimit);
    }
    let limits = Limits::default();
    let mut reader = Reader { bytes: request };
    if u16::from_le_bytes(reader.take(2)?.try_into().unwrap()) != VERSION {
        return Err(Error::InvalidRequest);
    }
    let packet =
        Psbt::parse(reader.blob(limits.max_bytes)?, limits).map_err(|_| Error::InvalidMetadata)?;
    let mut output = VERSION.to_le_bytes().to_vec();
    if operation == INSPECT {
        reader.finish()?;
        let inspection = psbt_metadata::inspect(&packet)?;
        write_number(&mut output, inspection.inputs.len())?;
        for input in inspection.inputs {
            write(&mut output, &input.previous_output.txid)?;
            write_number(&mut output, input.previous_output.vout as usize)?;
            write(
                &mut output,
                &[u8::from(input.witness_script_pubkey.is_some())],
            )?;
            if let Some(script) = input.witness_script_pubkey {
                write_blob(&mut output, &script)?;
            }
        }
        write_number(&mut output, inspection.output_scripts.len())?;
        for script in inspection.output_scripts {
            write_blob(&mut output, &script)?;
        }
    } else {
        let smart = match reader.take(1)?[0] {
            0 => false,
            1 => true,
            _ => return Err(Error::InvalidRequest),
        };
        let count = reader.count(packet.inputs().len() + packet.outputs().len(), 45)?;
        let mut keys = Vec::new();
        keys.try_reserve_exact(count)
            .map_err(|_| Error::ResourceLimit)?;
        for _ in 0..count {
            let public_key = reader.take(33)?.try_into().unwrap();
            let fingerprint = reader.take(4)?.try_into().unwrap();
            let path_count = reader.count((limits.max_value_bytes - 4) / 4, 4)?;
            let mut path = Vec::new();
            path.try_reserve_exact(path_count)
                .map_err(|_| Error::ResourceLimit)?;
            for _ in 0..path_count {
                path.push(reader.number()? as u32);
            }
            let script_pubkey = reader.blob(limits.max_value_bytes)?.to_vec();
            keys.push(KeyAssociation {
                public_key,
                fingerprint,
                path,
                script_pubkey,
            });
        }
        let count = reader.count(packet.inputs().len(), 36)?;
        let mut parents = Vec::new();
        parents
            .try_reserve_exact(count)
            .map_err(|_| Error::ResourceLimit)?;
        for _ in 0..count {
            let txid = reader.take(32)?.try_into().unwrap();
            let bytes = reader.blob(limits.max_value_bytes)?.to_vec();
            parents.push(PreviousTransaction { txid, bytes });
        }
        reader.finish()?;
        let enriched = psbt_metadata::enrich(&packet, &keys, &parents, smart)?;
        write_blob(
            &mut output,
            &enriched.serialize().map_err(|_| Error::InvalidMetadata)?,
        )?;
    }
    Ok(output)
}

// PSBT-only provisional transfer state. The application host owns one instance
// for its managed-child connection; dropping that instance on EOF clears every
// private buffer. Request IDs are correlation metadata, never packet fields.
struct Session {
    operation: u16,
    length: usize,
    offset: usize,
    bytes: Vec<u8>,
    response: bool,
    request_id: u64,
}
impl Drop for Session {
    fn drop(&mut self) {
        self.bytes.fill(0);
    }
}
#[derive(Default)]
pub struct Transfers {
    sessions: std::collections::BTreeMap<u64, Session>,
    next_id: u64,
}
impl Transfers {
    pub fn clear(&mut self) {
        self.sessions.clear();
    }
    pub fn cancel(&mut self, request_id: u64) {
        self.sessions
            .retain(|_, session| session.request_id != request_id);
    }
    pub fn active_sessions(&self) -> usize {
        self.sessions.len()
    }
    fn buffered(&self) -> usize {
        self.sessions.values().map(|session| session.length).sum()
    }
    pub fn handle(&mut self, request_id: u64, operation: u16, request: &[u8]) -> Result<Vec<u8>> {
        if request_id == 0 {
            return Err(Error::InvalidRequest);
        }
        if matches!(operation, ENRICH | INSPECT) {
            return handle(operation, request);
        }
        if !(BEGIN..=ABORT).contains(&operation) {
            return Err(Error::UnsupportedOperation);
        }
        if request.len() > CHUNK + 18 {
            return Err(Error::ResourceLimit);
        }
        let mut reader = Reader { bytes: request };
        if u16::from_le_bytes(reader.take(2)?.try_into().unwrap()) != VERSION {
            return Err(Error::InvalidRequest);
        }
        if operation == BEGIN {
            let target = u16::from_le_bytes(reader.take(2)?.try_into().unwrap());
            let length = reader.number()?;
            reader.finish()?;
            if !matches!(target, ENRICH | INSPECT) || length < 6 {
                return Err(Error::InvalidRequest);
            }
            if length > MAX_REQUEST
                || self.sessions.len() >= MAX_SESSIONS
                || self
                    .buffered()
                    .checked_add(length)
                    .is_none_or(|value| value > MAX_BUFFERED)
            {
                return Err(Error::ResourceLimit);
            }
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(length)
                .map_err(|_| Error::ResourceLimit)?;
            self.next_id = self.next_id.checked_add(1).ok_or(Error::ResourceLimit)?;
            let id = self.next_id;
            self.sessions.insert(
                id,
                Session {
                    operation: target,
                    length,
                    offset: 0,
                    bytes,
                    response: false,
                    request_id,
                },
            );
            return Ok(transfer_reply(id));
        }
        let id = u64::from_le_bytes(reader.take(8)?.try_into().unwrap());
        if operation == ABORT {
            reader.finish()?;
            self.sessions.remove(&id);
            return Ok(transfer_reply(id));
        }
        // Remove provisionally, then put back only on success. Any malformed
        // operation for this session clears its upload/result without mutation.
        let mut session = self.sessions.remove(&id).ok_or(Error::InvalidRequest)?;
        session.request_id = request_id;
        let mut output = transfer_reply(id);
        match operation {
            APPEND => {
                let offset = reader.number()?;
                let bytes = reader.blob(CHUNK)?;
                reader.finish()?;
                if session.response
                    || bytes.is_empty()
                    || offset != session.offset
                    || bytes.len() > session.length - session.offset
                {
                    return Err(Error::InvalidRequest);
                }
                session.bytes.extend_from_slice(bytes);
                session.offset += bytes.len();
                write_number(&mut output, session.offset)?;
            }
            COMMIT => {
                reader.finish()?;
                if session.response || session.offset != session.length {
                    return Err(Error::InvalidRequest);
                }
                let result = handle(session.operation, &session.bytes)?;
                if self
                    .buffered()
                    .checked_add(result.len())
                    .is_none_or(|value| value > MAX_BUFFERED)
                {
                    return Err(Error::ResourceLimit);
                }
                session.bytes.fill(0);
                session.bytes = result;
                session.length = session.bytes.len();
                session.offset = 0;
                session.response = true;
                write_number(&mut output, session.length)?;
            }
            READ => {
                let offset = reader.number()?;
                let length = reader.number()?;
                reader.finish()?;
                if !session.response
                    || offset != session.offset
                    || !(1..=CHUNK).contains(&length)
                    || length > session.length - session.offset
                {
                    return Err(Error::InvalidRequest);
                }
                write_number(&mut output, offset)?;
                write_blob(&mut output, &session.bytes[offset..offset + length])?;
                session.offset += length;
                if session.offset == session.length {
                    return Ok(output);
                }
            }
            _ => return Err(Error::UnsupportedOperation),
        }
        self.sessions.insert(id, session);
        Ok(output)
    }
}
fn transfer_reply(id: u64) -> Vec<u8> {
    let mut output = VERSION.to_le_bytes().to_vec();
    output.extend_from_slice(&id.to_le_bytes());
    output
}
