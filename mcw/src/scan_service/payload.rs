//! ISO Model 2 data segment parsing. Character maps and QR input are data.
use super::{Error, Result, text};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StructuredAppend {
    pub index: u8,
    pub total: u8,
    pub parity: u8,
}
pub(super) struct Payload {
    pub text: String,
    pub structured: Option<StructuredAppend>,
    pub parity: u8,
}
struct Bits<'a> {
    bytes: &'a [u8],
    offset: usize,
}
fn invalid() -> Error {
    Error::Invalid("QR data segments are malformed")
}
impl Bits<'_> {
    fn read(&mut self, count: usize) -> Result<u32> {
        if count > 24
            || self
                .offset
                .checked_add(count)
                .is_none_or(|n| n > self.bytes.len() * 8)
        {
            return Err(invalid());
        }
        let mut value = 0;
        for _ in 0..count {
            value = (value << 1)
                | u32::from((self.bytes[self.offset / 8] >> (7 - self.offset % 8)) & 1);
            self.offset += 1;
        }
        Ok(value)
    }
}

pub(super) fn decode(data: &[u8], version: usize) -> Result<Payload> {
    if !(1..=40).contains(&version) || data.len() > 4096 {
        return Err(invalid());
    }
    let tier = if version < 10 {
        0
    } else if version < 27 {
        1
    } else {
        2
    };
    let mut bits = Bits {
        bytes: data,
        offset: 0,
    };
    let mut charset = None;
    let mut fnc1 = false;
    let mut structured = None;
    let mut text = String::new();
    let mut parity = 0;
    const ALPHA: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:";
    while data.len() * 8 - bits.offset >= 4 {
        let offset = bits.offset;
        let mode = bits.read(4)?;
        if mode == 0 {
            break;
        }
        if mode == 3 {
            if offset != 0 || structured.is_some() {
                return Err(invalid());
            }
            let index = bits.read(4)? as u8;
            let total = bits.read(4)? as u8 + 1;
            let value = bits.read(8)? as u8;
            if total < 2 || index >= total {
                return Err(invalid());
            }
            structured = Some(StructuredAppend {
                index,
                total,
                parity: value,
            });
            continue;
        }
        if mode == 5 || mode == 9 {
            if fnc1 || !text.is_empty() {
                return Err(invalid());
            }
            fnc1 = true;
            if mode == 9 {
                let _application_indicator = bits.read(8)?;
            }
            continue;
        }
        if mode == 7 {
            let first = bits.read(8)?;
            let value = if first & 0x80 == 0 {
                first
            } else if first & 0xc0 == 0x80 {
                ((first & 0x3f) << 8) | bits.read(8)?
            } else if first & 0xe0 == 0xc0 {
                ((first & 0x1f) << 16) | bits.read(16)?
            } else {
                return Err(invalid());
            };
            if value > 999999 {
                return Err(invalid());
            }
            // Validate even an empty/unused ECI; silent substitution is unsafe.
            text::decode(&[], Some(value))?;
            charset = Some(value);
            continue;
        }
        if mode == 13 && bits.read(4)? != 1 {
            return Err(invalid());
        }
        let width = match mode {
            1 => [10, 12, 14][tier],
            2 => [9, 11, 13][tier],
            4 => {
                if tier == 0 {
                    8
                } else {
                    16
                }
            }
            8 | 13 => [8, 10, 12][tier],
            _ => return Err(invalid()),
        };
        let mut count = bits.read(width)? as usize;
        if count > 8192 {
            return Err(Error::Capacity);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve(count.saturating_mul(2))
            .map_err(|_| Error::Capacity)?;
        let mut encoding = charset;
        match mode {
            1 => {
                while count != 0 {
                    let width = count.min(3);
                    let value = bits.read(match width {
                        3 => 10,
                        2 => 7,
                        _ => 4,
                    })?;
                    if value >= 10u32.pow(width as u32) {
                        return Err(invalid());
                    }
                    for exp in (0..width).rev() {
                        bytes.push(b'0' + ((value / 10u32.pow(exp as u32)) % 10) as u8);
                    }
                    count -= width;
                }
                encoding = Some(27);
            }
            2 => {
                while count >= 2 {
                    let pair = bits.read(11)? as usize;
                    if pair >= 45 * 45 {
                        return Err(invalid());
                    }
                    bytes.extend_from_slice(&[ALPHA[pair / 45], ALPHA[pair % 45]]);
                    count -= 2;
                }
                if count == 1 {
                    bytes.push(*ALPHA.get(bits.read(6)? as usize).ok_or_else(invalid)?);
                }
                encoding = Some(27);
            }
            4 => {
                for _ in 0..count {
                    bytes.push(bits.read(8)? as u8);
                }
            }
            8 | 13 => {
                for _ in 0..count {
                    let value = bits.read(13)?;
                    let value = if mode == 8 {
                        let assembled = ((value / 0xc0) << 8) | (value % 0xc0);
                        assembled + if assembled < 0x1f00 { 0x8140 } else { 0xc140 }
                    } else {
                        let assembled = ((value / 0x60) << 8) | (value % 0x60);
                        assembled + if assembled < 0x03bf { 0xa1a1 } else { 0xa6a1 }
                    };
                    bytes.extend_from_slice(&[(value >> 8) as u8, value as u8]);
                }
                encoding = Some(if mode == 8 { 20 } else { 29 });
            }
            _ => unreachable!(),
        }
        parity ^= bytes.iter().fold(0, |a, b| a ^ b);
        let segment = text::decode(&bytes, encoding)?;
        if fnc1 && mode == 2 {
            let mut iter = segment.chars().peekable();
            while let Some(c) = iter.next() {
                if c == '%' {
                    if iter.peek() == Some(&'%') {
                        iter.next();
                        text.push('%');
                    } else {
                        text.push('\u{1d}');
                    }
                } else {
                    text.push(c);
                }
            }
        } else {
            text.push_str(&segment);
        }
        if text.len() > 32768 {
            return Err(Error::Capacity);
        }
    }
    Ok(Payload {
        text,
        structured,
        parity,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bits(values: &[(u32, usize)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut n = 0;
        for &(v, width) in values {
            for bit in (0..width).rev() {
                if n % 8 == 0 {
                    out.push(0);
                }
                out[n / 8] |= (((v >> bit) & 1) as u8) << (7 - n % 8);
                n += 1;
            }
        }
        out
    }
    #[test]
    fn fnc1_kanji_hanzi_and_structured_metadata() {
        let b = bits(&[(5, 4), (2, 4), (3, 9), (10 * 45 + 38, 11), (38, 6), (0, 4)]);
        assert_eq!(decode(&b, 1).unwrap().text, "A%");
        let b = bits(&[
            (8, 4),
            (1, 8),
            (
                ((0x8a - 0x81) * 256 + 0xbf - 0x40) / 256 * 0xc0 + 0xbf - 0x40,
                13,
            ),
            (0, 4),
        ]);
        assert_eq!(decode(&b, 1).unwrap().text, "漢");
        let b = bits(&[
            (3, 4),
            (1, 4),
            (1, 4),
            (65, 8),
            (4, 4),
            (1, 8),
            (65, 8),
            (0, 4),
        ]);
        let p = decode(&b, 1).unwrap();
        assert_eq!(
            p.structured,
            Some(StructuredAppend {
                index: 1,
                total: 2,
                parity: 65
            })
        );
        assert_eq!(p.text, "A");
        assert_eq!(p.parity, 65);
    }
}
