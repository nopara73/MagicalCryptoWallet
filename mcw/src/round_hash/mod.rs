//! Complete client-side WabiSabi round fingerprint calculation. This is a pure,
//! bounded service; it owns no wallet, round, credential or coordinator state.
#![forbid(unsafe_code)]
pub mod strobe;
use strobe::Strobe;

pub const OPERATION: u16 = 0x1200;
pub const VERSION: u16 = 1;
pub const MAX_PAYLOAD: usize = 1_048_560;
pub const MAX_STRING: usize = 65_536;
pub const MAX_SCRIPT_TYPES: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Truncated,
    InvalidVersion,
    InvalidUtf8,
    Limit,
    TrailingBytes,
    InvalidContinuation,
    UnsupportedOperation,
}

pub struct Parameters<'a> {
    pub input_registration_start_ms: i64,
    pub input_registration_ticks: i64,
    pub connection_confirmation_ticks: i64,
    pub output_registration_ticks: i64,
    pub transaction_signing_ticks: i64,
    pub allowed_input_amounts: [i64; 2],
    pub allowed_input_types: Vec<&'a str>,
    pub allowed_output_amounts: [i64; 2],
    pub allowed_output_types: Vec<&'a str>,
    pub network: &'a str,
    pub fee_per_k: i64,
    pub max_transaction_size: i32,
    pub min_relay_fee_per_k: i64,
    pub max_amount_credential_value: i64,
    pub max_vsize_credential_value: i64,
    pub max_vsize_allocation_per_alice: i64,
    pub max_suggested_amount: i64,
    pub coordination_identifier: &'a str,
    pub amount_cw: [u8; 33],
    pub amount_i: [u8; 33],
    pub vsize_cw: [u8; 33],
    pub vsize_i: [u8; 33],
}

struct Hasher(Strobe);
impl Hasher {
    fn new() -> Result<Self, Error> {
        let mut result = Self(Strobe::new(b"WabiSabi_v1.0"));
        // The existing string overload appends .Cw even for the domain label.
        result.append(b"domain-separator.Cw", b"round-parameters")?;
        Ok(result)
    }
    fn append(&mut self, label: &[u8], value: &[u8]) -> Result<(), Error> {
        self.0.meta_ad(label, false)?;
        self.0.meta_ad(&(value.len() as u32).to_le_bytes(), true)?;
        self.0.ad(value, false)
    }
    fn int64(&mut self, label: &[u8], value: i64) -> Result<(), Error> {
        self.append(label, &value.to_le_bytes())
    }
    fn string(&mut self, label: &str, value: &str) -> Result<(), Error> {
        if value.len() > MAX_STRING {
            return Err(Error::Limit);
        }
        self.append(format!("{label}.Cw").as_bytes(), value.as_bytes())
    }
    fn script_types(&mut self, label: &str, values: &[&str]) -> Result<(), Error> {
        if values.len() > MAX_SCRIPT_TYPES {
            return Err(Error::Limit);
        }
        for (index, name) in values.iter().enumerate() {
            self.string(&format!("{label}-{index}"), name)?;
        }
        Ok(())
    }
    fn finish(mut self) -> Result<[u8; 32], Error> {
        let mut result = [0; 32];
        self.0.prf(&mut result, false)?;
        Ok(result)
    }
}

pub fn calculate(p: &Parameters<'_>) -> Result<[u8; 32], Error> {
    // Apply the same total work bound to future direct Rust callers as to the
    // bridge, including the length prefixes in the versioned wire form.
    if p.allowed_input_types.len() > MAX_SCRIPT_TYPES
        || p.allowed_output_types.len() > MAX_SCRIPT_TYPES
    {
        return Err(Error::Limit);
    }
    let mut length = 272usize;
    for value in [p.network, p.coordination_identifier] {
        if value.len() > MAX_STRING {
            return Err(Error::Limit);
        }
        length = length.checked_add(value.len()).ok_or(Error::Limit)?;
    }
    for value in p.allowed_input_types.iter().chain(&p.allowed_output_types) {
        if value.len() > MAX_STRING {
            return Err(Error::Limit);
        }
        length = length.checked_add(4 + value.len()).ok_or(Error::Limit)?;
    }
    if length > MAX_PAYLOAD {
        return Err(Error::Limit);
    }
    let mut h = Hasher::new()?;
    h.int64(b"input-registration-start", p.input_registration_start_ms)?;
    h.int64(b"input-registration-timeout", p.input_registration_ticks)?;
    h.int64(
        b"connection-confirmation-timeout",
        p.connection_confirmation_ticks,
    )?;
    h.int64(b"output-registration-timeout", p.output_registration_ticks)?;
    h.int64(b"transaction-signing-timeout", p.transaction_signing_ticks)?;
    h.int64(b"allowed-input-amounts-min", p.allowed_input_amounts[0])?;
    h.int64(b"allowed-input-amounts-max", p.allowed_input_amounts[1])?;
    h.script_types("allowed-input-types", &p.allowed_input_types)?;
    h.int64(b"allowed-output-amounts-min", p.allowed_output_amounts[0])?;
    h.int64(b"allowed-output-amounts-max", p.allowed_output_amounts[1])?;
    h.script_types("allowed-output-types", &p.allowed_output_types)?;
    h.string("network", p.network)?;
    h.int64(b"fee-rate", p.fee_per_k)?;
    h.int64(b"coordination-fee-rate.Rate", 0)?;
    h.int64(b"coordination-fee-rate.PlebsDontPayThreshold", 0)?;
    h.append(
        b"max-transaction-size",
        &p.max_transaction_size.to_le_bytes(),
    )?;
    h.int64(b"min-relay-tx-fee", p.min_relay_fee_per_k)?;
    h.int64(
        b"maximum-amount-credential-value",
        p.max_amount_credential_value,
    )?;
    h.int64(
        b"maximum-vsize-credential-value",
        p.max_vsize_credential_value,
    )?;
    h.int64(
        b"per-alice-vsize-allocation",
        p.max_vsize_allocation_per_alice,
    )?;
    h.int64(b"maximum-suggested-amount", p.max_suggested_amount)?;
    h.string("coordination-identifier", p.coordination_identifier)?;
    h.append(b"amount-credential-issuer-parameters.Cw", &p.amount_cw)?;
    h.append(b"amount-credential-issuer-parameters.I", &p.amount_i)?;
    h.append(b"vsize-credential-issuer-parameters.Cw", &p.vsize_cw)?;
    h.append(b"vsize-credential-issuer-parameters.I", &p.vsize_i)?;
    h.finish()
}

struct Reader<'a> {
    remaining: &'a [u8],
}
impl<'a> Reader<'a> {
    fn bytes<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let result = self
            .remaining
            .get(..N)
            .ok_or(Error::Truncated)?
            .try_into()
            .map_err(|_| Error::Truncated)?;
        self.remaining = &self.remaining[N..];
        Ok(result)
    }
    fn int64(&mut self) -> Result<i64, Error> {
        Ok(i64::from_le_bytes(self.bytes()?))
    }
    fn string(&mut self) -> Result<&'a str, Error> {
        let length = u32::from_le_bytes(self.bytes()?) as usize;
        if length > MAX_STRING {
            return Err(Error::Limit);
        }
        let bytes = self.remaining.get(..length).ok_or(Error::Truncated)?;
        let result = std::str::from_utf8(bytes).map_err(|_| Error::InvalidUtf8)?;
        self.remaining = &self.remaining[length..];
        Ok(result)
    }
    fn scripts(&mut self) -> Result<Vec<&'a str>, Error> {
        let count = u16::from_le_bytes(self.bytes()?) as usize;
        if count > MAX_SCRIPT_TYPES {
            return Err(Error::Limit);
        }
        (0..count).map(|_| self.string()).collect()
    }
}

/// Bridge payload v1 has no caller-supplied transcript labels or hash commands.
/// Strings retain exact UTF-8 content and script-set iteration order. Signed
/// integers are hashed as supplied, matching the existing managed calculation.
pub fn decode(bytes: &[u8]) -> Result<Parameters<'_>, Error> {
    if bytes.len() > MAX_PAYLOAD {
        return Err(Error::Limit);
    }
    let mut r = Reader { remaining: bytes };
    if u16::from_le_bytes(r.bytes()?) != VERSION || r.bytes::<2>()? != [0, 0] {
        return Err(Error::InvalidVersion);
    }
    let result = Parameters {
        input_registration_start_ms: r.int64()?,
        input_registration_ticks: r.int64()?,
        connection_confirmation_ticks: r.int64()?,
        output_registration_ticks: r.int64()?,
        transaction_signing_ticks: r.int64()?,
        allowed_input_amounts: [r.int64()?, r.int64()?],
        allowed_input_types: r.scripts()?,
        allowed_output_amounts: [r.int64()?, r.int64()?],
        allowed_output_types: r.scripts()?,
        network: r.string()?,
        fee_per_k: r.int64()?,
        max_transaction_size: i32::from_le_bytes(r.bytes()?),
        min_relay_fee_per_k: r.int64()?,
        max_amount_credential_value: r.int64()?,
        max_vsize_credential_value: r.int64()?,
        max_vsize_allocation_per_alice: r.int64()?,
        max_suggested_amount: r.int64()?,
        coordination_identifier: r.string()?,
        amount_cw: r.bytes()?,
        amount_i: r.bytes()?,
        vsize_cw: r.bytes()?,
        vsize_i: r.bytes()?,
    };
    if !r.remaining.is_empty() {
        return Err(Error::TrailingBytes);
    }
    Ok(result)
}

pub fn handle(operation: u16, payload: &[u8]) -> Result<Vec<u8>, Error> {
    if operation != OPERATION {
        return Err(Error::UnsupportedOperation);
    }
    calculate(&decode(payload)?).map(|x| x.to_vec())
}

#[cfg(test)]
#[path = "../../tests/round_hash_strobe.inc"]
mod conformance;
