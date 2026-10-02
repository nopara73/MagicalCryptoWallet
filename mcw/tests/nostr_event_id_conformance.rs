#[path = "../src/bitcoin_encoding.rs"]
pub mod bitcoin_encoding;
#[path = "../src/json.rs"]
pub mod json;
#[path = "../src/nostr_event_id.rs"]
pub mod nostr_event_id;

use nostr_event_id::{Error, MAX_REQUEST_BYTES, canonical_preimage, digest};

const KEY: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";

fn text(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(&(value.len() as u32).to_le_bytes());
    out.extend_from_slice(value.as_bytes());
}

fn request(timestamp: i64, kind: i32, tags: &[&[&str]], content: &str) -> Vec<u8> {
    let mut out = vec![1];
    out.extend_from_slice(&bitcoin_encoding::hex_decode(KEY).unwrap());
    out.extend_from_slice(&timestamp.to_le_bytes());
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(&(tags.len() as u32).to_le_bytes());
    for tag in tags {
        out.extend_from_slice(&(tag.len() as u32).to_le_bytes());
        for item in *tag {
            text(&mut out, item);
        }
    }
    text(&mut out, content);
    out
}

#[test]
fn independent_nip01_empty_vector() {
    let payload = request(0, 1, &[], "");
    assert_eq!(
        canonical_preimage(&payload).unwrap(),
        "[0,\"79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798\",0,1,[],\"\"]"
    );
    // Python stdlib json (ensure_ascii=False, compact) + hashlib; see oracle.
    assert_eq!(
        bitcoin_encoding::hex_encode(&digest(&payload).unwrap()).unwrap(),
        "1d60156c7d5c3d752ed401ba085300ea90869712b4acc88edff9601de4c0b15c"
    );
}

#[test]
fn independent_nip01_unicode_and_escape_vector() {
    let payload = request(
        1_700_000_000,
        1,
        &[
            &["version", "2.5.0"],
            &[
                "unicode",
                "\u{e9}",
                "e\u{301}",
                "\u{1f9d9}",
                "\u{2028}",
                "\u{2029}",
            ],
        ],
        "quote\" slash/ backslash\\ controls\u{8}\t\n\u{c}\r\0 <tag> apostrophe'",
    );
    assert_eq!(
        bitcoin_encoding::hex_encode(&digest(&payload).unwrap()).unwrap(),
        "4444cdbcc6520f0ae9e8013b1026f0458aa6d5c5e450fd212af656e760f8a1fd"
    );
    let preimage = canonical_preimage(&payload).unwrap();
    assert!(preimage.contains("<tag> apostrophe'"));
    assert!(preimage.contains("\u{2028}"));
    assert!(!preimage.contains("\\/"));
    assert!(!preimage.contains("\\u2028"));
}

#[test]
fn tag_order_duplicates_and_empty_tags_are_hash_inputs() {
    let first = request(1, 1, &[&["p", "one"], &["p", "two"], &[]], "x");
    let second = request(1, 1, &[&[], &["p", "two"], &["p", "one"]], "x");
    assert_ne!(digest(&first).unwrap(), digest(&second).unwrap());
    assert!(
        canonical_preimage(&first)
            .unwrap()
            .contains("[[\"p\",\"one\"],[\"p\",\"two\"],[]]")
    );
}

#[test]
fn does_not_normalize_unicode() {
    assert_ne!(
        digest(&request(1, 1, &[], "\u{e9}")).unwrap(),
        digest(&request(1, 1, &[], "e\u{301}")).unwrap()
    );
}

#[test]
fn integers_are_exact_and_not_float_or_culture_formatted() {
    let preimage = canonical_preimage(&request(i64::MIN, i32::MIN, &[], "")).unwrap();
    assert!(preimage.contains(",-9223372036854775808,-2147483648,[],"));
    let preimage = canonical_preimage(&request(i64::MAX, i32::MAX, &[], "")).unwrap();
    assert!(preimage.contains(",9223372036854775807,2147483647,[],"));
}

#[test]
fn every_truncated_prefix_is_rejected() {
    let payload = request(123, 1, &[&["a", "\u{1f9d9}", ""], &[]], "test\n");
    for length in 0..payload.len() {
        assert!(digest(&payload[..length]).is_err(), "prefix {length}");
    }
    assert!(digest(&payload).is_ok());
}

#[test]
fn unsupported_version_and_trailing_bytes() {
    let mut payload = request(0, 1, &[], "");
    payload[0] = 2;
    assert_eq!(digest(&payload), Err(Error::UnsupportedVersion));
    payload[0] = 1;
    payload.push(0);
    assert_eq!(digest(&payload), Err(Error::TrailingData));
}

#[test]
fn counts_are_checked_before_allocation() {
    let mut payload = request(0, 1, &[], "");
    payload[45..49].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(digest(&payload), Err(Error::InvalidCount));
    let mut payload = request(0, 1, &[&[]], "");
    payload[49..53].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(digest(&payload), Err(Error::InvalidCount));
    let mut payload = request(0, 1, &[], "");
    payload[49..53].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(digest(&payload), Err(Error::Truncated));
}

#[test]
fn invalid_utf8_cannot_cross_the_boundary() {
    for bytes in [
        &[0xff][..],
        &[0xc0, 0x80],
        &[0xed, 0xa0, 0x80],
        &[0xf4, 0x90, 0x80, 0x80],
        &[0xe2, 0x82],
    ] {
        let mut payload = request(0, 1, &[], "");
        payload[49..53].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
        payload.extend_from_slice(bytes);
        assert_eq!(digest(&payload), Err(Error::InvalidUtf8));
    }
}

#[test]
fn exact_request_limit_and_worst_case_escaping_are_bounded() {
    let payload = request(0, 1, &[], &"\0".repeat(MAX_REQUEST_BYTES - 53));
    assert_eq!(payload.len(), MAX_REQUEST_BYTES);
    let preimage = canonical_preimage(&payload).unwrap();
    assert!(preimage.len() <= nostr_event_id::MAX_CANONICAL_BYTES);
    assert!(digest(&payload).is_ok());
    let mut oversized = payload;
    oversized.push(0);
    assert_eq!(digest(&oversized), Err(Error::TooLarge));
}

#[test]
fn largest_empty_tag_array_fits_without_hidden_default_json_limits() {
    let count = (MAX_REQUEST_BYTES - 53) / 4;
    let tags = vec![&[][..]; count];
    assert!(digest(&request(0, 1, &tags, "")).is_ok());
}

#[test]
fn errors_do_not_include_untrusted_event_content() {
    let payload = request(0, 1, &[], "PRIVATE-SYNTHETIC-CONTENT");
    let error = digest(&payload[..payload.len() - 1]).unwrap_err();
    assert!(!error.to_string().contains("PRIVATE"));
    assert!(!format!("{error:?}").contains("PRIVATE"));
}
