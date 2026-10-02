#![forbid(unsafe_code)]
use mcw::wallet_hashes::*;

fn bytes(hex: &str) -> Vec<u8> {
    assert!(hex.len().is_multiple_of(2));
    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digit = |c: u8| match c {
                b'0'..=b'9' => c - b'0',
                b'a'..=b'f' => c - b'a' + 10,
                b'A'..=b'F' => c - b'A' + 10,
                _ => panic!("invalid test hex"),
            };
            digit(pair[0]) * 16 + digit(pair[1])
        })
        .collect()
}
fn synthetic(length: usize) -> Vec<u8> {
    (0..length)
        .map(|i| (i.wrapping_mul(179).wrapping_add(71) % 256) as u8)
        .collect()
}

#[test]
fn all_offline_primary_vectors() {
    let mut count = 0;
    for line in include_str!("wallet_hashes_fixtures/vectors.tsv").lines() {
        if line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split('\t').collect();
        assert_eq!(fields.len(), 5);
        let key = bytes(fields[1]);
        let message = bytes(fields[2]);
        let expected = bytes(fields[4]);
        let iterations = fields[3].parse::<u32>().unwrap();
        let actual: Vec<u8> = match fields[0] {
            "ripemd160" => ripemd160(&message).unwrap().to_vec(),
            "sha512" => sha512(&message).unwrap().to_vec(),
            "hmac256" => hmac_sha256(&key, &message).unwrap()[..expected.len()].to_vec(),
            "hmac512" => hmac_sha512(&key, &message).unwrap()[..expected.len()].to_vec(),
            "pbkdf2256" => pbkdf2_hmac_sha256(&key, &message, iterations, expected.len()).unwrap(),
            "pbkdf2512" => pbkdf2_hmac_sha512(&key, &message, iterations, expected.len()).unwrap(),
            _ => panic!("unknown vector operation"),
        };
        assert_eq!(
            actual,
            expected,
            "primary vector row {} ({})",
            count + 1,
            fields[0]
        );
        count += 1;
    }
    assert!(
        count >= 550,
        "all NIST and multilingual BIP39 primitive vectors required"
    );
}

#[test]
fn published_million_a_hashes() {
    let chunk = vec![b'a'; 1000];
    let mut r = Ripemd160::new();
    let mut s = Sha512::new();
    for _ in 0..1000 {
        r.update(&chunk).unwrap();
        s.update(&chunk).unwrap();
    }
    assert_eq!(
        r.finalize().as_slice(),
        bytes("52783243c1697bdbe16d37f97f68f08325dc1528")
    );
    // FIPS 180-4 / RFC6234 SHA-512 million-'a' example.
    assert_eq!(
        s.finalize().as_slice(),
        bytes(concat!(
            "e718483d0ce769644e2e42c7bc15b4638e1f98b13b2044285632a803afa973eb",
            "de0ff244877ea60a4cb0432ce577c31beb009c5c2c49aa2e4eadb217ad8cc09b"
        ))
    );
}

#[test]
fn streaming_padding_boundaries_and_empty_updates() {
    for length in [
        0, 1, 2, 55, 56, 57, 63, 64, 65, 111, 112, 113, 119, 120, 127, 128, 129, 191, 192, 193,
        239, 240, 255, 256, 257, 511,
    ] {
        let data = synthetic(length);
        let expected_r = ripemd160(&data).unwrap();
        let expected_s = sha512(&data).unwrap();
        let expected_h = hash160(&data).unwrap();
        for split in 0..=length {
            let mut r = Ripemd160::new();
            let mut s = Sha512::new();
            let mut h = Hash160::new();
            for part in [&data[..split], b"", &data[split..], b""] {
                r.update(part).unwrap();
                s.update(part).unwrap();
                h.update(part).unwrap();
            }
            assert_eq!(
                r.finalize(),
                expected_r,
                "RIPEMD length {length}, split {split}"
            );
            assert_eq!(
                s.finalize(),
                expected_s,
                "SHA512 length {length}, split {split}"
            );
            assert_eq!(
                h.finalize(),
                expected_h,
                "HASH160 length {length}, split {split}"
            );
        }
    }
}

#[test]
fn hash160_published_bitcoin_public_key_bytes() {
    // SEC2 generator's compressed public encoding; no curve operations here.
    let public = bytes("0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798");
    assert_eq!(
        hash160(&public).unwrap().as_slice(),
        bytes("751e76e8199196d454941c45d1b3a323f1433bd6")
    );
    assert_eq!(
        hash160(b"").unwrap().as_slice(),
        bytes("b472a266d0bd89c13706a4132ccfb16f7c3b9fcb")
    );
}

#[test]
fn hmac_empty_binary_and_all_key_boundaries_stream_identically() {
    for key_len in [
        0, 1, 20, 31, 32, 33, 55, 56, 63, 64, 65, 111, 112, 127, 128, 129, 131, 255, 256, 1024,
    ] {
        let key = synthetic(key_len);
        for length in [0, 1, 55, 56, 63, 64, 65, 111, 112, 127, 128, 129, 255, 256] {
            let data = synthetic(length);
            let expected256 = hmac_sha256(&key, &data).unwrap();
            let expected512 = hmac_sha512(&key, &data).unwrap();
            for step in [1, 7, 64, 128] {
                let mut a = HmacSha256::new(&key).unwrap();
                let mut b = HmacSha512::new(&key).unwrap();
                for chunk in data.chunks(step) {
                    a.update(chunk).unwrap();
                    b.update(chunk).unwrap();
                }
                a.update(b"").unwrap();
                b.update(b"").unwrap();
                assert_eq!(a.finalize().unwrap(), expected256);
                assert_eq!(b.finalize().unwrap(), expected512);
            }
        }
    }
}

#[test]
fn clones_preserve_prefix_and_are_independent() {
    let prefix = synthetic(137);
    let mut r = Ripemd160::new();
    let mut s = Sha512::new();
    let mut h = Hash160::new();
    let mut a = HmacSha256::new(b"synthetic").unwrap();
    let mut b = HmacSha512::new(b"synthetic").unwrap();
    r.update(&prefix).unwrap();
    s.update(&prefix).unwrap();
    h.update(&prefix).unwrap();
    a.update(&prefix).unwrap();
    b.update(&prefix).unwrap();
    for suffix in [b"left".as_slice(), b"right", b""] {
        let data = [prefix.as_slice(), suffix].concat();
        let mut rr = r.clone();
        rr.update(suffix).unwrap();
        assert_eq!(rr.finalize(), ripemd160(&data).unwrap());
        let mut ss = s.clone();
        ss.update(suffix).unwrap();
        assert_eq!(ss.finalize(), sha512(&data).unwrap());
        let mut hh = h.clone();
        hh.update(suffix).unwrap();
        assert_eq!(hh.finalize(), hash160(&data).unwrap());
        let mut aa = a.clone();
        aa.update(suffix).unwrap();
        assert_eq!(
            aa.finalize().unwrap(),
            hmac_sha256(b"synthetic", &data).unwrap()
        );
        let mut bb = b.clone();
        bb.update(suffix).unwrap();
        assert_eq!(
            bb.finalize().unwrap(),
            hmac_sha512(b"synthetic", &data).unwrap()
        );
    }
}

#[test]
fn full_mac_verification_rejects_every_modified_byte_and_wrong_length() {
    let key = b"synthetic verification key";
    let message = b"synthetic payload";
    let a = hmac_sha256(key, message).unwrap();
    let b = hmac_sha512(key, message).unwrap();
    assert_eq!(verify_hmac_sha256(key, message, &a), Ok(()));
    assert_eq!(verify_hmac_sha512(key, message, &b), Ok(()));
    for index in 0..32 {
        let mut bad = a;
        bad[index] ^= 1;
        assert_eq!(
            verify_hmac_sha256(key, message, &bad),
            Err(Error::MacMismatch)
        );
    }
    for index in 0..64 {
        let mut bad = b;
        bad[index] ^= 1;
        assert_eq!(
            verify_hmac_sha512(key, message, &bad),
            Err(Error::MacMismatch)
        );
    }
    for length in [0, 1, 16, 31, 33, 63, 65] {
        let invalid = vec![0; length];
        if length != 32 {
            assert_eq!(
                verify_hmac_sha256(key, message, &invalid),
                Err(Error::InvalidMacLength)
            );
        }
        if length != 64 {
            assert_eq!(
                verify_hmac_sha512(key, message, &invalid),
                Err(Error::InvalidMacLength)
            );
        }
    }
    assert_eq!(
        HmacSha256::new(key).unwrap().verify(&a[..16]),
        Err(Error::InvalidMacLength)
    );
    assert_eq!(
        HmacSha512::new(key).unwrap().verify(&b[..16]),
        Err(Error::InvalidMacLength)
    );
}

#[test]
fn equal_length_comparison_visits_all_byte_positions() {
    for length in [0, 1, 4, 16, 32, 64, 255, 4096] {
        let data = synthetic(length);
        assert!(constant_time_eq(&data, &data));
        for index in 0..length {
            let mut bad = data.clone();
            bad[index] ^= 0x80;
            assert!(!constant_time_eq(&data, &bad));
        }
        assert!(!constant_time_eq(&data, &vec![0; length + 1]));
    }
}

#[test]
fn pbkdf2_allocation_and_caller_buffer_match_multi_block_truncation() {
    for length in [1, 2, 31, 32, 33, 63, 64, 65, 95, 96, 127, 128, 129, 257] {
        for iterations in [1, 2, 3, 7, 100] {
            let mut a = vec![0xa5; length];
            let mut b = vec![0xa5; length];
            pbkdf2_hmac_sha256_into(b"pass\0word", b"sa\0lt", iterations, &mut a).unwrap();
            pbkdf2_hmac_sha512_into(b"pass\0word", b"sa\0lt", iterations, &mut b).unwrap();
            assert_eq!(
                a,
                pbkdf2_hmac_sha256(b"pass\0word", b"sa\0lt", iterations, length).unwrap()
            );
            assert_eq!(
                b,
                pbkdf2_hmac_sha512(b"pass\0word", b"sa\0lt", iterations, length).unwrap()
            );
        }
    }
}

#[test]
fn pbkdf2_rejection_is_explicit_and_preserves_output() {
    let too_long = vec![0; MAX_PBKDF2_INPUT_BYTES + 1];
    for (password, salt, iterations, length, error) in [
        (
            b"p".as_slice(),
            b"s".as_slice(),
            0,
            64,
            Error::InvalidIterations,
        ),
        (
            b"p",
            b"s",
            MAX_PBKDF2_ITERATIONS + 1,
            64,
            Error::IterationLimit,
        ),
        (b"p", b"s", u32::MAX, 64, Error::IterationLimit),
        (b"p", b"s", 1, 0, Error::InvalidOutputLength),
        (
            b"p",
            b"s",
            1,
            MAX_PBKDF2_OUTPUT_BYTES + 1,
            Error::OutputLimit,
        ),
        (&too_long, b"s", 1, 64, Error::InputLimit),
        (b"p", &too_long, 1, 64, Error::InputLimit),
        (
            b"p",
            b"s",
            MAX_PBKDF2_ITERATIONS,
            MAX_PBKDF2_OUTPUT_BYTES,
            Error::WorkLimit,
        ),
    ] {
        let mut output = vec![0xa5; length];
        assert_eq!(
            pbkdf2_hmac_sha256_into(password, salt, iterations, &mut output),
            Err(error)
        );
        assert!(output.iter().all(|&b| b == 0xa5));
        assert_eq!(
            pbkdf2_hmac_sha512_into(password, salt, iterations, &mut output),
            Err(error)
        );
        assert!(output.iter().all(|&b| b == 0xa5));
        assert_eq!(
            pbkdf2_hmac_sha256(password, salt, iterations, length),
            Err(error)
        );
        assert_eq!(
            pbkdf2_hmac_sha512(password, salt, iterations, length),
            Err(error)
        );
    }
    assert_eq!(
        pbkdf2_hmac_sha512(b"", b"", 1, usize::MAX),
        Err(Error::OutputLimit)
    );
}

#[test]
fn diagnostic_states_and_errors_are_redacted() {
    let marker = b"synthetic SECRET marker";
    let mut r = Ripemd160::new();
    r.update(marker).unwrap();
    let mut s = Sha512::new();
    s.update(marker).unwrap();
    let mut h = Hash160::new();
    h.update(marker).unwrap();
    let a = HmacSha256::new(marker).unwrap();
    let b = HmacSha512::new(marker).unwrap();
    assert_eq!(format!("{r:?}"), "Ripemd160([REDACTED])");
    assert_eq!(format!("{s:?}"), "Sha512([REDACTED])");
    assert_eq!(format!("{h:?}"), "Hash160([REDACTED])");
    assert_eq!(format!("{a:?}"), "HmacSha256([REDACTED])");
    assert_eq!(format!("{b:?}"), "HmacSha512([REDACTED])");
    for error in [
        Error::Ripemd160MessageTooLong,
        Error::Sha256MessageTooLong,
        Error::Sha512MessageTooLong,
        Error::InvalidIterations,
        Error::IterationLimit,
        Error::InvalidOutputLength,
        Error::OutputLimit,
        Error::InputLimit,
        Error::WorkLimit,
        Error::AllocationFailed,
        Error::InvalidMacLength,
        Error::MacMismatch,
    ] {
        assert!(!format!("{error:?}: {error}").contains("SECRET"));
    }
}

#[test]
fn retained_slip21_hmac_compatibility_without_key_state_migration() {
    // Public SLIP-0021 example only. Nodes are test buffers, never wallet objects.
    let seed = bytes(concat!(
        "c76c4ac4f4e4a00d6b274d5c39c700bb4a7ddc04fbc6f78e85ca75007b5b495f7",
        "4a9043eeb77bdd53aa6fc3a0e31462270316fa04b8c19114c8798706cd02ac8"
    ));
    let master = hmac_sha512(b"Symmetric key seed", &seed).unwrap();
    assert_eq!(
        &master[32..],
        bytes("dbf12b44133eaab506a740f6565cc117228cbf1dd70635cfa8ddfdc9af734756")
    );
    let branch = hmac_sha512(&master[..32], b"\0SLIP-0021").unwrap();
    assert_eq!(
        &branch[32..],
        bytes("1d065e3ac1bbe5c7fad32cf2305f7d709dc070d672044a19e610c77cdf33de0d")
    );
    let encryption = hmac_sha512(&branch[..32], b"\0Master encryption key").unwrap();
    assert_eq!(
        &encryption[32..],
        bytes("ea163130e35bbafdf5ddee97a17b39cef2be4b4f390180d65b54cf05c6a82fde")
    );
    let auth = hmac_sha512(&branch[..32], b"\0Authentication key").unwrap();
    assert_eq!(
        &auth[32..],
        bytes("47194e938ab24cc82bfa25f6486ed54bebe79c40ae2a5a32ea6db294d81861a6")
    );
}

#[test]
fn retained_ownership_identifier_matches_existing_slip19_vector() {
    // Exact public SLIP19/managed-test bytes. Only HMAC composition and supplied
    // script bytes are tested; no BIP32, curve, signing, or wallet object exists.
    let seed = bytes(concat!(
        "c76c4ac4f4e4a00d6b274d5c39c700bb4a7ddc04fbc6f78e85ca75007b5b495f7",
        "4a9043eeb77bdd53aa6fc3a0e31462270316fa04b8c19114c8798706cd02ac8"
    ));
    let master = hmac_sha512(b"Symmetric key seed", &seed).unwrap();
    let branch = hmac_sha512(&master[..32], b"\0SLIP-0019").unwrap();
    let leaf = hmac_sha512(&branch[..32], b"\0Ownership identification key").unwrap();
    let script = bytes("0014b2f771c370ccf219cd3059cda92bdf7f00cf2103");
    let identifier = bytes("a122407efc198211c81af4450f40b235d54775efd934d16b9e31c6ce9bad5707");
    assert_eq!(
        hmac_sha256(&leaf[32..], &script).unwrap().as_slice(),
        identifier
    );
    assert_eq!(
        verify_hmac_sha256(&leaf[32..], &script, &identifier),
        Ok(())
    );
}
