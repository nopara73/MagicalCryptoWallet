//! QR ECI and legacy text decoding, implemented in Rust over Unicode map data.
//! No locale, process, network or external codec is consulted at runtime.
use super::{Error, Result};
#[path = "charset_data.rs"]
mod maps;

fn invalid() -> Error {
    Error::Invalid("QR text contains an invalid character sequence")
}

fn scalar(out: &mut String, value: u32) -> Result<()> {
    out.push(char::from_u32(value).ok_or_else(invalid)?);
    Ok(())
}

/// None follows the retained scanner's UTF-8 / Shift-JIS / Latin-1 detection.
/// Explicit ECI is strict; invalid byte sequences never silently change codec.
pub fn decode(bytes: &[u8], eci: Option<u32>) -> Result<String> {
    if bytes.len() > 8192 {
        return Err(Error::Capacity);
    }
    let charset = eci.unwrap_or_else(|| guess(bytes));
    let table = match charset {
        0 | 2 => Some(&maps::CP437),
        1 | 3 => Some(&maps::ISO8859_1),
        4 => Some(&maps::ISO8859_2),
        5 => Some(&maps::ISO8859_3),
        6 => Some(&maps::ISO8859_4),
        7 => Some(&maps::ISO8859_5),
        8 => Some(&maps::ISO8859_6),
        9 => Some(&maps::ISO8859_7),
        10 => Some(&maps::ISO8859_8),
        11 => Some(&maps::ISO8859_9),
        12 => Some(&maps::ISO8859_10),
        13 => Some(&maps::ISO8859_11),
        15 => Some(&maps::ISO8859_13),
        16 => Some(&maps::ISO8859_14),
        17 => Some(&maps::ISO8859_15),
        18 => Some(&maps::ISO8859_16),
        21 => Some(&maps::CP1250),
        22 => Some(&maps::CP1251),
        23 => Some(&maps::CP1252),
        24 => Some(&maps::CP1256),
        _ => None,
    };
    let mut output = String::new();
    output
        .try_reserve(bytes.len().saturating_mul(3))
        .map_err(|_| Error::Capacity)?;
    if let Some(table) = table {
        for &b in bytes {
            scalar(&mut output, table[b as usize])?;
        }
        return Ok(output);
    }
    if charset == 26 {
        return std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| invalid());
    }
    if matches!(charset, 27 | 170) {
        if bytes.iter().any(|&b| b >= 128) {
            return Err(invalid());
        }
        return Ok(bytes.iter().map(|&b| char::from(b)).collect());
    }
    if charset == 25 {
        if !bytes.len().is_multiple_of(2) {
            return Err(invalid());
        }
        for value in char::decode_utf16(
            bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|p| u16::from_be_bytes([p[0], p[1]])),
        ) {
            output.push(value.map_err(|_| invalid())?);
        }
        return Ok(output);
    }
    if !matches!(charset, 20 | 28 | 29 | 30) {
        return Err(Error::UnsupportedEncoding(charset));
    }
    let mut i = 0;
    while i < bytes.len() {
        let a = bytes[i];
        i += 1;
        if a < 128 {
            output.push(char::from(a));
            continue;
        }
        if charset == 20 && (0xa1..=0xdf).contains(&a) {
            scalar(&mut output, 0xff61 + u32::from(a - 0xa1))?;
            continue;
        }
        // GB18030 four-byte pointer, with monotone runs from the format map.
        let b = *bytes.get(i).ok_or_else(invalid)?;
        i += 1;
        if charset == 29 && (0x81..=0xfe).contains(&a) && (0x30..=0x39).contains(&b) {
            let c = *bytes.get(i).ok_or_else(invalid)?;
            let d = *bytes.get(i + 1).ok_or_else(invalid)?;
            i += 2;
            if !(0x81..=0xfe).contains(&c) || !(0x30..=0x39).contains(&d) {
                return Err(invalid());
            }
            let pointer = (((u32::from(a - 0x81) * 10 + u32::from(b - 0x30)) * 126
                + u32::from(c - 0x81))
                * 10)
                + u32::from(d - 0x30);
            let index = maps::GB_RANGES.partition_point(|&(start, _, _)| start <= pointer);
            if index == 0 {
                return Err(invalid());
            }
            let (start, end, value) = maps::GB_RANGES[index - 1];
            if pointer > end {
                return Err(invalid());
            }
            scalar(&mut output, value + pointer - start)?;
            continue;
        }
        let value = match charset {
            20 => {
                let lead = if (0x81..=0x9f).contains(&a) {
                    a - 0x81
                } else if (0xe0..=0xfc).contains(&a) {
                    a - 0xe0 + 31
                } else {
                    return Err(invalid());
                };
                let trail = if (0x40..=0x7e).contains(&b) {
                    b - 0x40
                } else if (0x80..=0xfc).contains(&b) {
                    b - 0x80 + 63
                } else {
                    return Err(invalid());
                };
                maps::SJIS[usize::from(lead) * 188 + usize::from(trail)]
            }
            28 => {
                if !(0x81..=0xfe).contains(&a) {
                    return Err(invalid());
                }
                let trail = if (0x40..=0x7e).contains(&b) {
                    b - 0x40
                } else if (0xa1..=0xfe).contains(&b) {
                    b - 0xa1 + 63
                } else {
                    return Err(invalid());
                };
                maps::BIG5[usize::from(a - 0x81) * 157 + usize::from(trail)]
            }
            29 => {
                if !(0x81..=0xfe).contains(&a) {
                    return Err(invalid());
                }
                let trail = if (0x40..=0x7e).contains(&b) {
                    b - 0x40
                } else if (0x80..=0xfe).contains(&b) {
                    b - 0x80 + 63
                } else {
                    return Err(invalid());
                };
                maps::GB18030[usize::from(a - 0x81) * 190 + usize::from(trail)]
            }
            30 => {
                if !(0xa1..=0xfe).contains(&a) || !(0xa1..=0xfe).contains(&b) {
                    return Err(invalid());
                }
                maps::EUC_KR[usize::from(a - 0xa1) * 94 + usize::from(b - 0xa1)]
            }
            _ => unreachable!(),
        };
        scalar(&mut output, value)?;
    }
    Ok(output)
}

fn guess(bytes: &[u8]) -> u32 {
    if bytes.iter().any(|&b| b >= 128) && std::str::from_utf8(bytes).is_ok() {
        return 26;
    }
    let mut i = 0;
    let mut pairs = 0;
    let mut kana = 0;
    let mut sjis = true;
    while i < bytes.len() {
        let b = bytes[i];
        i += 1;
        if b < 128 {
            continue;
        }
        if (0xa1..=0xdf).contains(&b) {
            kana += 1;
            continue;
        }
        if ((0x81..=0x9f).contains(&b) || (0xe0..=0xef).contains(&b))
            && let Some(&trail) = bytes.get(i)
            && (0x40..=0xfc).contains(&trail)
            && trail != 0x7f
        {
            i += 1;
            pairs += 1;
            continue;
        }
        sjis = false;
        break;
    }
    if sjis && (pairs >= 3 || kana >= 3 || bytes.iter().any(|b| (0x80..=0x9f).contains(b))) {
        20
    } else {
        3
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_payload_and_encodings() {
        assert_eq!(
            decode(b"bitcoin:x?label=A%20B\0", None).unwrap(),
            "bitcoin:x?label=A%20B\0"
        );
        assert_eq!(decode("€🦀".as_bytes(), None).unwrap(), "€🦀");
        assert_eq!(decode(&[0xe9], Some(3)).unwrap(), "é");
        assert_eq!(decode(&[0x8a, 0xbf, 0x8e, 0x9a], Some(20)).unwrap(), "漢字");
        assert_eq!(decode(&[0xd6, 0xd0, 0xce, 0xc4], Some(29)).unwrap(), "中文");
        assert_eq!(decode(&[0xd8, 0x3e, 0xdd, 0x80], Some(25)).unwrap(), "🦀");
        assert!(decode(&[0xff], Some(26)).is_err());
        assert!(decode(&[0xd8, 0x00], Some(25)).is_err());
        assert!(decode(&[0x81, 0x30, 0x81], Some(29)).is_err());
        assert_eq!(
            decode(b"x", Some(999)),
            Err(Error::UnsupportedEncoding(999))
        );
    }
}
