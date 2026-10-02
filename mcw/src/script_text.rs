//! The retained client's NBitcoin 10.0.13 Script text format, not execution.
//!
//! This compatibility boundary deliberately accepts the legacy OP_UNKNOWN
//! prefix/suffix grammar and renders a truncated push as a final zero. Strict
//! instruction parsing, templates and lossless wire formats remain unchanged.

#![forbid(unsafe_code)]

use crate::bitcoin_script::{self, AsmDialect, Opcode, Script, ScriptBuilder};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Script(bitcoin_script::Error),
    InvalidUnknownOpcode { token: usize },
    InvalidUtf8,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Script(error) => error.fmt(f),
            Self::InvalidUnknownOpcode { .. } => f.write_str("Invalid unknown opcode"),
            Self::InvalidUtf8 => f.write_str("Script text is not valid UTF-8"),
        }
    }
}

impl std::error::Error for Error {}

impl From<bitcoin_script::Error> for Error {
    fn from(error: bitcoin_script::Error) -> Self {
        Self::Script(error)
    }
}

// .NET String.Trim uses Char.IsWhiteSpace, whereas Op.ReadWord uses the six
// ASCII spaces. Spell the stable .NET whitespace set out instead of depending
// on the host locale or a future Rust Unicode database for persisted formats.
fn boundary_space(c: char) -> bool {
    matches!(
        c,
        '\u{0009}'..='\u{000d}'
            | '\u{0020}'
            | '\u{0085}'
            | '\u{00a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
    )
}

fn word_space(c: char) -> bool {
    matches!(c, '\u{0009}'..='\u{000d}' | ' ')
}

/// Retained new Script(string) behavior within the format codec's explicit
/// resource bounds. An OP_UNKNOWN token consumes its first two hex characters;
/// a closing parenthesis and any remaining suffix are intentionally ignored.
pub fn parse(text: &str) -> Result<Script, Error> {
    if text.len() > bitcoin_script::MAX_ASM_BYTES {
        return Err(bitcoin_script::Error::AsmTooLarge { length: text.len() }.into());
    }
    let mut builder = ScriptBuilder::new();
    for (token, word) in text
        .trim_matches(boundary_space)
        .split(word_space)
        .filter(|word| !word.is_empty())
        .enumerate()
    {
        if let Some(suffix) = word.strip_prefix("OP_UNKNOWN(0x") {
            let digits = suffix
                .get(..2)
                .filter(|digits| digits.bytes().all(|b| b.is_ascii_hexdigit()))
                .ok_or(Error::InvalidUnknownOpcode { token })?;
            let opcode = u8::from_str_radix(digits, 16)
                .map_err(|_| Error::InvalidUnknownOpcode { token })?;
            builder.append_opcode(Opcode(opcode))?;
        } else {
            // Reuse the published wallet grammar and canonical push builder.
            // Only the compatibility differences above live in this module.
            let part = Script::from_asm(word, AsmDialect::Wallet)?;
            builder.append_raw(part.as_bytes())?;
        }
    }
    Ok(builder.finish())
}

/// Native service input: UTF-8 text is the entire payload, with no secondary
/// schema or wire normalization. The returned bytes are the actual parsed Script.
pub fn parse_utf8(text: &[u8]) -> Result<Vec<u8>, Error> {
    let text = std::str::from_utf8(text).map_err(|_| Error::InvalidUtf8)?;
    Ok(parse(text)?.into_bytes())
}

/// Retained Script.ToString behavior. A truncated push consumes the remaining
/// stream and contributes "0", including a partially read length header. This
/// deliberately lossy display is separate from strict/lossless codec APIs.
pub fn render(bytes: &[u8]) -> Result<String, Error> {
    let script = Script::from_bytes(bytes)?;
    match script.to_wallet_asm() {
        Ok(text) => Ok(text),
        Err(
            bitcoin_script::Error::TruncatedLength { offset, .. }
            | bitcoin_script::Error::TruncatedPush { offset, .. },
        ) => {
            let prefix = Script::from_bytes(&bytes[..offset])?;
            let mut text = prefix.to_wallet_asm()?;
            if !text.is_empty() {
                text.push(' ');
            }
            text.push('0');
            Ok(text)
        }
        Err(error) => Err(error.into()),
    }
}
