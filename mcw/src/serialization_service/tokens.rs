//! Bounded typed-value interchange, not another JSON grammar implementation.
use super::{MAX_TRANSFER, ServiceError};
use crate::json::{JsonString, Limits, Number, Object, Value};

pub fn encode(value: &Value) -> Result<Vec<u8>, ServiceError> {
    encode_with_dates(value, false)
}
pub(super) fn encode_rpc(value: &Value) -> Result<Vec<u8>, ServiceError> {
    encode_with_dates(value, true)
}
fn encode_with_dates(value: &Value, rpc: bool) -> Result<Vec<u8>, ServiceError> {
    let mut writer = Writer {
        output: Vec::new(),
        rpc,
        nodes: 0,
        strings: 0,
        limits: Limits::default(),
    };
    writer.value(value, 0, false)?;
    Ok(writer.output)
}
pub fn decode(input: &[u8]) -> Result<Value, ServiceError> {
    if input.len() > MAX_TRANSFER {
        return Err(ServiceError::limit());
    }
    let mut reader = Reader {
        input,
        offset: 0,
        nodes: 0,
        strings: 0,
        limits: Limits::default(),
    };
    let value = reader.value(0)?;
    if reader.offset != input.len() {
        return Err(ServiceError::protocol("trailing typed JSON bytes"));
    }
    Ok(value)
}

struct Writer {
    rpc: bool,
    output: Vec<u8>,
    nodes: usize,
    strings: usize,
    limits: Limits,
}
impl Writer {
    fn bytes(&mut self, value: &[u8]) -> Result<(), ServiceError> {
        if self.output.len().saturating_add(value.len()) > MAX_TRANSFER {
            return Err(ServiceError::limit());
        }
        self.output
            .try_reserve(value.len())
            .map_err(|_| ServiceError::limit())?;
        self.output.extend_from_slice(value);
        Ok(())
    }
    fn count(&mut self, count: usize) -> Result<(), ServiceError> {
        self.bytes(&(u32::try_from(count).map_err(|_| ServiceError::limit())?).to_le_bytes())
    }
    fn text(&mut self, text: &str, number: bool) -> Result<(), ServiceError> {
        if text.len()
            > if number {
                self.limits.number_bytes
            } else {
                self.limits.string_bytes
            }
        {
            return Err(ServiceError::limit());
        }
        if !number {
            self.strings = self
                .strings
                .checked_add(text.len())
                .ok_or_else(ServiceError::limit)?;
            if self.strings > self.limits.total_decoded_bytes {
                return Err(ServiceError::limit());
            }
        }
        self.count(text.len())?;
        self.bytes(text.as_bytes())
    }
    fn node(&mut self) -> Result<(), ServiceError> {
        self.nodes += 1;
        if self.nodes > self.limits.nodes {
            return Err(ServiceError::limit());
        }
        Ok(())
    }
    fn value(&mut self, value: &Value, depth: usize, dates: bool) -> Result<(), ServiceError> {
        self.node()?;
        match value {
            Value::Null => self.bytes(&[0]),
            Value::Bool(false) => self.bytes(&[1]),
            Value::Bool(true) => self.bytes(&[2]),
            Value::Number(number) => {
                self.bytes(&[3])?;
                self.text(number.as_str(), true)
            }
            Value::String(text) => {
                self.bytes(&[if dates && super::rpc::legacy_date(text.as_str()) {
                    7
                } else {
                    4
                }])?;
                self.text(text.as_str(), false)
            }
            Value::Array(array) => {
                if depth >= self.limits.depth || array.len() > self.limits.container_entries {
                    return Err(ServiceError::limit());
                }
                self.bytes(&[5])?;
                self.count(array.len())?;
                for item in array {
                    self.value(item, depth + 1, dates)?;
                }
                Ok(())
            }
            Value::Object(object) => {
                if depth >= self.limits.depth
                    || object.members().len() > self.limits.container_entries
                {
                    return Err(ServiceError::limit());
                }
                self.bytes(&[6])?;
                self.count(object.members().len())?;
                let mut seen = std::collections::BTreeSet::new();
                for (key, value) in object.members() {
                    if !seen.insert(key.as_str()) {
                        return Err(ServiceError::protocol("duplicate typed JSON key"));
                    }
                    self.node()?;
                    self.text(key.as_str(), false)?;
                    self.value(
                        value,
                        depth + 1,
                        dates || (self.rpc && depth == 2 && key.as_str() == "params"),
                    )?;
                }
                Ok(())
            }
        }
    }
}

struct Reader<'a> {
    input: &'a [u8],
    offset: usize,
    nodes: usize,
    strings: usize,
    limits: Limits,
}
impl Reader<'_> {
    fn bytes(&mut self, count: usize) -> Result<&[u8], ServiceError> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or_else(ServiceError::limit)?;
        let part = self
            .input
            .get(self.offset..end)
            .ok_or_else(|| ServiceError::protocol("truncated typed JSON value"))?;
        self.offset = end;
        Ok(part)
    }
    fn count(&mut self) -> Result<usize, ServiceError> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()) as usize)
    }
    fn text(&mut self, number: bool) -> Result<String, ServiceError> {
        let count = self.count()?;
        if count
            > if number {
                self.limits.number_bytes
            } else {
                self.limits.string_bytes
            }
        {
            return Err(ServiceError::limit());
        }
        if !number {
            self.strings = self
                .strings
                .checked_add(count)
                .ok_or_else(ServiceError::limit)?;
            if self.strings > self.limits.total_decoded_bytes {
                return Err(ServiceError::limit());
            }
        }
        let text = std::str::from_utf8(self.bytes(count)?)
            .map_err(|_| ServiceError::protocol("invalid typed JSON UTF-8"))?;
        let mut output = String::new();
        output
            .try_reserve_exact(text.len())
            .map_err(|_| ServiceError::limit())?;
        output.push_str(text);
        Ok(output)
    }
    fn node(&mut self) -> Result<(), ServiceError> {
        self.nodes += 1;
        if self.nodes > self.limits.nodes {
            return Err(ServiceError::limit());
        }
        Ok(())
    }
    fn value(&mut self, depth: usize) -> Result<Value, ServiceError> {
        self.node()?;
        let tag = self.bytes(1)?[0];
        Ok(match tag {
            0 => Value::Null,
            1 => Value::Bool(false),
            2 => Value::Bool(true),
            3 => Value::Number(Number::parse(&self.text(true)?).map_err(ServiceError::json)?),
            4 => Value::string(self.text(false)?),
            5 | 6 => {
                if depth >= self.limits.depth {
                    return Err(ServiceError::limit());
                }
                let count = self.count()?;
                if count > self.limits.container_entries
                    || count > self.limits.nodes - self.nodes
                    || count > self.input.len() - self.offset
                {
                    return Err(ServiceError::limit());
                }
                if tag == 5 {
                    let mut items = Vec::new();
                    items
                        .try_reserve_exact(count)
                        .map_err(|_| ServiceError::limit())?;
                    for _ in 0..count {
                        items.push(self.value(depth + 1)?);
                    }
                    Value::Array(items)
                } else {
                    let mut members = Vec::new();
                    members
                        .try_reserve_exact(count)
                        .map_err(|_| ServiceError::limit())?;
                    let mut seen = std::collections::BTreeSet::new();
                    for _ in 0..count {
                        self.node()?;
                        let key = self.text(false)?;
                        if !seen.insert(key.clone()) {
                            return Err(ServiceError::protocol("duplicate typed JSON key"));
                        }
                        members.push((JsonString::new(key), self.value(depth + 1)?));
                    }
                    Value::Object(Object::new(members))
                }
            }
            _ => return Err(ServiceError::protocol("unknown typed JSON tag")),
        })
    }
}
