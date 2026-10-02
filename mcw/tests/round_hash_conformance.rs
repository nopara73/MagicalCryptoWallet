// Verify the exact unused leaf without registering it in the application. The
// activation patch switches the production caller and host dispatcher together.
pub use mcw::privacy_service;
#[path = "../src/round_hash/mod.rs"]
mod round_hash;
use round_hash::{Error, OPERATION, strobe::Strobe};

fn hex(value: &str) -> Vec<u8> {
    assert!(value.len().is_multiple_of(2));
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|x| {
            let digit = |b: u8| (b as char).to_digit(16).unwrap() as u8;
            digit(x[0]) * 16 + digit(x[1])
        })
        .collect()
}

#[test]
fn unchanged_managed_round_hasher_vectors() {
    let mut tested = 0;
    for line in include_str!("round_hash_vectors/managed.tsv")
        .lines()
        .filter(|x| !x.starts_with('#'))
    {
        let c: Vec<_> = line.split('\t').collect();
        let payload = hex(c[1]);
        assert_eq!(
            round_hash::handle(OPERATION, &payload).unwrap(),
            hex(c[2]),
            "{}",
            c[0]
        );
        let parameters = round_hash::decode(&payload).unwrap();
        assert_eq!(
            round_hash::calculate(&parameters).unwrap().as_slice(),
            hex(c[2]),
            "{}",
            c[0]
        );
        tested += 1;
    }
    assert!(tested >= 25);
}

#[test]
fn malformed_bounded_payloads_fail_before_hashing() {
    let line = include_str!("round_hash_vectors/managed.tsv")
        .lines()
        .find(|x| !x.starts_with('#'))
        .unwrap();
    let c: Vec<_> = line.split('\t').collect();
    let payload = hex(c[1]);
    for end in 0..payload.len() {
        assert!(
            round_hash::handle(OPERATION, &payload[..end]).is_err(),
            "truncation {end}"
        );
    }
    for at in [0, 2] {
        let mut mutated = payload.clone();
        mutated[at] = 2;
        assert_eq!(
            round_hash::handle(OPERATION, &mutated),
            Err(Error::InvalidVersion)
        );
    }
    let mut trailing = payload.clone();
    trailing.push(0);
    assert_eq!(
        round_hash::handle(OPERATION, &trailing),
        Err(Error::TrailingBytes)
    );
    assert_eq!(
        round_hash::handle(OPERATION + 1, &payload),
        Err(Error::UnsupportedOperation)
    );
    assert_eq!(
        round_hash::handle(OPERATION, &vec![0; round_hash::MAX_PAYLOAD + 1]),
        Err(Error::Limit)
    );
    // Five i64 timing values and two input amounts precede script count.
    let mut many = payload.clone();
    many[60..62].copy_from_slice(&65u16.to_le_bytes());
    assert_eq!(round_hash::handle(OPERATION, &many), Err(Error::Limit));
    let mut long = payload.clone();
    long[62..66].copy_from_slice(&65_537u32.to_le_bytes());
    assert_eq!(round_hash::handle(OPERATION, &long), Err(Error::Limit));
    let mut invalid_utf8 = payload;
    invalid_utf8[66] = 0xff;
    assert_eq!(
        round_hash::handle(OPERATION, &invalid_utf8),
        Err(Error::InvalidUtf8)
    );
}

#[test]
fn continued_operations_preserve_framing_and_reject_changed_flags() {
    for length in [0, 1, 164, 165, 166, 167, 331, 332, 65_536] {
        let data = vec![0x41; length];
        let mut whole = Strobe::new(b"round-hash synthetic");
        whole.meta_ad(b"label", false).unwrap();
        whole.ad(&data, false).unwrap();
        let mut a = [0; 32];
        whole.prf(&mut a, false).unwrap();
        let mut split = Strobe::new(b"round-hash synthetic");
        split.meta_ad(b"label", false).unwrap();
        split.ad(&data[..length / 2], false).unwrap();
        split.ad(&data[length / 2..], true).unwrap();
        let mut b = [0; 32];
        split.prf(&mut b[..16], false).unwrap();
        split.prf(&mut b[16..], true).unwrap();
        assert_eq!(a, b, "length {length}");
        assert_eq!(split.ad(&[], true), Err(Error::InvalidContinuation));
    }
}

#[test]
fn native_callers_have_the_same_size_and_work_bounds() {
    let line = include_str!("round_hash_vectors/managed.tsv")
        .lines()
        .find(|x| !x.starts_with('#'))
        .unwrap();
    let payload = hex(line.split('\t').nth(1).unwrap());
    let mut p = round_hash::decode(&payload).unwrap();
    let too_long = "a".repeat(round_hash::MAX_STRING + 1);
    p.network = &too_long;
    assert_eq!(round_hash::calculate(&p), Err(Error::Limit));
    p.network = "Main";
    p.allowed_input_types = vec!["P2WPKH"; round_hash::MAX_SCRIPT_TYPES + 1];
    assert_eq!(round_hash::calculate(&p), Err(Error::Limit));
    let large = "a".repeat(round_hash::MAX_STRING);
    p.allowed_input_types = vec![&large; round_hash::MAX_SCRIPT_TYPES];
    assert_eq!(round_hash::calculate(&p), Err(Error::Limit));
}
