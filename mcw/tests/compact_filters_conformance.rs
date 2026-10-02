//! Synthetic and published public-vector tests. No wallet state or network I/O.
use mcw::compact_filters::{
    Error, GcsFilter, Limits, Params, basic_key, chain_filter_headers, encode_basic, encode_gcs,
    encode_mapped_values, filter_header_from_hash, map_into_range, siphash24,
};

struct OfficialVector {
    height: u32,
    block_hash: &'static str,
    outputs: &'static [&'static str],
    spent: &'static [&'static str],
    previous_header: &'static str,
    encoded: &'static str,
    filter_hash_raw: &'static str,
    header: &'static str,
}

struct ReferenceCase {
    key: &'static str,
    p: u8,
    m: u32,
    items: &'static [&'static str],
    encoded: &'static str,
    values: &'static [u64],
    queries: &'static [&'static str],
    results: &'static [bool],
}

include!("compact_filters_vectors.inc");

fn hex(text: &str) -> Vec<u8> {
    assert!(text.len().is_multiple_of(2));
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let digit = |x: u8| match x {
                b'0'..=b'9' => x - b'0',
                b'a'..=b'f' => x - b'a' + 10,
                _ => panic!("bad fixture hex"),
            };
            (digit(pair[0]) << 4) | digit(pair[1])
        })
        .collect()
}

fn hash_from_display(text: &str) -> [u8; 32] {
    let mut bytes = hex(text);
    bytes.reverse();
    bytes.try_into().unwrap()
}

fn unhex_all(items: &[&str]) -> Vec<Vec<u8>> {
    items.iter().map(|x| hex(x)).collect()
}
fn refs(items: &[Vec<u8>]) -> Vec<&[u8]> {
    items.iter().map(Vec::as_slice).collect()
}

fn parse_error(bytes: &[u8], params: Params, limits: Limits) -> Error {
    GcsFilter::parse(bytes, [0; 16], params, limits).unwrap_err()
}

// Test-only arbitrary-delta encoder: textual bits rather than production codec.
// Allows deliberately invalid cumulative values and padding mutations.
fn raw_deltas(params: Params, deltas: &[u64]) -> Vec<u8> {
    assert!(deltas.len() < 253);
    let mut bits = String::new();
    for &delta in deltas {
        bits.extend(std::iter::repeat_n('1', (delta >> params.p()) as usize));
        bits.push('0');
        if params.p() != 0 {
            bits.push_str(&format!(
                "{:0width$b}",
                delta & ((1u64 << params.p()) - 1),
                width = params.p() as usize
            ));
        }
    }
    bits.extend(std::iter::repeat_n('0', (8 - bits.len() % 8) % 8));
    let mut encoded = vec![deltas.len() as u8];
    for byte in bits.as_bytes().chunks(8) {
        encoded.push(
            byte.iter()
                .fold(0u8, |n, b| (n << 1) | u8::from(*b == b'1')),
        );
    }
    encoded
}

#[test]
fn siphash_matches_all_64_original_author_vectors() {
    let key: [u8; 16] = std::array::from_fn(|i| i as u8);
    let message: Vec<u8> = (0..64).collect();
    for (n, &expected) in SIPHASH_VECTORS.iter().enumerate() {
        assert_eq!(
            siphash24(&key, &message[..n]),
            expected,
            "SipHash length {n}"
        );
    }
}

#[test]
fn siphash_length_byte_wrap_and_long_messages_match_independent_reference() {
    let key: [u8; 16] = std::array::from_fn(|i| i as u8);
    for &(n, expected) in LONG_SIPHASH_VECTORS {
        let message: Vec<u8> = (0..n).map(|i| i as u8).collect();
        assert_eq!(siphash24(&key, &message), expected, "SipHash length {n}");
    }
}

#[test]
fn basic_filters_and_bip157_headers_match_every_official_testnet_vector() {
    assert_eq!(OFFICIAL_VECTORS.len(), 10);
    for vector in OFFICIAL_VECTORS {
        let block_hash = hash_from_display(vector.block_hash);
        let outputs = unhex_all(vector.outputs);
        let spent = unhex_all(vector.spent);
        let expected = hex(vector.encoded);
        let built = encode_basic(
            &block_hash,
            &refs(&outputs),
            &refs(&spent),
            Limits::default(),
        )
        .unwrap();
        assert_eq!(built, expected, "official filter height {}", vector.height);
        let filter = GcsFilter::parse_basic(&built, &block_hash, Limits::default()).unwrap();
        assert_eq!(filter.params(), Params::BASIC);
        assert_eq!(filter.key().as_slice(), &block_hash[..16]);
        assert_eq!(filter.encoded(), expected);
        assert_eq!(
            filter.filter_hash().unwrap().as_slice(),
            hex(vector.filter_hash_raw)
        );
        let previous = hash_from_display(vector.previous_header);
        let header = filter.filter_header(&previous).unwrap();
        assert_eq!(
            header,
            hash_from_display(vector.header),
            "official header height {}",
            vector.height
        );
        assert_eq!(
            filter_header_from_hash(&filter.filter_hash().unwrap(), &previous).unwrap(),
            header
        );
        let included: Vec<&[u8]> = outputs
            .iter()
            .filter(|x| !x.is_empty() && x[0] != 106)
            .chain(spent.iter().filter(|x| !x.is_empty()))
            .map(Vec::as_slice)
            .collect();
        for element in &included {
            assert!(filter.matches(element).unwrap());
        }
        assert_eq!(filter.match_any(&included).unwrap(), !included.is_empty());
        assert!(filter.match_queries(&included).unwrap().iter().all(|x| *x));
        assert_eq!(
            encode_mapped_values(
                filter.params(),
                &filter.mapped_values().unwrap(),
                Limits::default()
            )
            .unwrap(),
            expected
        );
    }
}

#[test]
fn independent_gcs_reference_covers_parameters_collisions_duplicates_and_queries() {
    assert_eq!(REFERENCE_CASES.len(), 90);
    for (index, case) in REFERENCE_CASES.iter().enumerate() {
        let key: [u8; 16] = hex(case.key).try_into().unwrap();
        let params = Params::new(case.p, case.m).unwrap();
        let items = unhex_all(case.items);
        let queries = unhex_all(case.queries);
        let built = encode_gcs(&key, params, &refs(&items), Limits::default()).unwrap();
        assert_eq!(built, hex(case.encoded), "GCS case {index}");
        let filter = GcsFilter::parse(&built, key, params, Limits::default()).unwrap();
        assert_eq!(
            filter.mapped_values().unwrap(),
            case.values,
            "values case {index}"
        );
        assert_eq!(
            filter.match_queries(&refs(&queries)).unwrap(),
            case.results,
            "queries case {index}"
        );
        assert_eq!(
            filter.match_any(&refs(&queries)).unwrap(),
            case.results.contains(&true)
        );
        for (query, &expected) in queries.iter().zip(case.results) {
            assert_eq!(
                filter.matches(query).unwrap(),
                expected,
                "single query case {index}"
            );
        }
    }
}

#[test]
fn range_mapping_uses_product_high_bits_without_modulo_or_truncation() {
    for range in [0, 1, 2, 784_931, u32::MAX as u64, u64::MAX] {
        assert_eq!(map_into_range(0, range), 0);
        assert_eq!(map_into_range(u64::MAX, range), range.saturating_sub(1));
        assert_eq!(map_into_range(1u64 << 63, range), range / 2);
    }
    assert_eq!(
        map_into_range(0x123456789abcdef0, 0xfedcba9876543210),
        0x121fa00ad77d7422
    );
}

#[test]
fn block_hash_key_uses_first_16_internal_bytes() {
    let hash: [u8; 32] = std::array::from_fn(|i| i as u8);
    assert_eq!(basic_key(&hash), std::array::from_fn(|i| i as u8));
}

#[test]
fn golomb_rice_bit_order_matches_the_bip158_p2_table() {
    let params = Params::new(2, 10).unwrap();
    let bodies = [0x00, 0x20, 0x40, 0x60, 0x80, 0x90, 0xa0, 0xb0, 0xc0, 0xc8];
    for (n, &body) in bodies.iter().enumerate() {
        let encoded = encode_mapped_values(params, &[n as u64], Limits::default()).unwrap();
        assert_eq!(encoded, [1, body]);
        assert_eq!(
            GcsFilter::parse(&encoded, [0; 16], params, Limits::default())
                .unwrap()
                .mapped_values()
                .unwrap(),
            [n as u64]
        );
    }
}

#[test]
fn sorted_delta_coding_preserves_equal_mapped_values() {
    let params = Params::new(2, 2).unwrap();
    let values = [1, 1, 2, 6, 6, 8];
    let encoded = encode_mapped_values(params, &values, Limits::default()).unwrap();
    assert_eq!(encoded, [6, 0x20, 0xc0, 0x40]);
    let filter = GcsFilter::parse(&encoded, [0; 16], params, Limits::default()).unwrap();
    assert_eq!(filter.count(), 6);
    assert_eq!(filter.mapped_values().unwrap(), values);
    assert_eq!(
        encode_mapped_values(params, &[1, 0], Limits::default()),
        Err(Error::ValuesNotSorted)
    );
    assert_eq!(
        encode_mapped_values(params, &[4], Limits::default()),
        Err(Error::ValueOutOfRange)
    );
}

#[test]
fn empty_filter_and_empty_generic_element_have_distinct_meanings() {
    let params = Params::new(0, 1).unwrap();
    assert_eq!(
        encode_gcs(&[0; 16], params, &[], Limits::default()).unwrap(),
        [0]
    );
    let empty = GcsFilter::parse(&[0], [0; 16], params, Limits::default()).unwrap();
    assert_eq!(empty.count(), 0);
    assert!(!empty.matches(b"").unwrap());
    assert!(!empty.match_any(&[b"", b"a"]).unwrap());
    assert!(!empty.match_any(&[]).unwrap());
    assert_eq!(empty.match_queries(&[b"", b"a"]).unwrap(), [false, false]);
    assert_eq!(empty.mapped_values().unwrap(), []);
    let encoded = encode_gcs(&[0; 16], params, &[b"", b""], Limits::default()).unwrap();
    assert_eq!(encoded, [1, 0]);
    let filter = GcsFilter::parse(&encoded, [0; 16], params, Limits::default()).unwrap();
    assert_eq!(filter.count(), 1);
    assert!(filter.matches(b"").unwrap());
    // F=1 maps every query to zero: this is an intentional false positive.
    assert!(filter.matches(b"absent").unwrap());
}

#[test]
fn basic_script_selection_uses_byte_rules_and_a_single_union_set() {
    let outputs: &[&[u8]] = &[b"", &[0x6a, 0x51], &[0x4c, 0x6a], &[0x51], &[0x51]];
    let spent: &[&[u8]] = &[b"", &[0x6a, 0x52], &[0x51]];
    let block_hash = [1; 32];
    let encoded = encode_basic(&block_hash, outputs, spent, Limits::default()).unwrap();
    let expected = encode_gcs(
        &basic_key(&block_hash),
        Params::BASIC,
        &[&[0x4c, 0x6a], &[0x51], &[0x6a, 0x52]],
        Limits::default(),
    )
    .unwrap();
    assert_eq!(encoded, expected);
    let filter = GcsFilter::parse_basic(&encoded, &block_hash, Limits::default()).unwrap();
    assert_eq!(filter.count(), 3);
    assert!(filter.match_any(&[&[0x51], &[0x51]]).unwrap());
    assert_eq!(
        encode_basic(&block_hash, &[b"", &[0x6a]], &[b""], Limits::default()).unwrap(),
        [0]
    );
}

#[test]
fn compact_size_boundaries_are_minimal_and_little_endian() {
    let params = Params::new(0, 1).unwrap();
    let limits = Limits::default();
    for (count, prefix) in [
        (252, vec![252]),
        (253, vec![253, 253, 0]),
        (65535, vec![253, 255, 255]),
        (65536, vec![254, 0, 0, 1, 0]),
    ] {
        let encoded = encode_mapped_values(params, &vec![0; count], limits).unwrap();
        assert!(encoded.starts_with(&prefix));
        assert_eq!(encoded.len(), prefix.len() + count.div_ceil(8));
        assert_eq!(
            GcsFilter::parse(&encoded, [0; 16], params, limits)
                .unwrap()
                .count(),
            count as u32
        );
    }
    for bytes in [
        &[253, 252, 0][..],
        &[254, 255, 255, 0, 0],
        &[255, 255, 255, 255, 255, 0, 0, 0, 0],
    ] {
        assert_eq!(
            parse_error(bytes, params, limits),
            Error::NonCanonicalCompactSize
        );
    }
    assert_eq!(
        parse_error(&[255, 0, 0, 0, 0, 1, 0, 0, 0], params, limits),
        Error::CountOverflow
    );
    assert_eq!(
        parse_error(
            &[254, 255, 255, 255, 255],
            params,
            Limits {
                max_elements: u32::MAX,
                ..limits
            }
        ),
        Error::Truncated
    );
}

#[test]
fn decoder_rejects_truncation_trailing_bytes_and_nonzero_padding() {
    let limits = Limits::default();
    for bytes in [
        &[][..],
        &[253],
        &[253, 0],
        &[254, 0, 0, 0],
        &[255, 0, 0, 0, 0, 0, 0, 0],
    ] {
        assert_eq!(parse_error(bytes, Params::BASIC, limits), Error::Truncated);
    }
    assert_eq!(
        parse_error(&[1, 0], Params::BASIC, limits),
        Error::Truncated
    );
    assert_eq!(
        parse_error(&[0, 0], Params::BASIC, limits),
        Error::TrailingBytes
    );
    assert_eq!(
        parse_error(&[1, 0, 0, 0, 0], Params::BASIC, limits),
        Error::TrailingBytes
    );
    assert_eq!(
        parse_error(&[1, 0, 0, 1], Params::BASIC, limits),
        Error::NonZeroPadding
    );
    assert_eq!(
        parse_error(&[1, 0, 0, 15], Params::BASIC, limits),
        Error::NonZeroPadding
    );
}

#[test]
fn decoder_bounds_unary_quotients_and_cumulative_sums() {
    let limits = Limits::default();
    assert_eq!(
        parse_error(&[1, 255], Params::new(0, 1).unwrap(), limits),
        Error::ValueOutOfRange
    );
    assert_eq!(
        parse_error(&[1, 255, 255, 255], Params::BASIC, limits),
        Error::ValueOutOfRange
    );
    assert_eq!(
        parse_error(&[2, 0xa0], Params::new(0, 1).unwrap(), limits),
        Error::ValueOutOfRange
    );
    let params = Params::new(2, 10).unwrap();
    assert_eq!(
        parse_error(&raw_deltas(params, &[0, 20]), params, limits),
        Error::ValueOutOfRange
    );
    assert_eq!(
        parse_error(
            &raw_deltas(Params::BASIC, &[784_931]),
            Params::BASIC,
            limits
        ),
        Error::ValueOutOfRange
    );
}

#[test]
fn malformed_suffix_cannot_bypass_validation_on_a_match_or_empty_queries() {
    let encoded = encode_gcs(&[0; 16], Params::BASIC, &[b"synthetic"], Limits::default()).unwrap();
    for extra in [0, 255] {
        let mut bytes = encoded.clone();
        bytes.push(extra);
        let answer = GcsFilter::parse(&bytes, [0; 16], Params::BASIC, Limits::default())
            .and_then(|f| f.matches(b"synthetic"));
        assert_eq!(answer, Err(Error::TrailingBytes));
        let answer = GcsFilter::parse(&bytes, [0; 16], Params::BASIC, Limits::default())
            .and_then(|f| f.match_any(&[]));
        assert_eq!(answer, Err(Error::TrailingBytes));
    }
}

#[test]
fn resource_limits_apply_before_allocations_and_after_exclusions() {
    let limits = Limits::default();
    assert_eq!(
        encode_mapped_values(
            Params::new(0, u32::MAX).unwrap(),
            &[u32::MAX as u64 - 1],
            limits
        ),
        Err(Error::FilterTooLarge)
    );
    assert_eq!(
        parse_error(
            &[0],
            Params::BASIC,
            Limits {
                max_filter_bytes: 0,
                ..limits
            }
        ),
        Error::FilterTooLarge
    );
    assert_eq!(
        parse_error(
            &[1, 0, 0, 0],
            Params::BASIC,
            Limits {
                max_elements: 0,
                ..limits
            }
        ),
        Error::TooManyElements
    );
    assert_eq!(
        encode_gcs(
            &[0; 16],
            Params::BASIC,
            &[b"a", b"a"],
            Limits {
                max_elements: 1,
                ..limits
            }
        ),
        Err(Error::TooManyElements)
    );
    assert_eq!(
        encode_basic(
            &[0; 32],
            &[b"", b""],
            &[],
            Limits {
                max_elements: 1,
                ..limits
            }
        ),
        Err(Error::TooManyElements)
    );
    assert_eq!(
        encode_basic(
            &[0; 32],
            &[&[0x6a, 1]],
            &[b"ab"],
            Limits {
                max_input_bytes: 3,
                ..limits
            }
        ),
        Err(Error::InputTooLarge)
    );
    assert_eq!(
        encode_gcs(
            &[0; 16],
            Params::BASIC,
            &[b"ab"],
            Limits {
                max_input_bytes: 1,
                ..limits
            }
        ),
        Err(Error::InputTooLarge)
    );
    let f = GcsFilter::parse(
        &[0],
        [0; 16],
        Params::BASIC,
        Limits {
            max_queries: 1,
            max_input_bytes: 1,
            ..limits
        },
    )
    .unwrap();
    assert_eq!(f.match_any(&[b"", b""]), Err(Error::TooManyQueries));
    assert_eq!(f.match_queries(&[b"ab"]), Err(Error::InputTooLarge));
    assert_eq!(f.matches(b"ab"), Err(Error::InputTooLarge));
    let f = GcsFilter::parse(
        &[0],
        [0; 16],
        Params::BASIC,
        Limits {
            max_queries: 0,
            ..limits
        },
    )
    .unwrap();
    assert_eq!(f.matches(b""), Err(Error::TooManyQueries));
    assert!(!f.match_any(&[]).unwrap());
    assert_eq!(
        encode_mapped_values(
            Params::BASIC,
            &[],
            Limits {
                max_filter_bytes: 0,
                ..limits
            }
        ),
        Err(Error::FilterTooLarge)
    );
    let bytes = encode_mapped_values(Params::BASIC, &[0], limits).unwrap();
    assert_eq!(
        encode_mapped_values(
            Params::BASIC,
            &[0],
            Limits {
                max_filter_bytes: bytes.len(),
                ..limits
            }
        )
        .unwrap(),
        bytes
    );
}

#[test]
fn all_parameter_edges_are_safe_and_invalid_parameters_are_rejected() {
    assert_eq!(Params::new(64, 1), Err(Error::InvalidParameters));
    assert_eq!(Params::new(19, 0), Err(Error::InvalidParameters));
    for p in [0, 1, 19, 31, 32, 62, 63] {
        let params = Params::new(p, u32::MAX).unwrap();
        let values = if p == 0 { vec![0, 0, 0] } else { vec![0, 1, 2] };
        let encoded = encode_mapped_values(params, &values, Limits::default()).unwrap();
        assert_eq!(
            GcsFilter::parse(&encoded, [0; 16], params, Limits::default())
                .unwrap()
                .mapped_values()
                .unwrap(),
            values
        );
    }
    let params = Params::new(63, u32::MAX).unwrap();
    let values = [0, 2 * u32::MAX as u64 - 1];
    let encoded = encode_mapped_values(params, &values, Limits::default()).unwrap();
    assert_eq!(
        GcsFilter::parse(&encoded, [0; 16], params, Limits::default())
            .unwrap()
            .mapped_values()
            .unwrap(),
        values
    );
}

#[test]
fn header_chain_is_ordered_bounded_and_uses_the_given_anchor() {
    let hashes: Vec<[u8; 32]> = OFFICIAL_VECTORS
        .iter()
        .take(3)
        .map(|v| hex(v.filter_hash_raw).try_into().unwrap())
        .collect();
    let anchor = [0; 32];
    let headers = chain_filter_headers(&hashes, &anchor, 3).unwrap();
    assert_eq!(headers[0], hash_from_display(OFFICIAL_VECTORS[0].header));
    let mut previous = anchor;
    for (hash, &header) in hashes.iter().zip(&headers) {
        assert_eq!(filter_header_from_hash(hash, &previous).unwrap(), header);
        previous = header;
    }
    assert_eq!(
        chain_filter_headers(&hashes, &anchor, 2),
        Err(Error::TooManyElements)
    );
    assert!(chain_filter_headers(&[], &anchor, 0).unwrap().is_empty());
    assert_ne!(headers, chain_filter_headers(&hashes, &[1; 32], 3).unwrap());
    // Official vectors have gaps; this synthetic chain is not the real chain.
}

#[test]
fn exhaustive_short_untrusted_encodings_reencode_identically_if_accepted() {
    let params = Params::new(2, 7).unwrap();
    let limits = Limits {
        max_filter_bytes: 3,
        max_elements: 4,
        ..Limits::default()
    };
    let queries: &[&[u8]] = &[b"", b"one", b"two", b"three"];
    let mut accepted = 0;
    for count in 0..=4u8 {
        for word in 0..=u16::MAX {
            let bytes = [count, (word >> 8) as u8, word as u8];
            if let Ok(filter) = GcsFilter::parse(&bytes, [0; 16], params, limits) {
                accepted += 1;
                let values = filter.mapped_values().unwrap();
                assert_eq!(
                    encode_mapped_values(params, &values, limits).unwrap(),
                    bytes
                );
                let expected: Vec<bool> = queries
                    .iter()
                    .map(|q| {
                        values
                            .binary_search(&map_into_range(
                                siphash24(&[0; 16], q),
                                u64::from(count) * 7,
                            ))
                            .is_ok()
                    })
                    .collect();
                assert_eq!(filter.match_queries(queries).unwrap(), expected);
                assert_eq!(filter.match_any(queries).unwrap(), expected.contains(&true));
            }
        }
    }
    assert!(
        accepted > 1000,
        "exercise valid as well as rejected encodings: {accepted}"
    );
}

#[test]
fn mutations_of_official_filters_never_escape_canonical_validation() {
    for vector in OFFICIAL_VECTORS {
        let original = hex(vector.encoded);
        let key = basic_key(&hash_from_display(vector.block_hash));
        for index in 0..original.len() {
            for bit in 0..8 {
                let mut mutated = original.clone();
                mutated[index] ^= 1 << bit;
                if let Ok(filter) =
                    GcsFilter::parse(&mutated, key, Params::BASIC, Limits::default())
                {
                    assert_eq!(
                        encode_mapped_values(
                            Params::BASIC,
                            &filter.mapped_values().unwrap(),
                            Limits::default()
                        )
                        .unwrap(),
                        mutated
                    );
                }
            }
        }
    }
}
