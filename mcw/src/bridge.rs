//! The temporary transport adapter. Application services never depend on frames.
#![forbid(unsafe_code)]
use crate::qr::{self, Ecc};
use std::io::{self, Read, Write};

pub const VERSION: u16 = 1;
pub const MAX_FRAME: usize = 1_048_576;
pub const HEADER: usize = 16;
pub const HELLO: u8 = 1;
pub const RESPONSE: u8 = 2;
pub const REQUEST: u8 = 3;
pub const ERROR: u8 = 4;
pub const CANCEL: u8 = 5;
pub const QR: u16 = 1;
pub const SHUTDOWN: u16 = 2;
pub const RESTART: u16 = 3;
pub const UPDATE: u16 = 4;
pub const CRASH: u16 = 5;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub kind: u8,
    pub id: u64,
    pub operation: u16,
    pub payload: Vec<u8>,
}
impl Frame {
    pub fn read(reader: &mut impl Read) -> io::Result<Option<Self>> {
        let mut prefix = [0; 4];
        match reader.read(&mut prefix[..1])? {
            0 => return Ok(None),
            1 => (),
            _ => unreachable!(),
        }
        reader.read_exact(&mut prefix[1..])?;
        let length = u32::from_le_bytes(prefix) as usize;
        if !(HEADER..=MAX_FRAME).contains(&length) {
            return Err(invalid("invalid bridge frame length"));
        }
        let mut bytes = vec![0; length];
        reader.read_exact(&mut bytes)?;
        if u16::from_le_bytes([bytes[0], bytes[1]]) != VERSION {
            return Err(invalid("bridge protocol version mismatch"));
        }
        if !(HELLO..=CANCEL).contains(&bytes[2]) || bytes[3] != 0 || bytes[14..16] != [0, 0] {
            return Err(invalid("invalid bridge header"));
        }
        Ok(Some(Self {
            kind: bytes[2],
            id: u64::from_le_bytes(bytes[4..12].try_into().unwrap()),
            operation: u16::from_le_bytes([bytes[12], bytes[13]]),
            payload: bytes[HEADER..].to_vec(),
        }))
    }
    pub fn write(&self, writer: &mut impl Write) -> io::Result<()> {
        if self.payload.len() > MAX_FRAME - HEADER {
            return Err(invalid("bridge frame exceeds limit"));
        }
        writer.write_all(&((HEADER + self.payload.len()) as u32).to_le_bytes())?;
        writer.write_all(&VERSION.to_le_bytes())?;
        writer.write_all(&[self.kind, 0])?;
        writer.write_all(&self.id.to_le_bytes())?;
        writer.write_all(&self.operation.to_le_bytes())?;
        writer.write_all(&[0, 0])?;
        writer.write_all(&self.payload)?;
        writer.flush()
    }
    pub fn reply(&self, payload: Vec<u8>) -> Self {
        Self {
            kind: RESPONSE,
            id: self.id,
            operation: self.operation,
            payload,
        }
    }
    pub fn error(&self, code: u16, message: &str) -> Self {
        let mut payload = code.to_le_bytes().to_vec();
        payload.extend_from_slice(message.as_bytes());
        Self {
            kind: ERROR,
            id: self.id,
            operation: self.operation,
            payload,
        }
    }
}
pub fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub fn encode_qr(request: &Frame) -> Frame {
    let Some((&level, text)) = request.payload.split_first() else {
        return request.error(1, "missing QR correction level");
    };
    let Some(ecc) = Ecc::from_byte(level) else {
        return request.error(1, "invalid QR correction level");
    };
    let Ok(text) = std::str::from_utf8(text) else {
        return request.error(1, "QR content is not valid UTF-8");
    };
    match qr::encode(text, ecc) {
        Ok(symbol) => {
            let mut bytes = vec![symbol.version, symbol.ecc as u8];
            bytes.extend_from_slice(&(symbol.width as u16).to_le_bytes());
            bytes.extend_from_slice(&symbol.modules);
            request.reply(bytes)
        }
        Err(error) => request.error(2, error),
    }
}

/// Handoffs carry UTF-8 strings as count:u32, then length:u32 + bytes per string.
/// There is no shell command parser and no payload logging.
pub fn strings(payload: &[u8]) -> io::Result<Vec<String>> {
    let mut remaining = payload;
    fn number(bytes: &mut &[u8]) -> io::Result<usize> {
        if bytes.len() < 4 {
            return Err(invalid("truncated handoff"));
        }
        let value = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
        *bytes = &bytes[4..];
        Ok(value)
    }
    let count = number(&mut remaining)?;
    if count > 256 {
        return Err(invalid("too many handoff arguments"));
    }
    let mut result = Vec::with_capacity(count);
    for _ in 0..count {
        let length = number(&mut remaining)?;
        if length > remaining.len() {
            return Err(invalid("truncated handoff argument"));
        }
        let text = std::str::from_utf8(&remaining[..length])
            .map_err(|_| invalid("invalid handoff UTF-8"))?;
        if text.contains('\0') {
            return Err(invalid("invalid handoff argument"));
        }
        result.push(text.to_owned());
        remaining = &remaining[length..];
    }
    if !remaining.is_empty() {
        return Err(invalid("trailing handoff bytes"));
    }
    Ok(result)
}
pub fn encode_strings(strings: &[String]) -> Vec<u8> {
    let mut result = (strings.len() as u32).to_le_bytes().to_vec();
    for text in strings {
        result.extend_from_slice(&(text.len() as u32).to_le_bytes());
        result.extend_from_slice(text.as_bytes());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fragmented {
        data: io::Cursor<Vec<u8>>,
    }
    impl Read for Fragmented {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            let n = out.len().min(1);
            self.data.read(&mut out[..n])
        }
    }
    #[test]
    fn fragmented_frames_and_eof() {
        let frame = Frame {
            kind: REQUEST,
            id: 123,
            operation: QR,
            payload: vec![1, b'A'],
        };
        let mut data = Vec::new();
        frame.write(&mut data).unwrap();
        frame.write(&mut data).unwrap();
        let mut reader = Fragmented {
            data: io::Cursor::new(data),
        };
        assert_eq!(Frame::read(&mut reader).unwrap(), Some(frame.clone()));
        assert_eq!(Frame::read(&mut reader).unwrap(), Some(frame));
        assert_eq!(Frame::read(&mut reader).unwrap(), None);
    }
    #[test]
    fn malformed_frames_and_payloads() {
        for length in [0, 15, 1_048_577, u32::MAX] {
            assert!(Frame::read(&mut &length.to_le_bytes()[..]).is_err());
        }
        let frame = Frame {
            kind: REQUEST,
            id: 1,
            operation: QR,
            payload: vec![1, 255],
        };
        assert_eq!(encode_qr(&frame).kind, ERROR);
        let mut data = Vec::new();
        frame.write(&mut data).unwrap();
        data[4] = 2;
        assert!(Frame::read(&mut &data[..]).is_err());
        assert!(strings(&[255; 4]).is_err());
    }
}
