#![allow(dead_code)]
#[path = "../src/json.rs"]
mod json;
#[path = "../src/serialization_service/mod.rs"]
mod serialization_service;

use json::{Number, Value};
use serialization_service::{
    APPEND, Action, CLOSE, FINISH, MAX_JSON, MAX_READ, NUMERIC, OPEN, READ, SerializationService,
    tokens, transform,
};

fn transfer(service: &mut SerializationService, action: u8, input: &[u8]) -> Vec<u8> {
    let mut open = vec![action];
    open.extend_from_slice(&(input.len() as u32).to_le_bytes());
    let id = service.handle(OPEN, &open).unwrap();
    let mut append = id.clone();
    append.extend_from_slice(&0u32.to_le_bytes());
    append.extend_from_slice(input);
    service.handle(APPEND, &append).unwrap();
    let length = service.handle(FINISH, &id).unwrap();
    let length = u32::from_le_bytes(length.try_into().unwrap());
    let mut result = Vec::new();
    while result.len() < length as usize {
        let mut read = id.clone();
        read.extend_from_slice(&(result.len() as u32).to_le_bytes());
        read.extend_from_slice(&(MAX_READ as u32).to_le_bytes());
        result.extend(service.handle(READ, &read).unwrap());
    }
    result
}
fn convert(mode: u8, value: Value) -> Vec<u8> {
    let mut payload = vec![mode];
    payload.extend(tokens::encode(&value).unwrap());
    SerializationService::default()
        .handle(NUMERIC, &payload)
        .unwrap()
}
#[test]
fn legacy_constructor_fields_keep_last_occurrence_and_numeric_spelling() {
    let result = transform(
        Action::ParseRpc,
        br#"{"method":true,"id":1.2300,"ID":9007199254740993}"#,
    )
    .unwrap();
    let result = tokens::decode(&result).unwrap();
    let request = &result.as_array().unwrap()[1].as_array().unwrap()[0];
    let request = request.as_object().unwrap();
    assert_eq!(
        request.get_unique("id").unwrap().unwrap().as_str(),
        Some("9007199254740993")
    );
    assert_eq!(
        request.get_unique("method").unwrap().unwrap().as_str(),
        Some("true")
    );
}
#[test]
fn parameter_duplicates_keep_last_value_without_changing_order() {
    let result = transform(
        Action::ParseRpc,
        br#"{"method":"x","params":{"a":1,"b":2,"a":3}}"#,
    )
    .unwrap();
    let result = tokens::decode(&result).unwrap();
    let request = &result.as_array().unwrap()[1].as_array().unwrap()[0];
    let params = request
        .as_object()
        .unwrap()
        .get_unique("params")
        .unwrap()
        .unwrap()
        .as_object()
        .unwrap();
    assert_eq!(params.members().len(), 2);
    assert_eq!(params.members()[0].0.as_str(), "a");
    assert_eq!(params.members()[0].1.as_number().unwrap().as_str(), "3");
}
#[test]
fn lexical_extensions_do_not_coerce_array_values_or_quoted_content() {
    let input = br#"/*c*/{method:'x',id:1,params:[false,true,null,'say "hi"','it\'s',],}"#;
    let result = tokens::decode(&transform(Action::ParseRpc, input).unwrap()).unwrap();
    let request = &result.as_array().unwrap()[1].as_array().unwrap()[0];
    let params = request
        .as_object()
        .unwrap()
        .get_unique("params")
        .unwrap()
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(params[0], Value::Bool(false));
    assert_eq!(params[1], Value::Bool(true));
    assert_eq!(params[2], Value::Null);
    assert_eq!(params[3].as_str(), Some("say \"hi\""));
    assert_eq!(params[4].as_str(), Some("it's"));
}
#[test]
fn malformed_roots_and_structured_ids_fail_without_panicking() {
    for input in [
        b"null".as_slice(),
        b"[null]",
        b"42",
        b"{}",
        br#"{"method":"x","id":{}}"#,
        b"{method:'x}",
    ] {
        assert!(transform(Action::ParseRpc, input).is_err());
    }
}
#[test]
fn strings_are_bounded_and_invalid_utf8_is_rejected() {
    assert!(transform(Action::ParseRpc, b"{\"method\":\"\xff\"}").is_err());
    assert!(transform(Action::ParseRpc, &vec![b' '; MAX_JSON + 1]).is_err());
}
#[test]
fn numeric_tokens_do_not_pass_through_float_for_integer_contracts() {
    let bytes = convert(0, Value::Number(Number::from(9_007_199_254_740_993i64)));
    assert_eq!(bytes[0], 0);
    assert_eq!(
        i128::from_le_bytes(bytes[1..].try_into().unwrap()),
        9_007_199_254_740_993
    );
    let bytes = convert(0, Value::Number(Number::from(i64::MAX)));
    assert_eq!(
        i128::from_le_bytes(bytes[1..].try_into().unwrap()),
        i128::from(i64::MAX)
    );
}
#[test]
fn float_integer_coercion_retains_ties_to_even_and_numeric_strings() {
    for (text, expected) in [("1.5", 2), ("2.5", 2), ("-1.5", -2), ("1e3", 1000)] {
        let bytes = convert(0, Value::Number(Number::parse(text).unwrap()));
        assert_eq!(
            i128::from_le_bytes(bytes[1..].try_into().unwrap()),
            expected
        );
    }
    assert_eq!(convert(0, Value::string("1.0")), [1]);
    assert_eq!(convert(0, Value::Bool(true)), [1]);
    let bytes = convert(0, Value::string(" +0042 "));
    assert_eq!(i128::from_le_bytes(bytes[1..].try_into().unwrap()), 42);
}
#[test]
fn decimal_string_scale_and_float_precision_keep_rpc_compatibility() {
    let bytes = convert(1, Value::string("1.2300"));
    assert_eq!(u32::from_le_bytes(bytes[1..5].try_into().unwrap()), 12300);
    assert_eq!(
        u32::from_le_bytes(bytes[13..17].try_into().unwrap()),
        4 << 16
    );
    let bytes = convert(
        1,
        Value::Number(Number::parse("1.234567890123456789").unwrap()),
    );
    assert_eq!(
        u64::from_le_bytes(bytes[1..9].try_into().unwrap()),
        123456789012346
    );
    assert_eq!(
        u32::from_le_bytes(bytes[13..17].try_into().unwrap()),
        14 << 16
    );
    assert_eq!(convert(1, Value::Bool(true)), [1]);
}
#[test]
fn boolean_coercion_is_rpc_specific() {
    assert_eq!(convert(2, Value::string("true"))[1], 1);
    assert_eq!(
        convert(2, Value::Number(Number::parse("0.1").unwrap()))[1],
        1
    );
    assert_eq!(convert(2, Value::Number(Number::from(0i64)))[1], 0);
    assert_eq!(convert(2, Value::string("invalid")), [1]);
}
#[test]
fn response_order_ids_domain_numbers_and_unicode_are_native() {
    let input = Value::Array(vec![
        Value::string("1e3"),
        Value::Number(Number::from(9_007_199_254_740_993i64)),
        Value::Null,
        Value::Null,
    ]);
    let result = transform(Action::WriteRpcResponse, &tokens::encode(&input).unwrap()).unwrap();
    assert_eq!(
        result,
        br#"{"jsonrpc":"2.0","result":9007199254740993,"id":"1e3"}"#
    );
    let error = Value::Array(vec![
        Value::Null,
        Value::Null,
        Value::Number(Number::from(-32700i64)),
        Value::string("Parse error"),
    ]);
    assert_eq!(
        transform(Action::WriteRpcResponse, &tokens::encode(&error).unwrap()).unwrap(),
        br#"{"jsonrpc":"2.0","error":{"code":-32700,"message":"Parse error"},"id":null}"#
    );
}
#[test]
fn completed_transfers_are_removed_and_close_is_idempotent() {
    let mut service = SerializationService::default();
    for _ in 0..20 {
        tokens::decode(&transfer(&mut service, 0, br#"{"method":"x"}"#)).unwrap();
    }
    assert!(service.handle(READ, &[0; 16]).is_err());
    assert_eq!(service.handle(CLOSE, &[0; 8]).unwrap(), Vec::<u8>::new());
}
#[test]
fn invalid_transfer_offsets_and_failed_finishes_release_resources() {
    let mut service = SerializationService::default();
    let id = service.handle(OPEN, &[0, 1, 0, 0, 0]).unwrap();
    let mut append = id.clone();
    append.extend_from_slice(&1u32.to_le_bytes());
    append.push(b'{');
    assert!(service.handle(APPEND, &append).is_err());
    assert!(service.handle(FINISH, &id).is_err());
    assert!(service.handle(FINISH, &id).is_err());
    transfer(&mut service, 0, br#"{"method":"x"}"#);
}
#[test]
fn transfer_count_and_frame_sizes_are_bounded() {
    let mut service = SerializationService::default();
    let mut ids = Vec::new();
    for _ in 0..8 {
        ids.push(service.handle(OPEN, &[0, 0, 0, 0, 0]).unwrap());
    }
    assert_eq!(service.handle(OPEN, &[0, 0, 0, 0, 0]).unwrap_err().code, 12);
    for id in ids {
        service.handle(CLOSE, &id).unwrap();
    }
    assert!(service.handle(OPEN, &[3, 0, 0, 0, 0]).is_err());
    assert!(service.handle(NUMERIC, &vec![0; 16385]).is_err());
    assert!(tokens::decode(&[5, 255, 255, 255, 255]).is_err());
}
