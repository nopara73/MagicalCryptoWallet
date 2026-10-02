//! Explicit schema compatibility helpers. None changes the strict JSON grammar.
//! Domain owners supply Bitcoin validation, dates, defaults, enum mappings,
//! version migration, and transaction encodings using these checked primitives.

use super::{Number, NumberConversionError, Object, Value};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldCase {
    Exact,
    /// Legacy Decode.Field's initial ASCII lowercase/PascalCase fallback.
    AsciiPascalAlias,
    /// Legacy OutPoint converter's case-insensitive ASCII property matching.
    AsciiInsensitive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntegerInput {
    NumberOnly,
    /// Legacy integer fields may also be an ASCII signed decimal string.
    /// Surrounding ASCII whitespace, '+' and leading zeros are allowed;
    /// exponent/fraction strings and locale-specific digits/separators are not.
    NumberOrDecimalString,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    MissingField,
    AmbiguousField,
    ExpectedObject,
    ExpectedString,
    ExpectedBool,
    ExpectedNumber,
    InvalidIntegerString,
    InvalidDecimalString,
    Number(NumberConversionError),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "JSON schema decode: {self:?}")
    }
}
impl std::error::Error for DecodeError {}

/// Missing and explicit null remain distinct. Any matching duplicate/alias
/// collision fails, even if the duplicate values happen to be equal.
pub fn field<'a>(
    object: &'a Object,
    name: &str,
    case: FieldCase,
) -> Result<Option<&'a Value>, DecodeError> {
    let mut found = None;
    for (key, value) in object.members() {
        let key = key.as_str();
        let matches = match case {
            FieldCase::Exact => key == name,
            FieldCase::AsciiInsensitive => key.eq_ignore_ascii_case(name),
            FieldCase::AsciiPascalAlias => {
                key == name
                    || (!name.is_empty()
                        && name.as_bytes()[0].is_ascii_lowercase()
                        && key.len() == name.len()
                        && key.as_bytes()[0] == name.as_bytes()[0].to_ascii_uppercase()
                        && key.as_bytes()[1..] == name.as_bytes()[1..])
            }
        };
        if matches {
            if found.is_some() {
                return Err(DecodeError::AmbiguousField);
            }
            found = Some(value);
        }
    }
    Ok(found)
}

pub fn required_field<'a>(
    object: &'a Object,
    name: &str,
    case: FieldCase,
) -> Result<&'a Value, DecodeError> {
    field(object, name, case)?.ok_or(DecodeError::MissingField)
}

pub fn object(value: &Value) -> Result<&Object, DecodeError> {
    value.as_object().ok_or(DecodeError::ExpectedObject)
}

pub fn string(value: &Value) -> Result<&str, DecodeError> {
    value.as_str().ok_or(DecodeError::ExpectedString)
}

pub fn boolean(value: &Value) -> Result<bool, DecodeError> {
    value.as_bool().ok_or(DecodeError::ExpectedBool)
}

/// Exact fixed-point integer, including exponents in numeric tokens.
/// Strings are excluded; current MoneyBitcoins uses a separate string adapter.
pub fn scaled_i128(value: &Value, scale: u32) -> Result<i128, DecodeError> {
    value
        .as_number()
        .ok_or(DecodeError::ExpectedNumber)?
        .to_scaled_i128(scale)
        .map_err(DecodeError::Number)
}

/// Current wallet/config MoneyBitcoins is a string, while MoneySatoshis is a
/// number. Accept a plain JSON decimal spelling inside that string, without
/// locale coercion, exponent syntax, whitespace, or rounding. This deliberately
/// narrows Money.Parse's user-input leniency at the persisted-schema boundary.
pub fn scaled_decimal_string_i128(value: &Value, scale: u32) -> Result<i128, DecodeError> {
    let token = string(value)?;
    if token.contains(['e', 'E']) {
        return Err(DecodeError::InvalidDecimalString);
    }
    Number::parse(token)
        .map_err(|_| DecodeError::InvalidDecimalString)?
        .to_scaled_i128(scale)
        .map_err(DecodeError::Number)
}

pub fn integer_i128(value: &Value, input: IntegerInput) -> Result<i128, DecodeError> {
    match value {
        Value::Number(number) => number.to_i128_exact().map_err(DecodeError::Number),
        Value::String(text) if input == IntegerInput::NumberOrDecimalString => {
            let token = text
                .as_str()
                .trim_matches(|c: char| c.is_ascii_whitespace());
            let digits = token.strip_prefix(['+', '-']).unwrap_or(token);
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return Err(DecodeError::InvalidIntegerString);
            }
            token
                .parse::<i128>()
                .map_err(|_| DecodeError::Number(NumberConversionError::OutOfRange))
        }
        _ => Err(DecodeError::ExpectedNumber),
    }
}

pub fn integer_i64(value: &Value, input: IntegerInput) -> Result<i64, DecodeError> {
    integer_i128(value, input)?
        .try_into()
        .map_err(|_| DecodeError::Number(NumberConversionError::OutOfRange))
}

pub fn integer_u64(value: &Value, input: IntegerInput) -> Result<u64, DecodeError> {
    integer_i128(value, input)?
        .try_into()
        .map_err(|_| DecodeError::Number(NumberConversionError::OutOfRange))
}

/// Preserve old UseTor's bool-or-string union, without inventing enum validation.
pub fn tor_setting(value: &Value) -> Result<&str, DecodeError> {
    match value {
        Value::Bool(true) => Ok("Enabled"),
        Value::Bool(false) => Ok("Disabled"),
        Value::String(text) => Ok(text.as_str()),
        _ => Err(DecodeError::ExpectedString),
    }
}

/// Construct amounts without converting through a managed or Rust float.
pub fn fixed_point(units: i128, scale: u32) -> Result<Value, super::ErrorKind> {
    Number::from_scaled_i128(units, scale).map(Value::Number)
}

/// String counterpart of fixed_point. Trimming fractional zeroes is explicit,
/// matching the current MoneyBitcoins encoder when the caller requests it.
pub fn fixed_point_string(
    units: i128,
    scale: u32,
    trim_fractional_zeroes: bool,
) -> Result<Value, super::ErrorKind> {
    let number = Number::from_scaled_i128(units, scale)?;
    let token = if trim_fractional_zeroes && scale > 0 {
        number.as_str().trim_end_matches('0').trim_end_matches('.')
    } else {
        number.as_str()
    };
    Ok(Value::string(super::copy_string(token)?))
}
