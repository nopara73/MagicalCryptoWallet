//! Bounded Bitcoin Script formats and output templates. This is not an interpreter.
//!
//! Raw construction preserves even malformed scripts. Instruction iteration and
//! validation are explicit and never normalize bytes. Public keys and hashes are
//! supplied payloads: key shape checks do not verify points, signatures or ownership.
//! See bitcoin_script_fixtures/SOURCES.md for format and vector provenance.

#![forbid(unsafe_code)]

use crate::bitcoin_encoding::{self, Address, LegacyKind, Network};
use std::fmt;

/// Application format bound, independent of consensus/relay script size limits.
pub const MAX_SCRIPT_BYTES: usize = 1_048_576;
pub const MAX_ASM_BYTES: usize = MAX_SCRIPT_BYTES * 32;
pub const MAX_SCRIPT_NUMBER_BYTES: usize = 9;
pub const MAX_MULTISIG_KEYS: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    ScriptTooLarge {
        length: usize,
    },
    AsmTooLarge {
        length: usize,
    },
    TruncatedLength {
        offset: usize,
        needed: usize,
        available: usize,
    },
    TruncatedPush {
        offset: usize,
        declared: u32,
        available: usize,
    },
    NonMinimalPush {
        offset: usize,
    },
    PushTooLarge {
        offset: usize,
        length: usize,
        maximum: usize,
    },
    InvalidPushEncoding {
        encoding: PushEncoding,
        length: usize,
    },
    NumberTooLarge {
        length: usize,
        maximum: usize,
    },
    InvalidNumberLimit {
        maximum: usize,
    },
    NonMinimalNumber,
    NumberOverflow,
    InvalidAsm {
        token: usize,
    },
    NonAsciiAsm {
        offset: usize,
    },
    InvalidPublicKeyShape {
        length: usize,
    },
    InvalidMultisig {
        required: u8,
        keys: usize,
    },
    InvalidWitnessVersion {
        version: u8,
    },
    InvalidWitnessLength {
        version: u8,
        length: usize,
    },
    Encoding(bitcoin_encoding::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ScriptTooLarge { length } => {
                write!(f, "script length {length} exceeds {MAX_SCRIPT_BYTES}")
            }
            Self::AsmTooLarge { length } => {
                write!(f, "asm length {length} exceeds {MAX_ASM_BYTES}")
            }
            Self::TruncatedLength {
                offset,
                needed,
                available,
            } => write!(
                f,
                "push length at {offset} needs {needed} bytes; {available} remain"
            ),
            Self::TruncatedPush {
                offset,
                declared,
                available,
            } => write!(
                f,
                "push at {offset} declares {declared} bytes; {available} remain"
            ),
            Self::NonMinimalPush { offset } => write!(f, "nonminimal push at {offset}"),
            Self::PushTooLarge {
                offset,
                length,
                maximum,
            } => write!(f, "push at {offset} has {length} bytes, limit {maximum}"),
            Self::InvalidPushEncoding { encoding, length } => {
                write!(f, "{encoding:?} cannot encode {length} bytes")
            }
            Self::NumberTooLarge { length, maximum } => {
                write!(f, "script number has {length} bytes, limit {maximum}")
            }
            Self::InvalidNumberLimit { maximum } => write!(
                f,
                "number limit {maximum} exceeds {MAX_SCRIPT_NUMBER_BYTES}"
            ),
            Self::NonMinimalNumber => f.write_str("nonminimal script number"),
            Self::NumberOverflow => f.write_str("script number exceeds signed 64-bit range"),
            Self::InvalidAsm { token } => write!(f, "invalid asm token {token}"),
            Self::NonAsciiAsm { offset } => write!(f, "non-ASCII asm byte at {offset}"),
            Self::InvalidPublicKeyShape { length } => {
                write!(f, "invalid public key encoding shape of {length} bytes")
            }
            Self::InvalidMultisig { required, keys } => {
                write!(f, "invalid multisig threshold {required} for {keys} keys")
            }
            Self::InvalidWitnessVersion { version } => {
                write!(f, "witness version {version} exceeds 16")
            }
            Self::InvalidWitnessLength { version, length } => {
                write!(f, "invalid witness v{version} program length {length}")
            }
            Self::Encoding(e) => write!(f, "Bitcoin encoding: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Encoding(e) => Some(e),
            _ => None,
        }
    }
}

impl From<bitcoin_encoding::Error> for Error {
    fn from(value: bitcoin_encoding::Error) -> Self {
        Self::Encoding(value)
    }
}

/// Every byte is representable, including future, disabled and invalid opcodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Opcode(pub u8);

macro_rules! opcode_table {
    ($($name:ident = $value:literal => $text:literal),* $(,)?) => {
        pub mod opcodes {
            use super::Opcode;
            $(pub const $name: Opcode = Opcode($value);)*
            pub const OP_FALSE: Opcode = OP_0;
            pub const OP_TRUE: Opcode = OP_1;
            pub const OP_NOP2: Opcode = OP_CHECKLOCKTIMEVERIFY;
            pub const OP_NOP3: Opcode = OP_CHECKSEQUENCEVERIFY;
        }
        const OPCODE_NAMES: &[(u8, &str)] = &[$(($value, $text)),*];
        fn opcode_name(byte: u8) -> Option<&'static str> {
            match byte { $($value => Some($text),)* _ => None }
        }
    };
}

// Assigned wire values and names, as specified by Bitcoin Core's script.h.
opcode_table! {
    OP_0 = 0x00 => "0", OP_PUSHDATA1 = 0x4c => "OP_PUSHDATA1",
    OP_PUSHDATA2 = 0x4d => "OP_PUSHDATA2", OP_PUSHDATA4 = 0x4e => "OP_PUSHDATA4",
    OP_1NEGATE = 0x4f => "-1", OP_RESERVED = 0x50 => "OP_RESERVED",
    OP_1 = 0x51 => "1", OP_2 = 0x52 => "2", OP_3 = 0x53 => "3",
    OP_4 = 0x54 => "4", OP_5 = 0x55 => "5", OP_6 = 0x56 => "6",
    OP_7 = 0x57 => "7", OP_8 = 0x58 => "8", OP_9 = 0x59 => "9",
    OP_10 = 0x5a => "10", OP_11 = 0x5b => "11", OP_12 = 0x5c => "12",
    OP_13 = 0x5d => "13", OP_14 = 0x5e => "14", OP_15 = 0x5f => "15", OP_16 = 0x60 => "16",
    OP_NOP = 0x61 => "OP_NOP", OP_VER = 0x62 => "OP_VER",
    OP_IF = 0x63 => "OP_IF", OP_NOTIF = 0x64 => "OP_NOTIF",
    OP_VERIF = 0x65 => "OP_VERIF", OP_VERNOTIF = 0x66 => "OP_VERNOTIF",
    OP_ELSE = 0x67 => "OP_ELSE", OP_ENDIF = 0x68 => "OP_ENDIF",
    OP_VERIFY = 0x69 => "OP_VERIFY", OP_RETURN = 0x6a => "OP_RETURN",
    OP_TOALTSTACK = 0x6b => "OP_TOALTSTACK", OP_FROMALTSTACK = 0x6c => "OP_FROMALTSTACK",
    OP_2DROP = 0x6d => "OP_2DROP", OP_2DUP = 0x6e => "OP_2DUP", OP_3DUP = 0x6f => "OP_3DUP",
    OP_2OVER = 0x70 => "OP_2OVER", OP_2ROT = 0x71 => "OP_2ROT", OP_2SWAP = 0x72 => "OP_2SWAP",
    OP_IFDUP = 0x73 => "OP_IFDUP", OP_DEPTH = 0x74 => "OP_DEPTH", OP_DROP = 0x75 => "OP_DROP",
    OP_DUP = 0x76 => "OP_DUP", OP_NIP = 0x77 => "OP_NIP", OP_OVER = 0x78 => "OP_OVER",
    OP_PICK = 0x79 => "OP_PICK", OP_ROLL = 0x7a => "OP_ROLL", OP_ROT = 0x7b => "OP_ROT",
    OP_SWAP = 0x7c => "OP_SWAP", OP_TUCK = 0x7d => "OP_TUCK",
    OP_CAT = 0x7e => "OP_CAT", OP_SUBSTR = 0x7f => "OP_SUBSTR",
    OP_LEFT = 0x80 => "OP_LEFT", OP_RIGHT = 0x81 => "OP_RIGHT", OP_SIZE = 0x82 => "OP_SIZE",
    OP_INVERT = 0x83 => "OP_INVERT", OP_AND = 0x84 => "OP_AND", OP_OR = 0x85 => "OP_OR",
    OP_XOR = 0x86 => "OP_XOR", OP_EQUAL = 0x87 => "OP_EQUAL",
    OP_EQUALVERIFY = 0x88 => "OP_EQUALVERIFY", OP_RESERVED1 = 0x89 => "OP_RESERVED1",
    OP_RESERVED2 = 0x8a => "OP_RESERVED2", OP_1ADD = 0x8b => "OP_1ADD",
    OP_1SUB = 0x8c => "OP_1SUB", OP_2MUL = 0x8d => "OP_2MUL", OP_2DIV = 0x8e => "OP_2DIV",
    OP_NEGATE = 0x8f => "OP_NEGATE", OP_ABS = 0x90 => "OP_ABS", OP_NOT = 0x91 => "OP_NOT",
    OP_0NOTEQUAL = 0x92 => "OP_0NOTEQUAL", OP_ADD = 0x93 => "OP_ADD", OP_SUB = 0x94 => "OP_SUB",
    OP_MUL = 0x95 => "OP_MUL", OP_DIV = 0x96 => "OP_DIV", OP_MOD = 0x97 => "OP_MOD",
    OP_LSHIFT = 0x98 => "OP_LSHIFT", OP_RSHIFT = 0x99 => "OP_RSHIFT",
    OP_BOOLAND = 0x9a => "OP_BOOLAND", OP_BOOLOR = 0x9b => "OP_BOOLOR",
    OP_NUMEQUAL = 0x9c => "OP_NUMEQUAL", OP_NUMEQUALVERIFY = 0x9d => "OP_NUMEQUALVERIFY",
    OP_NUMNOTEQUAL = 0x9e => "OP_NUMNOTEQUAL", OP_LESSTHAN = 0x9f => "OP_LESSTHAN",
    OP_GREATERTHAN = 0xa0 => "OP_GREATERTHAN", OP_LESSTHANOREQUAL = 0xa1 => "OP_LESSTHANOREQUAL",
    OP_GREATERTHANOREQUAL = 0xa2 => "OP_GREATERTHANOREQUAL", OP_MIN = 0xa3 => "OP_MIN",
    OP_MAX = 0xa4 => "OP_MAX", OP_WITHIN = 0xa5 => "OP_WITHIN",
    OP_RIPEMD160 = 0xa6 => "OP_RIPEMD160", OP_SHA1 = 0xa7 => "OP_SHA1",
    OP_SHA256 = 0xa8 => "OP_SHA256", OP_HASH160 = 0xa9 => "OP_HASH160",
    OP_HASH256 = 0xaa => "OP_HASH256", OP_CODESEPARATOR = 0xab => "OP_CODESEPARATOR",
    OP_CHECKSIG = 0xac => "OP_CHECKSIG", OP_CHECKSIGVERIFY = 0xad => "OP_CHECKSIGVERIFY",
    OP_CHECKMULTISIG = 0xae => "OP_CHECKMULTISIG", OP_CHECKMULTISIGVERIFY = 0xaf => "OP_CHECKMULTISIGVERIFY",
    OP_NOP1 = 0xb0 => "OP_NOP1", OP_CHECKLOCKTIMEVERIFY = 0xb1 => "OP_CHECKLOCKTIMEVERIFY",
    OP_CHECKSEQUENCEVERIFY = 0xb2 => "OP_CHECKSEQUENCEVERIFY", OP_NOP4 = 0xb3 => "OP_NOP4",
    OP_NOP5 = 0xb4 => "OP_NOP5", OP_NOP6 = 0xb5 => "OP_NOP6", OP_NOP7 = 0xb6 => "OP_NOP7",
    OP_NOP8 = 0xb7 => "OP_NOP8", OP_NOP9 = 0xb8 => "OP_NOP9", OP_NOP10 = 0xb9 => "OP_NOP10",
    OP_CHECKSIGADD = 0xba => "OP_CHECKSIGADD", OP_INVALIDOPCODE = 0xff => "OP_INVALIDOPCODE",
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpcodeClass {
    DataPush,
    SmallInteger,
    Control,
    Stack,
    Splice,
    BitLogic,
    Numeric,
    Crypto,
    Nop,
    Reserved,
    Disabled,
    Unknown,
}

impl Opcode {
    pub fn name(self) -> Option<&'static str> {
        opcode_name(self.0)
    }

    /// Canonical opcode names plus familiar aliases. Direct-length pushes have no name.
    pub fn from_name(name: &str) -> Option<Self> {
        let alias = match name {
            "OP_FALSE" | "OP_0" => Some(0),
            "OP_TRUE" => Some(0x51),
            "OP_1NEGATE" => Some(0x4f),
            "OP_NOP2" | "OP_CLTV" | "OP_HODL" => Some(0xb1),
            "OP_NOP3" | "OP_CSV" => Some(0xb2),
            _ => None,
        };
        if let Some(byte) = alias {
            return Some(Self(byte));
        }
        if let Some(n) = name.strip_prefix("OP_").and_then(|s| s.parse::<u8>().ok())
            && (1..=16).contains(&n)
        {
            return Some(Self(0x50 + n));
        }
        OPCODE_NAMES
            .iter()
            .find(|(_, text)| *text == name)
            .map(|(byte, _)| Self(*byte))
    }

    pub const fn small_integer(self) -> Option<i64> {
        match self.0 {
            0 => Some(0),
            0x4f => Some(-1),
            0x51..=0x60 => Some((self.0 - 0x50) as i64),
            _ => None,
        }
    }

    pub const fn is_disabled_legacy(self) -> bool {
        matches!(self.0, 0x65..=0x66 | 0x7e..=0x81 | 0x83..=0x86 | 0x8d..=0x8e | 0x95..=0x99)
    }

    /// BIP342 opcode classification only, never an execution/success verdict.
    pub const fn is_tapscript_success(self) -> bool {
        matches!(self.0, 80 | 98 | 126..=129 | 131..=134 | 137..=138 | 141..=142 | 149..=153 | 187..=254)
    }

    /// Core's syntactic IsPushOnly predicate includes OP_RESERVED.
    pub const fn is_push_only_opcode(self) -> bool {
        self.0 <= 0x60
    }

    pub const fn class(self) -> OpcodeClass {
        if self.is_disabled_legacy() {
            return OpcodeClass::Disabled;
        }
        match self.0 {
            0..=0x4e => OpcodeClass::DataPush,
            0x4f | 0x51..=0x60 => OpcodeClass::SmallInteger,
            0x50 | 0x62 | 0x89..=0x8a => OpcodeClass::Reserved,
            0x61 | 0xb0 | 0xb3..=0xb9 => OpcodeClass::Nop,
            0x63..=0x6a | 0xb1..=0xb2 => OpcodeClass::Control,
            0x6b..=0x7d => OpcodeClass::Stack,
            0x82 => OpcodeClass::Splice,
            0x87..=0x88 => OpcodeClass::BitLogic,
            0x8b..=0xa5 => OpcodeClass::Numeric,
            0xa6..=0xaf | 0xba => OpcodeClass::Crypto,
            _ => OpcodeClass::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushEncoding {
    Direct,
    PushData1,
    PushData2,
    PushData4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstructionKind<'a> {
    Push {
        data: &'a [u8],
        encoding: PushEncoding,
    },
    Op(Opcode),
}

/// A token retains its exact prefix and payload through raw_bytes().
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instruction<'a> {
    offset: usize,
    opcode: Opcode,
    kind: InstructionKind<'a>,
    raw: &'a [u8],
}

impl<'a> Instruction<'a> {
    pub const fn offset(self) -> usize {
        self.offset
    }
    pub const fn opcode(self) -> Opcode {
        self.opcode
    }
    pub const fn kind(self) -> InstructionKind<'a> {
        self.kind
    }
    pub const fn raw_bytes(self) -> &'a [u8] {
        self.raw
    }
    pub const fn pushed_bytes(self) -> Option<&'a [u8]> {
        match self.kind {
            InstructionKind::Push { data, .. } => Some(data),
            _ => None,
        }
    }
    pub fn is_minimal_push(self) -> Option<bool> {
        self.pushed_bytes()
            .map(|data| minimal_push_opcode(data) == self.opcode)
    }
    pub fn script_number(
        self,
        maximum: usize,
        require_minimal: bool,
    ) -> Result<Option<i64>, Error> {
        if maximum > MAX_SCRIPT_NUMBER_BYTES {
            return Err(Error::InvalidNumberLimit { maximum });
        }
        match self.kind {
            InstructionKind::Push { data, .. } => {
                decode_script_number(data, maximum, require_minimal).map(Some)
            }
            InstructionKind::Op(op) => Ok(op.small_integer()),
        }
    }
}

/// Zero-allocation borrowed iterator. A malformed push yields one error, then fuses.
#[derive(Debug, Clone)]
pub struct Instructions<'a> {
    bytes: &'a [u8],
    cursor: usize,
    failed: bool,
}

pub fn instructions(bytes: &[u8]) -> Result<Instructions<'_>, Error> {
    check_script_size(bytes.len())?;
    Ok(Instructions {
        bytes,
        cursor: 0,
        failed: false,
    })
}

impl<'a> Instructions<'a> {
    fn read(&mut self) -> Result<Instruction<'a>, Error> {
        let offset = self.cursor;
        let opcode = Opcode(self.bytes[self.cursor]);
        self.cursor += 1;
        let kind = if opcode.0 <= 0x4e {
            let (width, encoding) = match opcode.0 {
                0..=75 => (0, PushEncoding::Direct),
                76 => (1, PushEncoding::PushData1),
                77 => (2, PushEncoding::PushData2),
                _ => (4, PushEncoding::PushData4),
            };
            let available = self.bytes.len() - self.cursor;
            if available < width {
                return Err(Error::TruncatedLength {
                    offset,
                    needed: width,
                    available,
                });
            }
            let declared = if width == 0 {
                u32::from(opcode.0)
            } else {
                let mut length_bytes = [0; 4];
                length_bytes[..width]
                    .copy_from_slice(&self.bytes[self.cursor..self.cursor + width]);
                self.cursor += width;
                u32::from_le_bytes(length_bytes)
            };
            let available = self.bytes.len() - self.cursor;
            if u64::from(declared) > available as u64 {
                return Err(Error::TruncatedPush {
                    offset,
                    declared,
                    available,
                });
            }
            let end = self.cursor + declared as usize;
            let data = &self.bytes[self.cursor..end];
            self.cursor = end;
            InstructionKind::Push { data, encoding }
        } else {
            InstructionKind::Op(opcode)
        };
        Ok(Instruction {
            offset,
            opcode,
            kind,
            raw: &self.bytes[offset..self.cursor],
        })
    }
}

impl<'a> Iterator for Instructions<'a> {
    type Item = Result<Instruction<'a>, Error>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.cursor == self.bytes.len() {
            return None;
        }
        let result = self.read();
        if result.is_err() {
            self.failed = true;
        }
        Some(result)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (
            0,
            Some(if self.failed {
                0
            } else {
                self.bytes.len() - self.cursor
            }),
        )
    }
}
impl std::iter::FusedIterator for Instructions<'_> {}

fn check_script_size(length: usize) -> Result<(), Error> {
    if length > MAX_SCRIPT_BYTES {
        Err(Error::ScriptTooLarge { length })
    } else {
        Ok(())
    }
}

pub fn minimal_push_opcode(data: &[u8]) -> Opcode {
    match data {
        [] => opcodes::OP_0,
        [n @ 1..=16] => Opcode(0x50 + n),
        [0x81] => opcodes::OP_1NEGATE,
        _ if data.len() <= 75 => Opcode(data.len() as u8),
        _ if data.len() <= 255 => opcodes::OP_PUSHDATA1,
        _ if data.len() <= 65_535 => opcodes::OP_PUSHDATA2,
        _ => opcodes::OP_PUSHDATA4,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidationPolicy {
    pub require_minimal_pushes: bool,
    pub maximum_push_bytes: usize,
}
impl Default for ValidationPolicy {
    fn default() -> Self {
        Self {
            require_minimal_pushes: false,
            maximum_push_bytes: MAX_SCRIPT_BYTES,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptStats {
    pub instruction_count: usize,
    pub data_push_count: usize,
    pub nonminimal_push_count: usize,
    pub maximum_push_bytes: usize,
    pub is_push_only: bool,
}

pub fn validate(bytes: &[u8], policy: ValidationPolicy) -> Result<ScriptStats, Error> {
    let mut stats = ScriptStats {
        instruction_count: 0,
        data_push_count: 0,
        nonminimal_push_count: 0,
        maximum_push_bytes: 0,
        is_push_only: true,
    };
    for item in instructions(bytes)? {
        let item = item?;
        stats.instruction_count += 1;
        stats.is_push_only &= item.opcode.is_push_only_opcode();
        if let Some(data) = item.pushed_bytes() {
            stats.data_push_count += 1;
            stats.maximum_push_bytes = stats.maximum_push_bytes.max(data.len());
            if data.len() > policy.maximum_push_bytes {
                return Err(Error::PushTooLarge {
                    offset: item.offset,
                    length: data.len(),
                    maximum: policy.maximum_push_bytes,
                });
            }
            if item.is_minimal_push() == Some(false) {
                stats.nonminimal_push_count += 1;
                if policy.require_minimal_pushes {
                    return Err(Error::NonMinimalPush {
                        offset: item.offset,
                    });
                }
            }
        }
    }
    Ok(stats)
}

/// Immutable byte container; construction is size checked, not syntax checked.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Script {
    bytes: Vec<u8>,
}

impl Script {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        check_script_size(bytes.len())?;
        Ok(Self {
            bytes: bytes.to_vec(),
        })
    }
    pub fn from_vec(bytes: Vec<u8>) -> Result<Self, Error> {
        check_script_size(bytes.len())?;
        Ok(Self { bytes })
    }
    pub fn from_hex(text: &str) -> Result<Self, Error> {
        Self::from_vec(bitcoin_encoding::hex_decode(text)?)
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
    pub fn to_hex(&self) -> Result<String, Error> {
        Ok(bitcoin_encoding::hex_encode(&self.bytes)?)
    }
    pub fn instructions(&self) -> Instructions<'_> {
        Instructions {
            bytes: &self.bytes,
            cursor: 0,
            failed: false,
        }
    }
    pub fn validate(&self, policy: ValidationPolicy) -> Result<ScriptStats, Error> {
        validate(&self.bytes, policy)
    }
    pub fn is_push_only(&self) -> Result<bool, Error> {
        Ok(self.validate(ValidationPolicy::default())?.is_push_only)
    }
    pub fn witness_program(&self) -> Option<WitnessProgram<'_>> {
        witness_program(&self.bytes)
    }
    pub fn output_template(&self) -> Result<OutputTemplate<'_>, Error> {
        classify_output(&self.bytes)
    }
}

/// Bounded serializer. append_raw/append_opcode deliberately allow malformed bytes.
/// Explicit push methods check their entire addition before mutating the builder.
#[derive(Debug, Clone, Default)]
pub struct ScriptBuilder {
    bytes: Vec<u8>,
}

impl ScriptBuilder {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn finish(self) -> Script {
        Script { bytes: self.bytes }
    }
    fn check_addition(&self, size: usize) -> Result<(), Error> {
        check_script_size(self.bytes.len().saturating_add(size))
    }
    pub fn append_raw(&mut self, raw: &[u8]) -> Result<&mut Self, Error> {
        self.check_addition(raw.len())?;
        self.bytes.extend_from_slice(raw);
        Ok(self)
    }
    pub fn append_opcode(&mut self, opcode: Opcode) -> Result<&mut Self, Error> {
        self.append_raw(&[opcode.0])
    }
    pub fn append_instruction(&mut self, item: Instruction<'_>) -> Result<&mut Self, Error> {
        self.append_raw(item.raw)
    }
    /// Minimal stack-data push, including small-number opcodes when applicable.
    pub fn push_data(&mut self, data: &[u8]) -> Result<&mut Self, Error> {
        let opcode = minimal_push_opcode(data);
        if opcode.small_integer().is_some() {
            self.append_opcode(opcode)
        } else {
            self.push_data_length_only(data)
        }
    }
    /// Core's CScript << byte-vector behavior: shortest length prefix, no OP_n substitution.
    pub fn push_data_length_only(&mut self, data: &[u8]) -> Result<&mut Self, Error> {
        let encoding = match data.len() {
            0..=75 => PushEncoding::Direct,
            76..=255 => PushEncoding::PushData1,
            256..=65_535 => PushEncoding::PushData2,
            _ => PushEncoding::PushData4,
        };
        self.push_data_exact(data, encoding)
    }
    /// Preserve a caller-selected, possibly nonminimal, data-length encoding.
    pub fn push_data_exact(
        &mut self,
        data: &[u8],
        encoding: PushEncoding,
    ) -> Result<&mut Self, Error> {
        let (opcode, width, max_length) = match encoding {
            PushEncoding::Direct => (data.len() as u8, 0, 75),
            PushEncoding::PushData1 => (76, 1, 255),
            PushEncoding::PushData2 => (77, 2, 65_535),
            PushEncoding::PushData4 => (78, 4, u32::MAX as usize),
        };
        if data.len() > max_length {
            return Err(Error::InvalidPushEncoding {
                encoding,
                length: data.len(),
            });
        }
        self.check_addition(data.len().saturating_add(1 + width))?;
        self.bytes.push(opcode);
        self.bytes
            .extend_from_slice(&(data.len() as u32).to_le_bytes()[..width]);
        self.bytes.extend_from_slice(data);
        Ok(self)
    }
    pub fn push_number(&mut self, value: i64) -> Result<&mut Self, Error> {
        self.push_data(&encode_script_number(value))
    }
}

/// Signed-magnitude, little-endian Script numbers, including a defined i64::MIN encoding.
pub fn encode_script_number(value: i64) -> Vec<u8> {
    if value == 0 {
        return Vec::new();
    }
    let negative = value < 0;
    let mut magnitude = value.unsigned_abs();
    let mut out = Vec::with_capacity(MAX_SCRIPT_NUMBER_BYTES);
    while magnitude != 0 {
        out.push(magnitude as u8);
        magnitude >>= 8;
    }
    let last = out.len() - 1;
    if out[last] & 0x80 != 0 {
        out.push(if negative { 0x80 } else { 0 });
    } else if negative {
        out[last] |= 0x80;
    }
    out
}

pub fn is_minimal_script_number(data: &[u8]) -> bool {
    data.last()
        .is_none_or(|last| last & 0x7f != 0 || (data.len() > 1 && data[data.len() - 2] & 0x80 != 0))
}

/// maximum is explicit: common arithmetic uses 4 bytes, CLTV/CSV use 5.
/// Size and minimality checks precede conversion. No truncation or saturation.
pub fn decode_script_number(
    data: &[u8],
    maximum: usize,
    require_minimal: bool,
) -> Result<i64, Error> {
    if maximum > MAX_SCRIPT_NUMBER_BYTES {
        return Err(Error::InvalidNumberLimit { maximum });
    }
    if data.len() > maximum {
        return Err(Error::NumberTooLarge {
            length: data.len(),
            maximum,
        });
    }
    if require_minimal && !is_minimal_script_number(data) {
        return Err(Error::NonMinimalNumber);
    }
    if data.is_empty() {
        return Ok(0);
    }
    let negative = data[data.len() - 1] & 0x80 != 0;
    let mut magnitude = 0_u128;
    for (i, byte) in data.iter().copied().enumerate() {
        let byte = if i == data.len() - 1 {
            byte & 0x7f
        } else {
            byte
        };
        magnitude |= u128::from(byte) << (8 * i);
    }
    if negative && magnitude == 1_u128 << 63 {
        return Ok(i64::MIN);
    }
    let magnitude = i64::try_from(magnitude).map_err(|_| Error::NumberOverflow)?;
    Ok(if negative { -magnitude } else { magnitude })
}

/// Script boolean conversion includes negative zero; no execution is performed.
pub fn cast_to_bool(data: &[u8]) -> bool {
    data.iter()
        .enumerate()
        .any(|(i, byte)| *byte != 0 && !(i == data.len() - 1 && *byte == 0x80))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsmDialect {
    /// Retained NBitcoin Script.ToString()/new Script(string) grammar.
    /// Data are hex, not decimal. Parsing chooses minimal pushes and may normalize.
    Wallet,
    /// Core ParseScript grammar: decimal numbers, 0x raw bytes, 'quoted' data,
    /// opcode names with optional OP_ prefix. Preserves raw tokens even if malformed.
    CoreFormat,
}

fn check_asm(text: &str) -> Result<(), Error> {
    if text.len() > MAX_ASM_BYTES {
        return Err(Error::AsmTooLarge { length: text.len() });
    }
    if let Some(offset) = text.bytes().position(|b| !b.is_ascii()) {
        return Err(Error::NonAsciiAsm { offset });
    }
    Ok(())
}

impl Script {
    pub fn from_asm(text: &str, dialect: AsmDialect) -> Result<Self, Error> {
        check_asm(text)?;
        let mut builder = ScriptBuilder::new();
        for (token, word) in text
            .split(|c: char| match dialect {
                AsmDialect::Wallet => c.is_ascii_whitespace(),
                AsmDialect::CoreFormat => matches!(c, ' ' | '\t' | '\n'),
            })
            .filter(|word| !word.is_empty())
            .enumerate()
        {
            match dialect {
                AsmDialect::Wallet => parse_wallet_word(&mut builder, word, token)?,
                AsmDialect::CoreFormat => parse_core_word(&mut builder, word, token)?,
            }
        }
        Ok(builder.finish())
    }

    /// Wallet-compatible display for syntactically complete instructions.
    /// Nonminimal pushes are display-normalized; use hex/CoreFormat for byte identity.
    pub fn to_wallet_asm(&self) -> Result<String, Error> {
        let mut out = String::new();
        for item in self.instructions() {
            let item = item?;
            let word = match item.kind {
                InstructionKind::Push { data: [], .. } => "0".to_owned(),
                InstructionKind::Push { data, .. } => wallet_hex(data)?,
                InstructionKind::Op(op) => {
                    if let Some(value) = op.small_integer() {
                        wallet_hex(&encode_script_number(value))?
                    } else {
                        match op.0 {
                            0xb1 => "OP_CLTV".to_owned(),
                            0xb2 => "OP_CSV".to_owned(),
                            0xff => "OP_UNKNOWN(0xff)".to_owned(),
                            _ => op
                                .name()
                                .map(str::to_owned)
                                .unwrap_or_else(|| format!("OP_UNKNOWN(0x{:02x})", op.0)),
                        }
                    }
                }
            };
            append_word(&mut out, &word)?;
        }
        Ok(out)
    }

    /// Core ScriptToAsmStr without signature/sighash interpretation.
    /// This is display text, not CoreFormat's round-trip grammar. Malformed tails
    /// appear as [error], matching Core's diagnostic rendering.
    pub fn to_core_asm(&self) -> Result<String, Error> {
        let mut out = String::new();
        for item in self.instructions() {
            let word = match item {
                Err(_) => {
                    append_word(&mut out, "[error]")?;
                    break;
                }
                Ok(item) => match item.kind {
                    InstructionKind::Push { data, .. } if data.len() <= 4 => {
                        decode_script_number(data, 4, false)?.to_string()
                    }
                    InstructionKind::Push { data, .. } => bitcoin_encoding::hex_encode(data)?,
                    InstructionKind::Op(op) => op.name().unwrap_or("OP_UNKNOWN").to_owned(),
                },
            };
            append_word(&mut out, &word)?;
        }
        Ok(out)
    }

    /// Core FormatScript's lossless representation, including malformed tails.
    /// Parsing this with CoreFormat always recovers the exact original bytes.
    pub fn to_format_asm(&self) -> Result<String, Error> {
        let mut out = String::new();
        let mut cursor = 0;
        for result in self.instructions() {
            let item = match result {
                Ok(item) => item,
                Err(_) => {
                    append_raw_word(&mut out, &self.bytes[cursor..])?;
                    break;
                }
            };
            cursor += item.raw.len();
            if let Some(value) = item.opcode.small_integer() {
                append_word(&mut out, &value.to_string())?;
            } else if (0x61..=0xb9).contains(&item.opcode.0) {
                let name = item.opcode.name().expect("assigned opcode name");
                append_word(&mut out, name.strip_prefix("OP_").unwrap_or(name))?;
            } else if let Some(data) = item.pushed_bytes().filter(|data| !data.is_empty()) {
                append_raw_word(&mut out, &item.raw[..item.raw.len() - data.len()])?;
                append_raw_word(&mut out, data)?;
            } else {
                append_raw_word(&mut out, item.raw)?;
            }
        }
        Ok(out)
    }
}

fn wallet_hex(data: &[u8]) -> Result<String, Error> {
    let hex = bitcoin_encoding::hex_encode(data)?;
    Ok(if hex.len() == 2 && hex.starts_with('0') {
        hex[1..].to_owned()
    } else {
        hex
    })
}

fn append_word(out: &mut String, word: &str) -> Result<(), Error> {
    let length = out
        .len()
        .saturating_add(usize::from(!out.is_empty()))
        .saturating_add(word.len());
    if length > MAX_ASM_BYTES {
        return Err(Error::AsmTooLarge { length });
    }
    if !out.is_empty() {
        out.push(' ');
    }
    out.push_str(word);
    Ok(())
}

fn append_raw_word(out: &mut String, bytes: &[u8]) -> Result<(), Error> {
    append_word(out, &format!("0x{}", bitcoin_encoding::hex_encode(bytes)?))
}

fn parse_wallet_word(builder: &mut ScriptBuilder, word: &str, token: usize) -> Result<(), Error> {
    let invalid = || Error::InvalidAsm { token };
    if let Some(raw) = word
        .strip_prefix("OP_UNKNOWN(0x")
        .and_then(|s| s.strip_suffix(')'))
    {
        if raw.len() != 2 {
            return Err(invalid());
        }
        let raw = bitcoin_encoding::hex_decode(raw).map_err(|_| invalid())?;
        builder.append_opcode(Opcode(raw[0]))?;
        return Ok(());
    }
    // Match the retained wallet's hex-oriented grammar, including OP_10..16's
    // historical hexadecimal interpretation. -1/OP_1NEGATE are not wallet hex.
    let opcode = if matches!(word, "OP_1NEGATE" | "-1" | "OP_INVALIDOPCODE") {
        None
    } else {
        Opcode::from_name(word)
    };
    if let Some(opcode) = opcode {
        if opcode.0 == 0 {
            builder.push_data(&[])?;
            return Ok(());
        }
        if !opcode.is_push_only_opcode() || opcode.0 == 0x50 {
            builder.append_opcode(opcode)?;
            return Ok(());
        }
    }
    let stripped = word.replace("OP_", "");
    let data_text = if stripped.eq_ignore_ascii_case("TRUE") {
        "1"
    } else if stripped.eq_ignore_ascii_case("FALSE") {
        "0"
    } else {
        &stripped
    };
    let padded;
    let data_text = if data_text.len() == 1 {
        padded = format!("0{data_text}");
        padded.as_str()
    } else {
        data_text
    };
    let data = bitcoin_encoding::hex_decode(data_text).map_err(|_| invalid())?;
    builder.push_data(&data)?;
    Ok(())
}

fn parse_core_word(builder: &mut ScriptBuilder, word: &str, token: usize) -> Result<(), Error> {
    let invalid = || Error::InvalidAsm { token };
    let digits = word.strip_prefix('-').unwrap_or(word);
    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
        let value: i64 = word.parse().map_err(|_| invalid())?;
        if !(-4_294_967_295..=4_294_967_295).contains(&value) {
            return Err(invalid());
        }
        builder.push_number(value)?;
    } else if let Some(raw) = word.strip_prefix("0x") {
        if raw.is_empty() {
            return Err(invalid());
        }
        let data = bitcoin_encoding::hex_decode(raw).map_err(|_| invalid())?;
        builder.append_raw(&data)?;
    } else if word.len() >= 2 && word.starts_with('\'') && word.ends_with('\'') {
        builder.push_data_length_only(&word.as_bytes()[1..word.len() - 1])?;
    } else {
        let expanded;
        let name = if word.starts_with("OP_") {
            word
        } else {
            expanded = format!("OP_{word}");
            &expanded
        };
        let opcode = Opcode::from_name(name).ok_or_else(invalid)?;
        // Core's ParseScript opcode map excludes push opcodes, CHECKSIGADD and
        // INVALIDOPCODE; they are expressible via 0x raw bytes instead.
        if opcode.0 != 0x50 && !(0x61..=0xb9).contains(&opcode.0) {
            return Err(invalid());
        }
        // These aliases exist for wallet text, not the pinned Core parser map.
        if matches!(
            name,
            "OP_HODL" | "OP_CLTV" | "OP_CSV" | "OP_NOP2" | "OP_NOP3"
        ) {
            return Err(invalid());
        }
        builder.append_opcode(opcode)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WitnessProgram<'a> {
    pub version: u8,
    pub program: &'a [u8],
}

/// Syntactic witness envelope, including unsupported v0 lengths and future versions.
/// Requires the exact one-byte length push; PUSHDATA encodings do not qualify.
pub fn witness_program(bytes: &[u8]) -> Option<WitnessProgram<'_>> {
    if !(4..=42).contains(&bytes.len()) {
        return None;
    }
    let version = match bytes[0] {
        0 => 0,
        0x51..=0x60 => bytes[0] - 0x50,
        _ => return None,
    };
    if bytes[1] as usize + 2 != bytes.len() {
        return None;
    }
    Some(WitnessProgram {
        version,
        program: &bytes[2..],
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicKeyEncoding {
    Compressed,
    Uncompressed,
    Hybrid,
}

/// Encoding shape only. No elliptic curve or x-coordinate validation.
pub fn public_key_encoding(bytes: &[u8]) -> Option<PublicKeyEncoding> {
    match (bytes.first().copied(), bytes.len()) {
        (Some(2 | 3), 33) => Some(PublicKeyEncoding::Compressed),
        (Some(4), 65) => Some(PublicKeyEncoding::Uncompressed),
        (Some(6 | 7), 65) => Some(PublicKeyEncoding::Hybrid),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputTemplate<'a> {
    P2pkh(&'a [u8; 20]),
    P2sh(&'a [u8; 20]),
    P2wpkh(&'a [u8; 20]),
    P2wsh(&'a [u8; 32]),
    P2tr(&'a [u8; 32]),
    P2anchor,
    /// Witness v0 envelopes of lengths other than 20/32 are not recognized outputs.
    InvalidWitnessV0 {
        program: &'a [u8],
    },
    /// Future/unknown v1..16 program: address encoding is not spendability evidence.
    WitnessUnknown(WitnessProgram<'a>),
    P2pk {
        public_key: &'a [u8],
        encoding: PublicKeyEncoding,
    },
    Multisig {
        required: u8,
        public_keys: Vec<&'a [u8]>,
    },
    /// Tail is a valid instruction stream. push_only uses Core's syntactic rule.
    OpReturn {
        tail: &'a [u8],
        push_only: bool,
    },
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputType {
    P2pkh,
    P2sh,
    P2wpkh,
    P2wsh,
    P2tr,
    P2anchor,
    InvalidWitnessV0,
    WitnessUnknown,
    P2pk,
    Multisig,
    NullData,
    NonstandardReturn,
    Unknown,
}

impl OutputTemplate<'_> {
    pub const fn output_type(&self) -> OutputType {
        match self {
            Self::P2pkh(_) => OutputType::P2pkh,
            Self::P2sh(_) => OutputType::P2sh,
            Self::P2wpkh(_) => OutputType::P2wpkh,
            Self::P2wsh(_) => OutputType::P2wsh,
            Self::P2tr(_) => OutputType::P2tr,
            Self::P2anchor => OutputType::P2anchor,
            Self::InvalidWitnessV0 { .. } => OutputType::InvalidWitnessV0,
            Self::WitnessUnknown(_) => OutputType::WitnessUnknown,
            Self::P2pk { .. } => OutputType::P2pk,
            Self::Multisig { .. } => OutputType::Multisig,
            Self::OpReturn {
                push_only: true, ..
            } => OutputType::NullData,
            Self::OpReturn {
                push_only: false, ..
            } => OutputType::NonstandardReturn,
            Self::Unknown => OutputType::Unknown,
        }
    }
}

/// Exact template recognition; malformed streams are an error, not silently unknown.
pub fn classify_output(bytes: &[u8]) -> Result<OutputTemplate<'_>, Error> {
    validate(bytes, ValidationPolicy::default())?;
    if bytes.len() == 25 && bytes[..3] == [0x76, 0xa9, 20] && bytes[23..] == [0x88, 0xac] {
        return Ok(OutputTemplate::P2pkh(
            bytes[3..23].try_into().expect("20-byte slice"),
        ));
    }
    if bytes.len() == 23 && bytes[..2] == [0xa9, 20] && bytes[22] == 0x87 {
        return Ok(OutputTemplate::P2sh(
            bytes[2..22].try_into().expect("20-byte slice"),
        ));
    }
    if let Some(witness) = witness_program(bytes) {
        return Ok(match (witness.version, witness.program.len()) {
            (0, 20) => OutputTemplate::P2wpkh(witness.program.try_into().expect("20-byte slice")),
            (0, 32) => OutputTemplate::P2wsh(witness.program.try_into().expect("32-byte slice")),
            (0, _) => OutputTemplate::InvalidWitnessV0 {
                program: witness.program,
            },
            (1, 32) => OutputTemplate::P2tr(witness.program.try_into().expect("32-byte slice")),
            (1, 2) if witness.program == [0x4e, 0x73] => OutputTemplate::P2anchor,
            _ => OutputTemplate::WitnessUnknown(witness),
        });
    }
    if bytes.first() == Some(&0x6a) {
        return Ok(OutputTemplate::OpReturn {
            tail: &bytes[1..],
            push_only: validate(&bytes[1..], ValidationPolicy::default())?.is_push_only,
        });
    }
    if matches!(bytes.len(), 35 | 67)
        && bytes[0] as usize == bytes.len() - 2
        && bytes[bytes.len() - 1] == 0xac
    {
        let key = &bytes[1..bytes.len() - 1];
        if let Some(encoding) = public_key_encoding(key) {
            return Ok(OutputTemplate::P2pk {
                public_key: key,
                encoding,
            });
        }
    }
    if bytes.last() == Some(&0xae)
        && let Some(template) = match_multisig(bytes)?
    {
        return Ok(template);
    }
    Ok(OutputTemplate::Unknown)
}

fn threshold(item: Instruction<'_>) -> Option<u8> {
    if item.is_minimal_push() == Some(false) {
        return None;
    }
    let number = item.script_number(4, true).ok().flatten()?;
    if !(1..=MAX_MULTISIG_KEYS as i64).contains(&number) {
        return None;
    }
    Some(number as u8)
}

fn match_multisig(bytes: &[u8]) -> Result<Option<OutputTemplate<'_>>, Error> {
    let mut iter = instructions(bytes)?;
    let Some(first) = iter.next().transpose()? else {
        return Ok(None);
    };
    let Some(required) = threshold(first) else {
        return Ok(None);
    };
    let mut keys = Vec::with_capacity(MAX_MULTISIG_KEYS);
    for item in iter.by_ref() {
        let item = item?;
        if let Some(key) = item
            .pushed_bytes()
            .filter(|data| public_key_encoding(data).is_some())
        {
            if keys.len() == MAX_MULTISIG_KEYS {
                return Ok(None);
            }
            keys.push(key);
            continue;
        }
        if threshold(item) != Some(keys.len() as u8) || keys.len() < required as usize {
            return Ok(None);
        }
        let Some(last) = iter.next().transpose()? else {
            return Ok(None);
        };
        if last.opcode != opcodes::OP_CHECKMULTISIG || iter.next().is_some() {
            return Ok(None);
        }
        return Ok(Some(OutputTemplate::Multisig {
            required,
            public_keys: keys,
        }));
    }
    Ok(None)
}

fn fixed_output(prefix: &[u8], payload: &[u8], suffix: &[u8]) -> Script {
    let mut bytes = Vec::with_capacity(prefix.len() + payload.len() + suffix.len());
    bytes.extend_from_slice(prefix);
    bytes.extend_from_slice(payload);
    bytes.extend_from_slice(suffix);
    Script { bytes }
}

pub fn p2pkh(hash: &[u8; 20]) -> Script {
    fixed_output(&[0x76, 0xa9, 20], hash, &[0x88, 0xac])
}
pub fn p2sh(hash: &[u8; 20]) -> Script {
    fixed_output(&[0xa9, 20], hash, &[0x87])
}
pub fn p2wpkh(hash: &[u8; 20]) -> Script {
    fixed_output(&[0, 20], hash, &[])
}
pub fn p2wsh(hash: &[u8; 32]) -> Script {
    fixed_output(&[0, 32], hash, &[])
}
pub fn p2tr(output_key: &[u8; 32]) -> Script {
    fixed_output(&[0x51, 32], output_key, &[])
}
pub fn p2anchor() -> Script {
    fixed_output(&[0x51, 2], &[0x4e, 0x73], &[])
}

/// Valid witness-address program policy, distinct from envelope recognition.
pub fn witness_output(version: u8, program: &[u8]) -> Result<Script, Error> {
    if version > 16 {
        return Err(Error::InvalidWitnessVersion { version });
    }
    if !(2..=40).contains(&program.len()) || (version == 0 && !matches!(program.len(), 20 | 32)) {
        return Err(Error::InvalidWitnessLength {
            version,
            length: program.len(),
        });
    }
    Ok(fixed_output(
        &[
            if version == 0 { 0 } else { 0x50 + version },
            program.len() as u8,
        ],
        program,
        &[],
    ))
}

pub fn p2pk(public_key: &[u8]) -> Result<Script, Error> {
    if public_key_encoding(public_key).is_none() {
        return Err(Error::InvalidPublicKeyShape {
            length: public_key.len(),
        });
    }
    Ok(fixed_output(&[public_key.len() as u8], public_key, &[0xac]))
}

pub fn multisig(required: u8, public_keys: &[&[u8]]) -> Result<Script, Error> {
    if required == 0
        || public_keys.len() > MAX_MULTISIG_KEYS
        || required as usize > public_keys.len()
    {
        return Err(Error::InvalidMultisig {
            required,
            keys: public_keys.len(),
        });
    }
    let mut builder = ScriptBuilder::new();
    builder.push_number(i64::from(required))?;
    for key in public_keys {
        if public_key_encoding(key).is_none() {
            return Err(Error::InvalidPublicKeyShape { length: key.len() });
        }
        builder.push_data(key)?;
    }
    builder
        .push_number(public_keys.len() as i64)?
        .append_opcode(opcodes::OP_CHECKMULTISIG)?;
    Ok(builder.finish())
}

/// Data-carrying output format only; no relay size/standardness promise.
pub fn op_return(data: &[&[u8]]) -> Result<Script, Error> {
    let mut builder = ScriptBuilder::new();
    builder.append_opcode(opcodes::OP_RETURN)?;
    for item in data {
        builder.push_data(item)?;
    }
    Ok(builder.finish())
}

/// Existing address payloads only. Revalidates mutable address fields with the
/// actual first-party address codec; never creates/derives public keys or hashes.
pub fn script_from_address(address: &Address) -> Result<Script, Error> {
    bitcoin_encoding::address_encode(address)?;
    match address {
        Address::Legacy(a) => Ok(match a.kind {
            LegacyKind::P2pkh => p2pkh(&a.hash),
            LegacyKind::P2sh => p2sh(&a.hash),
        }),
        Address::Witness(a) => witness_output(a.version, &a.program),
    }
}

pub fn script_from_address_text(text: &str, network: Network) -> Result<Script, Error> {
    script_from_address(&bitcoin_encoding::address_decode(text, network)?)
}

/// P2PK and multisig have no direct address here: hashing/curve logic is not hidden
/// inside this API. Invalid v0 witness lengths are explicit errors.
pub fn address_from_script(bytes: &[u8], network: Network) -> Result<Option<String>, Error> {
    let address = match classify_output(bytes)? {
        OutputTemplate::P2pkh(hash) => {
            bitcoin_encoding::legacy_address_encode(network, LegacyKind::P2pkh, hash)?
        }
        OutputTemplate::P2sh(hash) => {
            bitcoin_encoding::legacy_address_encode(network, LegacyKind::P2sh, hash)?
        }
        OutputTemplate::P2wpkh(hash) => bitcoin_encoding::witness_address_encode(network, 0, hash)?,
        OutputTemplate::P2wsh(hash) => bitcoin_encoding::witness_address_encode(network, 0, hash)?,
        OutputTemplate::P2tr(key) => bitcoin_encoding::witness_address_encode(network, 1, key)?,
        OutputTemplate::P2anchor => {
            bitcoin_encoding::witness_address_encode(network, 1, &[0x4e, 0x73])?
        }
        OutputTemplate::WitnessUnknown(witness) => {
            bitcoin_encoding::witness_address_encode(network, witness.version, witness.program)?
        }
        OutputTemplate::InvalidWitnessV0 { program } => {
            return Err(Error::InvalidWitnessLength {
                version: 0,
                length: program.len(),
            });
        }
        _ => return Ok(None),
    };
    Ok(Some(address))
}
