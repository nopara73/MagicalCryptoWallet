//! Compatibility boundary for the application's retained JSON-RPC format.
//! Existing numeric IDs are echoed as strings; null IDs remain notifications.
use super::ServiceError;
use crate::json::{
    self, DuplicatePolicy, JsonString, Object, ParseOptions, SerializeOptions, StringEncoding,
    Value,
};

fn member(name: &str, value: Value) -> (JsonString, Value) {
    (JsonString::new(name), value)
}
fn invalid() -> ServiceError {
    ServiceError::protocol("invalid application RPC request")
}

pub(super) fn parse_requests(input: &[u8]) -> Result<Value, ServiceError> {
    // These two legacy extensions were accepted by the old RPC loader.
    let options = ParseOptions {
        extensions: json::Extensions {
            comments: true,
            trailing_commas: true,
            // The existing HTTP StreamReader consumed an initial UTF-8 BOM.
            utf8_bom: true,
        },
        ..ParseOptions::default()
    };
    let options = ParseOptions {
        duplicates: DuplicatePolicy::Preserve,
        ..options
    };
    let normalized = normalize_legacy_quotes(input)?;
    let document = json::parse(&normalized, &options).map_err(ServiceError::json)?;
    let batch = matches!(document.root(), Value::Array(_));
    let items = match document.root() {
        Value::Array(array) => array.as_slice(),
        value => std::slice::from_ref(value),
    };
    if items.len() > 1024 {
        return Err(ServiceError::limit());
    }
    let mut requests = Vec::new();
    requests
        .try_reserve_exact(items.len())
        .map_err(|_| ServiceError::limit())?;
    for item in items {
        let object = item.as_object().ok_or_else(invalid)?;
        // Constructor properties were matched case-insensitively, with the
        // last occurrence winning. Parameter object names remain exact.
        let field = |name: &str| {
            object
                .members()
                .iter()
                .rev()
                .find(|(key, _)| key.as_str().eq_ignore_ascii_case(name))
                .map(|(_, value)| value)
        };
        let method = legacy_string(field("method").ok_or_else(invalid)?)?.ok_or_else(invalid)?;
        let version = (match field("jsonrpc") {
            None => None,
            Some(value) => legacy_string(value)?,
        })
        .map_or(Value::Null, Value::string);
        let id = (match field("id") {
            None => None,
            Some(value) => legacy_string(value)?,
        })
        .map_or(Value::Null, Value::string);
        let parameters = normalize_duplicates(field("params").unwrap_or(&Value::Null));
        requests.push(Value::Object(Object::new(vec![
            member("jsonrpc", version),
            member("id", id),
            member("method", Value::string(method)),
            member("params", parameters),
        ])));
    }
    Ok(Value::Array(vec![
        Value::Bool(batch),
        Value::Array(requests),
    ]))
}

fn response(input: &Value) -> Result<Value, ServiceError> {
    let parts = input
        .as_array()
        .ok_or_else(|| ServiceError::protocol("invalid RPC response payload"))?;
    if parts.len() != 4 || !matches!(parts[0], Value::Null | Value::String(_)) {
        return Err(ServiceError::protocol("invalid RPC response payload"));
    }
    let body = if matches!(parts[2], Value::Null) {
        member("result", parts[1].clone())
    } else {
        let code = parts[2]
            .as_number()
            .and_then(|number| number.to_i64_exact().ok())
            .and_then(|number| i32::try_from(number).ok())
            .ok_or_else(|| ServiceError::protocol("invalid RPC error code"))?;
        let message = parts[3]
            .as_str()
            .ok_or_else(|| ServiceError::protocol("invalid RPC error message"))?;
        member(
            "error",
            Value::Object(Object::new(vec![
                member("code", Value::Number(json::Number::from(i64::from(code)))),
                member("message", Value::string(message)),
            ])),
        )
    };
    Ok(Value::Object(Object::new(vec![
        member("jsonrpc", Value::string("2.0")),
        body,
        member("id", parts[0].clone()),
    ])))
}
fn write(value: &Value) -> Result<Vec<u8>, ServiceError> {
    json::serialize(
        value,
        &SerializeOptions {
            strings: StringEncoding::Minimal,
            ..SerializeOptions::default()
        },
    )
    .map(String::into_bytes)
    .map_err(ServiceError::json)
}
pub(super) fn write_response(input: &Value) -> Result<Vec<u8>, ServiceError> {
    write(&response(input)?)
}
pub(super) fn write_batch(input: &Value) -> Result<Vec<u8>, ServiceError> {
    let requests = input
        .as_array()
        .ok_or_else(|| ServiceError::protocol("invalid RPC batch response"))?;
    if requests.len() > 1024 {
        return Err(ServiceError::limit());
    }
    let responses = requests
        .iter()
        .map(response)
        .collect::<Result<Vec<_>, _>>()?;
    write(&Value::Array(responses))
}

fn legacy_string(value: &Value) -> Result<Option<String>, ServiceError> {
    match value {
        Value::Null => Ok(None),
        Value::String(text) => Ok(Some(text.as_str().into())),
        Value::Number(number) => Ok(Some(number.as_str().into())),
        Value::Bool(value) => Ok(Some(value.to_string())),
        _ => Err(invalid()),
    }
}
fn normalize_duplicates(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(normalize_duplicates).collect()),
        Value::Object(object) => {
            let mut members: Vec<(JsonString, Value)> = Vec::new();
            let mut indexes = std::collections::BTreeMap::new();
            for (key, value) in object.members() {
                let value = normalize_duplicates(value);
                if let Some(&index) = indexes.get(key.as_str()) {
                    members[index] = (key.clone(), value);
                } else {
                    indexes.insert(key.as_str(), members.len());
                    members.push((key.clone(), value));
                }
            }
            Value::Object(Object::new(members))
        }
        value => value.clone(),
    }
}
// Small lexical compatibility pass: only quote style and bare object keys.
// Structural grammar, UTF-8, numbers, escapes, limits and comments are validated
// by the first-party engine. This pass never executes or evaluates input.
fn normalize_legacy_quotes(input: &[u8]) -> Result<Vec<u8>, ServiceError> {
    let mut output = Vec::new();
    let mut at = 0;
    let mut key_position = false;
    while at < input.len() {
        let byte = input[at];
        if byte == b'/' && input.get(at + 1) == Some(&b'/') {
            let start = at;
            at += 2;
            while at < input.len() && input[at] != b'\n' {
                at += 1;
            }
            output.extend_from_slice(&input[start..at]);
            continue;
        }
        if byte == b'/' && input.get(at + 1) == Some(&b'*') {
            let start = at;
            at += 2;
            while at + 1 < input.len() && &input[at..at + 2] != b"*/" {
                at += 1;
            }
            at = (at + 2).min(input.len());
            output.extend_from_slice(&input[start..at]);
            continue;
        }
        if byte == b'"' || byte == b'\'' {
            let single = byte == b'\'';
            output.push(b'"');
            at += 1;
            let mut ended = false;
            while at < input.len() {
                let current = input[at];
                at += 1;
                if current == byte {
                    ended = true;
                    output.push(b'"');
                    break;
                }
                if current == b'\\' {
                    let next = *input.get(at).ok_or_else(invalid)?;
                    at += 1;
                    if single && next == b'\'' {
                        output.push(next);
                    } else {
                        output.push(b'\\');
                        output.push(next);
                    }
                } else {
                    if single && current == b'"' {
                        output.push(b'\\');
                    }
                    output.push(current);
                }
            }
            if !ended {
                return Err(invalid());
            }
            key_position = false;
            continue;
        }
        if key_position && (byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'$')) {
            let start = at;
            at += 1;
            while at < input.len()
                && (input[at].is_ascii_alphanumeric() || matches!(input[at], b'_' | b'$'))
            {
                at += 1;
            }
            let mut next = at;
            while input.get(next).is_some_and(u8::is_ascii_whitespace) {
                next += 1;
            }
            if input.get(next) == Some(&b':') {
                output.push(b'"');
                output.extend_from_slice(&input[start..at]);
                output.push(b'"');
            } else {
                output.extend_from_slice(&input[start..at]);
            }
            key_position = false;
            continue;
        }
        if !byte.is_ascii_whitespace() {
            key_position = matches!(byte, b'{' | b',');
        }
        output.push(byte);
        at += 1;
    }
    if output.len() > super::MAX_JSON {
        return Err(ServiceError::limit());
    }
    Ok(output)
}
// JToken's default date detection affected only parameter values, not the string
// constructor fields. Retain its string guard behavior without managed parsing.
pub(super) fn legacy_date(text: &str) -> bool {
    if let Some(body) = text
        .strip_prefix("/Date(")
        .and_then(|s| s.strip_suffix(")/"))
    {
        let integer = body
            .get(1..)
            .unwrap_or("")
            .find(['+', '-'])
            .map_or(body, |at| &body[..at + 1]);
        return integer.parse::<i64>().is_ok();
    }
    let bytes = text.as_bytes();
    if !(19..=40).contains(&bytes.len()) {
        return false;
    }
    let digits = |start: usize, length: usize| -> Option<u32> {
        let part = bytes.get(start..start + length)?;
        if part.iter().all(u8::is_ascii_digit) {
            std::str::from_utf8(part).ok()?.parse().ok()
        } else {
            None
        }
    };
    if bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return false;
    }
    let Some(year) = digits(0, 4).filter(|v| *v != 0) else {
        return false;
    };
    let Some(month) = digits(5, 2).filter(|v| (1..=12).contains(v)) else {
        return false;
    };
    let days = match month {
        2 => {
            if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if !digits(8, 2).is_some_and(|day| (1..=days).contains(&day)) {
        return false;
    }
    let Some(hour) = digits(11, 2).filter(|v| *v <= 24) else {
        return false;
    };
    let Some(minute) = digits(14, 2).filter(|v| *v < 60) else {
        return false;
    };
    let Some(second) = digits(17, 2).filter(|v| *v < 60) else {
        return false;
    };
    let mut at = 19;
    if bytes.get(at) == Some(&b'.') {
        at += 1;
        let start = at;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        if !(1..=7).contains(&(at - start))
            || (hour == 24 && bytes[start..at].iter().any(|v| *v != b'0'))
        {
            return false;
        }
    }
    if hour == 24 && (minute != 0 || second != 0) {
        return false;
    }
    match &bytes[at..] {
        [] | [b'Z'] => true,
        [b'+' | b'-', _, _] => digits(at + 1, 2).is_some_and(|v| v <= 23),
        [b'+' | b'-', _, _, _, _] => {
            digits(at + 1, 2).is_some_and(|v| v <= 23) && digits(at + 3, 2).is_some_and(|v| v < 60)
        }
        [b'+' | b'-', _, _, b':', _, _] => {
            digits(at + 1, 2).is_some_and(|v| v <= 23) && digits(at + 4, 2).is_some_and(|v| v < 60)
        }
        _ => false,
    }
}
