//! Real published domain/address service through the owned bounded payload API.
use mcw::bitcoin_encoding::Network;
use mcw::payment_uri::{self as uri, service as svc};

const A: &str = "18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX";
const W: &str = "bc1p0xlxvlhemja6c4dqv22uapctqupfhlxm9h8z3k2e72q4k9hcz7vqzk5jj0";

fn text(output: &mut Vec<u8>, value: &str) {
    output.extend_from_slice(&(value.len() as u32).to_le_bytes());
    output.extend_from_slice(value.as_bytes());
}
fn text_request(value: &str) -> Vec<u8> {
    let mut output = vec![1];
    text(&mut output, value);
    output
}
fn parse_request(value: &str, network: u8, mode: u8) -> Vec<u8> {
    let mut output = vec![1, network, mode];
    text(&mut output, value);
    output
}
fn decode_request(value: &str, mode: u8) -> Vec<u8> {
    let mut output = vec![1, mode];
    text(&mut output, value);
    output
}
fn format_request(
    address: &str,
    network: u8,
    flags: u8,
    amount: u64,
    label: &str,
    message: &str,
) -> Vec<u8> {
    let mut output = vec![1, network];
    text(&mut output, address);
    output.push(flags);
    if flags & 1 != 0 {
        output.extend_from_slice(&amount.to_le_bytes());
    }
    if flags & 2 != 0 {
        text(&mut output, label);
    }
    if flags & 4 != 0 {
        text(&mut output, message);
    }
    output
}

struct Reply<'a> {
    remaining: &'a [u8],
}
impl<'a> Reply<'a> {
    fn new(reply: &'a [u8]) -> Self {
        assert_eq!(reply[0], 1);
        Self {
            remaining: &reply[1..],
        }
    }
    fn bytes(&mut self, count: usize) -> &'a [u8] {
        let (result, tail) = self.remaining.split_at(count);
        self.remaining = tail;
        result
    }
    fn byte(&mut self) -> u8 {
        self.bytes(1)[0]
    }
    fn count(&mut self) -> usize {
        u32::from_le_bytes(self.bytes(4).try_into().unwrap()) as usize
    }
    fn data(&mut self) -> &'a [u8] {
        let length = self.count();
        self.bytes(length)
    }
    fn text(&mut self) -> &'a str {
        std::str::from_utf8(self.data()).unwrap()
    }
    fn number(&mut self) -> u64 {
        u64::from_le_bytes(self.bytes(8).try_into().unwrap())
    }
    fn finish(self) {
        assert!(self.remaining.is_empty());
    }
}

#[test]
fn amount_service_uses_exact_satoshis_and_full_width() {
    for (btc, sats) in [
        ("0", 0),
        (".00000001", 1),
        ("20.3", 2_030_000_000),
        ("20999999.99999999", uri::MAX_SATOSHIS - 1),
        ("21000000", uri::MAX_SATOSHIS),
    ] {
        let payload = svc::handle(svc::PARSE_AMOUNT, &text_request(btc)).unwrap();
        assert_eq!(payload.len(), 9);
        let mut reply = Reply::new(&payload);
        assert_eq!(reply.number(), sats);
        reply.finish();
        let mut request = vec![1];
        request.extend_from_slice(&sats.to_le_bytes());
        let payload = svc::handle(svc::FORMAT_AMOUNT, &request).unwrap();
        let mut reply = Reply::new(&payload);
        assert_eq!(
            reply.text(),
            uri::Amount::from_satoshis(sats).unwrap().to_btc()
        );
        reply.finish();
    }
    for value in [
        "-0.01",
        "1e8",
        "1,000",
        "0.000000001",
        "21000000.00000001",
        "+1",
    ] {
        assert_eq!(
            svc::handle(svc::PARSE_AMOUNT, &text_request(value))
                .unwrap_err()
                .code,
            8
        );
    }
    assert_eq!(
        svc::handle(svc::PARSE_AMOUNT, &text_request(""))
            .unwrap_err()
            .code,
        7
    );
    let mut request = vec![1];
    request.extend_from_slice(&u64::MAX.to_le_bytes());
    assert_eq!(
        svc::handle(svc::FORMAT_AMOUNT, &request).unwrap_err().code,
        8
    );
}

#[test]
fn metadata_service_preserves_utf8_and_distinguishes_form_mode() {
    let value = "Árvíztűrő 東京 🦀 & = + ? # % /";
    let payload = svc::handle(svc::ENCODE_METADATA, &text_request(value)).unwrap();
    let mut reply = Reply::new(&payload);
    let encoded = reply.text().to_owned();
    reply.finish();
    assert_eq!(encoded, uri::percent_encode(value).unwrap());
    let payload = svc::handle(svc::DECODE_METADATA, &decode_request(&encoded, 0)).unwrap();
    let mut reply = Reply::new(&payload);
    assert_eq!(reply.text(), value);
    reply.finish();
    for (mode, expected) in [(0, "A++B"), (1, "A +B")] {
        let payload = svc::handle(svc::DECODE_METADATA, &decode_request("A+%2bB", mode)).unwrap();
        let mut reply = Reply::new(&payload);
        assert_eq!(reply.text(), expected);
        reply.finish();
    }
    for encoded in ["%", "%GG", "%FF", "%ED%A0%80", "%C0%AF"] {
        assert_eq!(
            svc::handle(svc::DECODE_METADATA, &decode_request(encoded, 0))
                .unwrap_err()
                .code,
            5
        );
    }
}

#[test]
fn parsed_request_returns_real_descriptor_and_original_case_content() {
    let uppercase = W.to_ascii_uppercase();
    let original = format!(
        "BITCOIN:{uppercase}?Amount=0.00000001&Label=MiXeD+%2bCase&Message=%2541&pj=https%3A%2F%2Fexample.invalid&flag&empty="
    );
    let payload = svc::handle(svc::PARSE_DESTINATION, &parse_request(&original, 0, 1)).unwrap();
    let mut reply = Reply::new(&payload);
    assert_eq!(reply.byte(), 1); // URI, not bare address.
    assert_eq!(reply.byte(), 0); // Explicit mainnet.
    assert_eq!(reply.byte(), 2);
    assert_eq!(reply.byte(), 1); // Witness v1.
    assert_eq!(
        reply.data(),
        mcw::bitcoin_encoding::hex_decode(
            "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798"
        )
        .unwrap()
    );
    assert_eq!(reply.text(), uppercase);
    assert_eq!(reply.text(), W);
    assert_eq!(reply.byte(), 7);
    assert_eq!(reply.number(), 1);
    assert_eq!(reply.text(), "MiXeD +Case");
    assert_eq!(reply.text(), "%41");
    assert_eq!(reply.byte(), 1);
    assert_eq!(reply.text(), original);
    assert_eq!(reply.count(), 3);
    assert_eq!(reply.text(), "pj");
    assert_eq!(reply.byte(), 1);
    assert_eq!(reply.text(), "https://example.invalid");
    assert_eq!(reply.text(), "flag");
    assert_eq!(reply.byte(), 0);
    assert_eq!(reply.text(), "empty");
    assert_eq!(reply.byte(), 1);
    assert_eq!(reply.text(), "");
    reply.finish();
}

#[test]
fn bare_address_and_explicit_network_ids_use_actual_validator() {
    for (network, text, kind) in [
        (0, A, 0),
        (0, "3EktnHQD7RiAE6uzMj2ZifT9YgRrkSgzQX", 1),
        (1, "mipcBbFg9gMiCh81Kj8tqqdgoZub1ZJRfn", 0),
        (2, "mipcBbFg9gMiCh81Kj8tqqdgoZub1ZJRfn", 0),
        (3, "mipcBbFg9gMiCh81Kj8tqqdgoZub1ZJRfn", 0),
        (4, "mipcBbFg9gMiCh81Kj8tqqdgoZub1ZJRfn", 0),
        (1, "2MzQwSSnBHWHqSAqtTVQ6v47XtaisrJa1Vc", 1),
        (2, "2MzQwSSnBHWHqSAqtTVQ6v47XtaisrJa1Vc", 1),
        (3, "2MzQwSSnBHWHqSAqtTVQ6v47XtaisrJa1Vc", 1),
        (4, "2MzQwSSnBHWHqSAqtTVQ6v47XtaisrJa1Vc", 1),
    ] {
        let payload = svc::handle(
            svc::PARSE_DESTINATION,
            &parse_request(&format!("  {text}  "), network, 1),
        )
        .unwrap();
        let mut reply = Reply::new(&payload);
        assert_eq!(reply.byte(), 0);
        assert_eq!(reply.byte(), network);
        assert_eq!(reply.byte(), kind);
        assert_eq!(reply.byte(), 255);
        assert_eq!(reply.data().len(), 20);
        assert_eq!(reply.text(), text);
        assert_eq!(reply.text(), text);
        assert_eq!(reply.byte(), 0);
        assert_eq!(reply.byte(), 0);
        assert_eq!(reply.count(), 0);
        reply.finish();
    }
    for network in 1..=4 {
        assert_eq!(
            svc::handle(svc::PARSE_DESTINATION, &parse_request(A, network, 0))
                .unwrap_err()
                .code,
            4
        );
    }
    let bcrt =
        mcw::bitcoin_encoding::witness_address_encode(Network::Regtest, 0, &[0x21; 20]).unwrap();
    let payload = svc::handle(svc::PARSE_DESTINATION, &parse_request(&bcrt, 4, 0)).unwrap();
    let mut reply = Reply::new(&payload);
    assert_eq!(reply.byte(), 0);
    assert_eq!(reply.byte(), 4);
    assert_eq!(reply.byte(), 2);
    assert_eq!(reply.byte(), 0);
    assert_eq!(reply.data(), &[0x21; 20]);
    assert_eq!(reply.text(), bcrt);
    assert_eq!(reply.text(), bcrt);
    assert_eq!(reply.byte(), 0);
    assert_eq!(reply.byte(), 0);
    assert_eq!(reply.count(), 0);
    reply.finish();
    assert_eq!(
        svc::handle(svc::PARSE_DESTINATION, &parse_request(&bcrt, 1, 0))
            .unwrap_err()
            .code,
        4
    );
}

#[test]
fn format_service_uses_checked_destination_and_metadata_only() {
    let payload = svc::handle(
        svc::FORMAT_REQUEST,
        &format_request(A, 0, 7, 0, "東京 &+", ""),
    )
    .unwrap();
    let mut reply = Reply::new(&payload);
    assert_eq!(
        reply.text(),
        format!("bitcoin:{A}?amount=0&label=%E6%9D%B1%E4%BA%AC%20%26%2B&message=")
    );
    reply.finish();
    let payload = svc::handle(svc::FORMAT_REQUEST, &format_request(A, 0, 0, 0, "", "")).unwrap();
    let mut reply = Reply::new(&payload);
    assert_eq!(reply.text(), format!("bitcoin:{A}"));
    reply.finish();
    assert_eq!(
        svc::handle(
            svc::FORMAT_REQUEST,
            &format_request("badAddress", 0, 0, 0, "", "")
        )
        .unwrap_err()
        .code,
        4
    );
    assert_eq!(
        svc::handle(svc::FORMAT_REQUEST, &format_request(A, 1, 0, 0, "", ""))
            .unwrap_err()
            .code,
        4
    );
    assert_eq!(
        svc::handle(
            svc::FORMAT_REQUEST,
            &format_request(A, 0, 1, uri::MAX_SATOSHIS + 1, "", "")
        )
        .unwrap_err()
        .code,
        8
    );
    assert_eq!(
        svc::handle(svc::FORMAT_REQUEST, &format_request(A, 0, 8, 0, "", ""))
            .unwrap_err()
            .code,
        106
    );
    assert_eq!(
        svc::handle(
            svc::FORMAT_REQUEST,
            &format_request(A, 0, 2, 0, &"東京".repeat(200), "")
        )
        .unwrap_err()
        .code,
        10
    );
}

#[test]
fn compatible_parse_is_not_invalidated_by_an_unrequested_format() {
    // Formatting is a separate operation. A valid raw-compatible URI may exceed
    // the URI length cap only after canonical percent encoding expands it.
    let original = format!("bitcoin:{A}?label={}", "東京".repeat(200));
    assert!(svc::handle(svc::PARSE_DESTINATION, &parse_request(&original, 0, 1)).is_ok());
    assert_eq!(
        svc::handle(svc::PARSE_DESTINATION, &parse_request(&original, 0, 0))
            .unwrap_err()
            .code,
        5
    );
}

#[test]
fn domain_errors_are_preserved_and_never_embed_supplied_metadata() {
    for (suffix, code) in [
        ("?req-privateLabel=SensitiveMarker", 9),
        ("?amount=1&Amount=2", 6),
        ("?amount=0.000000001", 8),
        ("?amount=", 7),
        ("?label=%FF", 5),
    ] {
        let error = svc::handle(
            svc::PARSE_DESTINATION,
            &parse_request(&format!("bitcoin:{A}{suffix}"), 0, 1),
        )
        .unwrap_err();
        assert_eq!(error.code, code);
        assert!(!format!("{error:?} {error}").contains("SensitiveMarker"));
        assert!(!format!("{error:?} {error}").contains("privateLabel"));
    }
    assert_eq!(
        svc::handle(svc::PARSE_DESTINATION, &parse_request(" ", 0, 1))
            .unwrap_err()
            .code,
        11
    );
    assert_eq!(
        svc::handle(
            svc::PARSE_DESTINATION,
            &parse_request("bitcoin:?sp=x", 0, 1)
        )
        .unwrap_err()
        .code,
        3
    );
    assert_eq!(
        svc::handle(
            svc::PARSE_DESTINATION,
            &parse_request(&"x".repeat(1_001), 0, 1)
        )
        .unwrap_err()
        .code,
        10
    );
}

#[test]
fn malformed_payloads_and_unknown_ids_fail_closed() {
    assert!(!svc::handles(0x03ff));
    assert!(!svc::handles(0x0406));
    assert_eq!(svc::handle(0x0406, &[]).unwrap_err().code, 100);
    for operation in svc::PARSE_DESTINATION..=svc::DECODE_METADATA {
        assert!(svc::handles(operation));
        assert_eq!(svc::handle(operation, &[]).unwrap_err().code, 101);
        assert_eq!(svc::handle(operation, &[2]).unwrap_err().code, 102);
        assert_eq!(
            svc::handle(operation, &vec![0; svc::MAX_PAYLOAD_BYTES + 1])
                .unwrap_err()
                .code,
            107
        );
    }
    assert_eq!(
        svc::handle(svc::PARSE_DESTINATION, &parse_request(A, 5, 0))
            .unwrap_err()
            .code,
        103
    );
    assert_eq!(
        svc::handle(svc::PARSE_DESTINATION, &parse_request(A, 0, 2))
            .unwrap_err()
            .code,
        104
    );
    assert_eq!(
        svc::handle(svc::DECODE_METADATA, &decode_request("a", 2))
            .unwrap_err()
            .code,
        104
    );
    for (operation, request) in [
        (svc::PARSE_DESTINATION, parse_request(A, 0, 1)),
        (
            svc::FORMAT_REQUEST,
            format_request(A, 0, 7, 1, "label", "message"),
        ),
        (svc::PARSE_AMOUNT, text_request("1")),
        (svc::FORMAT_AMOUNT, vec![1, 1, 0, 0, 0, 0, 0, 0, 0]),
        (svc::ENCODE_METADATA, text_request("label")),
        (svc::DECODE_METADATA, decode_request("label", 0)),
    ] {
        for length in 0..request.len() {
            assert!(
                svc::handle(operation, &request[..length]).is_err(),
                "{operation:#x} length {length}"
            );
        }
        let mut trailing = request;
        trailing.push(0);
        assert_eq!(svc::handle(operation, &trailing).unwrap_err().code, 101);
    }
    let mut invalid = vec![1];
    invalid.extend_from_slice(&1_u32.to_le_bytes());
    invalid.push(255);
    assert_eq!(
        svc::handle(svc::ENCODE_METADATA, &invalid)
            .unwrap_err()
            .code,
        105
    );
    let mut oversized = vec![1];
    oversized.extend_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        svc::handle(svc::ENCODE_METADATA, &oversized)
            .unwrap_err()
            .code,
        107
    );
}

#[test]
fn exact_component_capacity_and_encoded_output_capacity_are_supported() {
    let value = " ".repeat(uri::MAX_COMPONENT_BYTES);
    let payload = svc::handle(svc::ENCODE_METADATA, &text_request(&value)).unwrap();
    let mut reply = Reply::new(&payload);
    let encoded = reply.text().to_owned();
    reply.finish();
    assert_eq!(encoded.len(), uri::MAX_COMPONENT_BYTES * 3);
    let payload = svc::handle(svc::DECODE_METADATA, &decode_request(&encoded, 0)).unwrap();
    let mut reply = Reply::new(&payload);
    assert_eq!(reply.text(), value);
    reply.finish();
    assert_eq!(
        svc::handle(svc::ENCODE_METADATA, &text_request(&format!("{value}x")))
            .unwrap_err()
            .code,
        107
    );
    assert_eq!(
        svc::handle(
            svc::DECODE_METADATA,
            &decode_request(&format!("{encoded}%20"), 0)
        )
        .unwrap_err()
        .code,
        107
    );
}

#[test]
fn deterministic_payload_mutations_do_not_panic() {
    let mut state = 0x0400_c0de_1234_5678_u64;
    for operation in svc::PARSE_DESTINATION..=svc::DECODE_METADATA {
        for length in 0..128 {
            let mut payload = Vec::new();
            for _ in 0..length {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                payload.push((state >> 32) as u8);
            }
            let _ = svc::handle(operation, &payload);
            if let Some(version) = payload.first_mut() {
                *version = 1;
                let _ = svc::handle(operation, &payload);
            }
        }
    }
}
