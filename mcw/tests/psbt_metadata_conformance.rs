//! Synthetic retained-implementation comparisons and bounded-transfer failures.
#[path = "../src/psbt_metadata.rs"]
pub mod psbt_metadata;
#[path = "../src/psbt_metadata_service.rs"]
pub mod psbt_metadata_service;
pub use mcw::{bitcoin_encoding, bitcoin_script, bitcoin_wire, psbt, wallet_hashes};
use mcw::{
    bitcoin_wire::{self as wire, Transaction},
    psbt::{Limits, Map, Psbt, Record},
};
use psbt_metadata::{self as metadata, PreviousTransaction};
use psbt_metadata_service::{self as service, Transfers};
use std::collections::BTreeMap;

// Fixture transport only: the application service consumes binary packets.
// This avoids imposing the separate address/hex codec's 1 MiB input policy on
// the explicitly larger parent-transaction reference fixture.
fn hex_decode(text: &str) -> Result<Vec<u8>, &'static str> {
    if !text.len().is_multiple_of(2) || text.len() > 64 * 1024 * 1024 {
        return Err("invalid fixture hex");
    }
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digit = |byte: u8| {
                (byte as char)
                    .to_digit(16)
                    .map(|value| value as u8)
                    .ok_or("invalid fixture hex")
            };
            Ok(digit(pair[0])? << 4 | digit(pair[1])?)
        })
        .collect()
}

fn packet_from_request(request: &[u8]) -> Psbt {
    let length = u32::from_le_bytes(request[2..6].try_into().unwrap()) as usize;
    Psbt::parse(&request[6..6 + length], Limits::default()).unwrap()
}
fn result_packet(response: &[u8]) -> Psbt {
    assert_eq!(&response[..2], &1u16.to_le_bytes());
    let length = u32::from_le_bytes(response[2..6].try_into().unwrap()) as usize;
    assert_eq!(response.len(), length + 6);
    Psbt::parse(&response[6..], Limits::default()).unwrap()
}
fn records(map: &Map) -> BTreeMap<Vec<u8>, Vec<u8>> {
    map.records()
        .iter()
        .map(|record| (record.key().to_vec(), record.value().to_vec()))
        .collect()
}
fn compare_case(line: &str) {
    let columns: Vec<_> = line.split('\t').collect();
    let request = hex_decode(columns[1]).unwrap();
    let original = packet_from_request(&request);
    let expected = Psbt::parse(&hex_decode(columns[2]).unwrap(), Limits::default()).unwrap();
    let actual = result_packet(&service::handle(service::ENRICH, &request).unwrap());
    assert_eq!(
        records(actual.global()),
        records(expected.global()),
        "{} global",
        columns[0]
    );
    for (index, (actual, expected)) in actual.inputs().iter().zip(expected.inputs()).enumerate() {
        assert_eq!(
            records(actual),
            records(expected),
            "{} input {index}",
            columns[0]
        );
    }
    for (index, (actual, expected)) in actual.outputs().iter().zip(expected.outputs()).enumerate() {
        assert_eq!(
            records(actual),
            records(expected),
            "{} output {index}",
            columns[0]
        );
    }
    assert_eq!(
        actual.unsigned_transaction().unwrap(),
        original.unsigned_transaction().unwrap()
    );
    assert_eq!(actual.inputs().len(), expected.inputs().len());
    assert_eq!(actual.outputs().len(), expected.outputs().len());
    // Every unrelated key/value and its relative order survives the native edit.
    for (source, target) in original
        .inputs()
        .iter()
        .zip(actual.inputs())
        .chain(original.outputs().iter().zip(actual.outputs()))
    {
        let unknown = |map: &Map| {
            map.records()
                .iter()
                .filter(|record| record.key_type() >= 0x80)
                .cloned()
                .collect::<Vec<_>>()
        };
        assert_eq!(unknown(source), unknown(target));
    }
}
#[test]
fn retained_metadata_results_match_all_synthetic_reference_cases() {
    let mut count = 0;
    for line in include_str!("psbt_metadata_vectors.tsv")
        .lines()
        .filter(|line| !line.starts_with('#'))
    {
        compare_case(line);
        count += 1;
    }
    assert_eq!(count, 13);
    if let Ok(path) = std::env::var("MCW_PSBT_METADATA_REFERENCE") {
        let generated = std::fs::read_to_string(path).unwrap();
        let cases = generated
            .lines()
            .filter(|line| !line.starts_with('#'))
            .collect::<Vec<_>>();
        assert_eq!(cases.len(), 14);
        for line in cases {
            compare_case(line);
        }
    }
}
fn version() -> Vec<u8> {
    1u16.to_le_bytes().to_vec()
}
fn number(bytes: &mut Vec<u8>, number: usize) {
    bytes.extend_from_slice(&(number as u32).to_le_bytes());
}
fn session_payload(id: u64) -> Vec<u8> {
    let mut bytes = version();
    bytes.extend_from_slice(&id.to_le_bytes());
    bytes
}
fn begin(transfers: &mut Transfers, request_id: u64, operation: u16, length: usize) -> u64 {
    let mut request = version();
    request.extend_from_slice(&operation.to_le_bytes());
    number(&mut request, length);
    let result = transfers
        .handle(request_id, service::BEGIN, &request)
        .unwrap();
    assert_eq!(result.len(), 10);
    u64::from_le_bytes(result[2..].try_into().unwrap())
}
fn append(
    transfers: &mut Transfers,
    request_id: u64,
    id: u64,
    offset: usize,
    bytes: &[u8],
) -> Result<Vec<u8>, service::Error> {
    let mut request = session_payload(id);
    number(&mut request, offset);
    number(&mut request, bytes.len());
    request.extend_from_slice(bytes);
    transfers.handle(request_id, service::APPEND, &request)
}
fn through_transfer(request: &[u8]) -> Vec<u8> {
    let mut transfers = Transfers::default();
    let mut request_id = 1;
    let id = begin(&mut transfers, request_id, service::ENRICH, request.len());
    for (index, chunk) in request.chunks(service::CHUNK).enumerate() {
        request_id += 1;
        let ack = append(
            &mut transfers,
            request_id,
            id,
            index * service::CHUNK,
            chunk,
        )
        .unwrap();
        assert_eq!(
            u32::from_le_bytes(ack[10..14].try_into().unwrap()) as usize,
            index * service::CHUNK + chunk.len()
        );
        assert!(ack.len() < 1_048_576 - 16);
    }
    request_id += 1;
    let response = transfers
        .handle(request_id, service::COMMIT, &session_payload(id))
        .unwrap();
    let length = u32::from_le_bytes(response[10..14].try_into().unwrap()) as usize;
    let mut result = Vec::new();
    while result.len() < length {
        let offset = result.len();
        let count = (length - offset).min(service::CHUNK);
        let mut read = session_payload(id);
        number(&mut read, offset);
        number(&mut read, count);
        request_id += 1;
        let response = transfers.handle(request_id, service::READ, &read).unwrap();
        assert_eq!(
            u32::from_le_bytes(response[10..14].try_into().unwrap()) as usize,
            offset
        );
        assert_eq!(
            u32::from_le_bytes(response[14..18].try_into().unwrap()) as usize,
            count
        );
        assert!(response.len() < 1_048_576 - 16);
        result.extend_from_slice(&response[18..]);
    }
    assert_eq!(transfers.active_sessions(), 0);
    result
}
#[test]
fn complete_transfer_preserves_packet_across_chunks_above_frame_limit() {
    let line = include_str!("psbt_metadata_vectors.tsv")
        .lines()
        .find(|line| line.starts_with("standard\t"))
        .unwrap();
    let request = hex_decode(line.split('\t').nth(1).unwrap()).unwrap();
    assert_eq!(
        through_transfer(&request),
        service::handle(service::ENRICH, &request).unwrap()
    );
    if let Ok(path) = std::env::var("MCW_PSBT_METADATA_REFERENCE") {
        let generated = std::fs::read_to_string(path).unwrap();
        let large = generated
            .lines()
            .find(|line| line.starts_with("large-parent\t"))
            .unwrap();
        let request = hex_decode(large.split('\t').nth(1).unwrap()).unwrap();
        assert!(request.len() > 1_048_576);
        assert_eq!(
            through_transfer(&request),
            service::handle(service::ENRICH, &request).unwrap()
        );
    }
}
#[test]
fn transfer_rejects_offsets_truncation_replays_and_cleans_cancellation() {
    let mut transfers = Transfers::default();
    let id = begin(&mut transfers, 1, service::ENRICH, 10);
    assert!(append(&mut transfers, 2, id, 1, &[1]).is_err());
    assert_eq!(transfers.active_sessions(), 0);
    let id = begin(&mut transfers, 3, service::ENRICH, 10);
    append(&mut transfers, 4, id, 0, &[1, 2]).unwrap();
    assert!(
        transfers
            .handle(5, service::COMMIT, &session_payload(id))
            .is_err()
    );
    assert_eq!(transfers.active_sessions(), 0);
    let id = begin(&mut transfers, 6, service::ENRICH, 10);
    append(&mut transfers, 7, id, 0, &[1]).unwrap();
    transfers.cancel(6);
    assert_eq!(transfers.active_sessions(), 1);
    transfers.cancel(7);
    assert_eq!(transfers.active_sessions(), 0);
    assert!(append(&mut transfers, 8, id, 1, &[2]).is_err());
    let id = begin(&mut transfers, 9, service::ENRICH, 10);
    transfers
        .handle(10, service::ABORT, &session_payload(id))
        .unwrap();
    transfers
        .handle(11, service::ABORT, &session_payload(id))
        .unwrap();
    assert_eq!(transfers.active_sessions(), 0);
    for request_id in 12..16 {
        begin(&mut transfers, request_id, service::INSPECT, 6);
    }
    let mut request = version();
    request.extend_from_slice(&service::INSPECT.to_le_bytes());
    number(&mut request, 6);
    assert_eq!(
        transfers.handle(16, service::BEGIN, &request),
        Err(service::Error::ResourceLimit)
    );
    let mut isolated = Transfers::default();
    begin(&mut isolated, 20, service::INSPECT, service::MAX_REQUEST);
    begin(&mut isolated, 21, service::INSPECT, service::MAX_REQUEST);
    assert_eq!(
        isolated.handle(22, service::BEGIN, &request),
        Err(service::Error::ResourceLimit)
    );
    // EOF destroys this per-connection state, including incomplete uploads.
    drop(isolated);
}
#[test]
fn untrusted_metadata_lengths_and_headers_fail_before_packet_edits() {
    let line = include_str!("psbt_metadata_vectors.tsv")
        .lines()
        .find(|line| line.starts_with("standard\t"))
        .unwrap();
    let request = hex_decode(line.split('\t').nth(1).unwrap()).unwrap();
    for length in [0, 1, 2, 5, request.len() - 1] {
        assert!(service::handle(service::ENRICH, &request[..length]).is_err());
    }
    let mut altered = request.clone();
    altered[0] = 2;
    assert!(service::handle(service::ENRICH, &altered).is_err());
    let mut altered = request.clone();
    altered.push(0);
    assert!(service::handle(service::ENRICH, &altered).is_err());
    let mut altered = request;
    altered[2..6].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        service::handle(service::ENRICH, &altered),
        Err(service::Error::ResourceLimit)
    );
}
#[test]
fn parents_are_verified_and_failed_edits_leave_source_unchanged() {
    let line = include_str!("psbt_metadata_vectors.tsv")
        .lines()
        .find(|line| line.starts_with("standard\t"))
        .unwrap();
    let request = hex_decode(line.split('\t').nth(1).unwrap()).unwrap();
    let source = packet_from_request(&request);
    let before = source.serialize().unwrap();
    let enriched = result_packet(&service::handle(service::ENRICH, &request).unwrap());
    let bytes = enriched.inputs()[0].singleton(0).unwrap().to_vec();
    let tx = Transaction::decode(&bytes, &wire::Limits::default()).unwrap();
    let parent = PreviousTransaction {
        txid: tx.txid(&wire::Limits::default()).unwrap().0,
        bytes,
    };
    let mut wrong = parent.clone();
    wrong.txid[0] ^= 1;
    assert!(metadata::enrich(&source, &[], &[wrong], true).is_err());
    let mut inputs = source.inputs().to_vec();
    let mut output =
        wire::decode_output(inputs[0].singleton(1).unwrap(), &wire::Limits::default()).unwrap();
    output.value += 1;
    inputs[0] = inputs[0].with_record(
        Record::new(
            1,
            &[],
            &wire::serialize_output(&output, &wire::Limits::default()).unwrap(),
        )
        .unwrap(),
    );
    let inconsistent = Psbt::from_maps(
        source.global().clone(),
        inputs,
        source.outputs().to_vec(),
        Limits::default(),
    )
    .unwrap();
    assert!(metadata::enrich(&inconsistent, &[], &[parent], true).is_err());
    assert_eq!(source.serialize().unwrap(), before);
}
