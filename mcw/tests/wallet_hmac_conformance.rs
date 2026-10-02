#![forbid(unsafe_code)]
use mcw::{
    bitcoin_encoding::hex_decode,
    wallet_hash_service::{self as service, Error, MAX_REQUEST_BYTES, Response},
};

fn bytes(hex: &str) -> Vec<u8> {
    hex_decode(hex).unwrap()
}
fn execute(operation: u16, payload: &[u8]) -> Vec<u8> {
    service::execute(operation, payload)
        .unwrap()
        .as_bytes()
        .to_vec()
}

#[test]
fn independent_binary_request_and_typed_handler_compatibility() {
    let mut count = 0;
    for row in include_str!("wallet_hmac_fixtures/independent.tsv").lines() {
        let f: Vec<_> = row.split('\t').collect();
        let operation = u16::from_str_radix(f[0], 16).unwrap();
        let payload = bytes(f[1]);
        let expected = bytes(f[2]);
        assert_eq!(execute(operation, &payload), expected, "case {count}");
        let typed = match operation {
            service::OWNERSHIP_IDENTIFIER => {
                service::ownership_identifier(payload[..32].try_into().unwrap(), &payload[32..])
            }
            service::SLIP21_SEED => service::slip21_seed(&payload),
            service::SLIP21_CHILD => {
                service::slip21_child(payload[..32].try_into().unwrap(), &payload[32..])
            }
            _ => unreachable!(),
        }
        .unwrap();
        assert_eq!(typed.as_bytes(), expected, "typed case {count}");
        count += 1;
    }
    assert_eq!(count, 441);
}

#[test]
fn public_slip21_tree_and_slip19_ownership_identifier() {
    let seed = bytes(concat!(
        "c76c4ac4f4e4a00d6b274d5c39c700bb4a7ddc04fbc6f78e85ca75007b5b495f7",
        "4a9043eeb77bdd53aa6fc3a0e31462270316fa04b8c19114c8798706cd02ac8"
    ));
    let master = execute(service::SLIP21_SEED, &seed);
    assert_eq!(
        &master[32..],
        bytes("dbf12b44133eaab506a740f6565cc117228cbf1dd70635cfa8ddfdc9af734756")
    );
    fn child(parent: &[u8], label: &[u8]) -> Vec<u8> {
        execute(service::SLIP21_CHILD, &[&parent[..32], label].concat())
    }
    let branch = child(&master, b"SLIP-0021");
    assert_eq!(
        &branch[32..],
        bytes("1d065e3ac1bbe5c7fad32cf2305f7d709dc070d672044a19e610c77cdf33de0d")
    );
    for (label, expected) in [
        (
            b"Master encryption key".as_slice(),
            "ea163130e35bbafdf5ddee97a17b39cef2be4b4f390180d65b54cf05c6a82fde",
        ),
        (
            b"Authentication key".as_slice(),
            "47194e938ab24cc82bfa25f6486ed54bebe79c40ae2a5a32ea6db294d81861a6",
        ),
    ] {
        assert_eq!(&child(&branch, label)[32..], bytes(expected));
    }
    let identification = child(
        &child(&master, b"SLIP-0019"),
        b"Ownership identification key",
    );
    let script = bytes("0014b2f771c370ccf219cd3059cda92bdf7f00cf2103");
    assert_eq!(
        execute(
            service::OWNERSHIP_IDENTIFIER,
            &[&identification[32..], &script].concat()
        ),
        bytes("a122407efc198211c81af4450f40b235d54775efd934d16b9e31c6ce9bad5707")
    );
}

#[test]
fn malformed_key_lengths_and_unknown_operations_reject_without_output() {
    for operation in [service::OWNERSHIP_IDENTIFIER, service::SLIP21_CHILD] {
        for length in 0..32 {
            assert!(matches!(
                service::execute(operation, &vec![0x53; length]),
                Err(Error::InvalidPayload)
            ));
        }
        assert!(service::execute(operation, &[0; 32]).is_ok());
    }
    for operation in [0, 1, 2, 0x0a00, 0x0a13, u16::MAX] {
        assert!(!service::handles(operation));
        assert!(matches!(
            service::execute(operation, b"SYNTHETIC_SECRET_MARKER"),
            Err(Error::UnknownOperation)
        ));
    }
    assert!(service::execute(service::SLIP21_SEED, b"").is_ok());
}

#[test]
fn exact_frame_capacity_and_one_byte_overflow() {
    let zeros = vec![0; MAX_REQUEST_BYTES];
    for (operation, expected) in [
        (
            service::OWNERSHIP_IDENTIFIER,
            "a9e1560a9ee06dec22d755da9725801e601f006b12be4522f2a2909dfe99c2d7",
        ),
        (
            service::SLIP21_SEED,
            "6b5629c1c4ed5442747c97d0d7c64074439d997242677aa7a89ad553eefa2a016e6995174813e95aa89d274905bed755ecd6c7bd59fcfddf607ca1de180882a0",
        ),
        (
            service::SLIP21_CHILD,
            "163af2bcb971545e25b44dc70ba556b0676953968c575aaa818cbb34fc554fdcdf7bde878d910f7bf0e80c0eee52bf55045289222abfb948995635a98de82a79",
        ),
    ] {
        assert_eq!(execute(operation, &zeros), bytes(expected));
        assert!(matches!(
            service::execute(operation, &vec![0; MAX_REQUEST_BYTES + 1]),
            Err(Error::InputLimit)
        ));
    }
    assert!(matches!(
        service::ownership_identifier(&[0; 32], &zeros),
        Err(Error::InputLimit)
    ));
    assert!(matches!(
        service::slip21_child(&[0; 32], &zeros),
        Err(Error::InputLimit)
    ));
}

#[test]
fn full_mac_lengths_label_prefix_and_binary_label_are_preserved() {
    let key = [0x61; 32];
    let label = b"\0\xff\0label";
    let output = service::slip21_child(&key, label).unwrap();
    assert_eq!(output.as_bytes().len(), 64);
    assert_eq!(
        output.as_bytes(),
        mcw::wallet_hashes::hmac_sha512(&key, &[&[0], label.as_slice()].concat()).unwrap()
    );
    assert_ne!(
        output.as_bytes(),
        mcw::wallet_hashes::hmac_sha512(&key, label).unwrap()
    );
    assert_eq!(service::slip21_seed(b"").unwrap().as_bytes().len(), 64);
    assert_eq!(
        service::ownership_identifier(&key, b"")
            .unwrap()
            .as_bytes()
            .len(),
        32
    );
}

#[test]
fn diagnostics_and_response_debug_never_include_payloads() {
    let marker = b"SYNTHETIC_SECRET_MARKER";
    let response = Response::Node([0x53; 64]);
    assert_eq!(format!("{response:?}"), "WalletHashResponse([REDACTED])");
    for error in [
        Error::InvalidPayload,
        Error::InputLimit,
        Error::HashFailure,
        Error::UnknownOperation,
    ] {
        let diagnostics = format!("{error:?}: {error}");
        assert!(!diagnostics.contains(std::str::from_utf8(marker).unwrap()));
        assert!((1..=3).contains(&error.code()));
    }
}
