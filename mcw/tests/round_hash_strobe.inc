use crate::round_hash::strobe::Strobe;

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
fn published_strobego_ad_meta_ad_prf_and_rate_checkpoints() {
    let mut tested = 0;
    for line in include_str!("round_hash_vectors/strobego.tsv")
        .lines()
        .filter(|x| !x.starts_with('#'))
    {
        let c: Vec<_> = line.split('\t').collect();
        if c[1] == "init" {
            let s = Strobe::new(&hex(c[2]));
            assert_eq!(s.test_state().as_slice(), hex(c[3]), "{}", c[0]);
        } else {
            let mut s = Strobe::from_test_checkpoint(
                hex(c[2]).try_into().unwrap(),
                c[3].parse().unwrap(),
                c[4].parse().unwrap(),
                c[5].parse().unwrap(),
            );
            let more = c[6] == "1";
            match c[1] {
                "AD" => s.ad(&hex(c[7]), more).unwrap(),
                "metaAD" => s.meta_ad(&hex(c[7]), more).unwrap(),
                "PRF" => {
                    let expected = hex(c[7]);
                    let mut output = vec![0; expected.len()];
                    s.prf(&mut output, more).unwrap();
                    assert_eq!(output, expected, "{}", c[0]);
                }
                _ => panic!("invalid fixture operation"),
            }
            assert_eq!(s.test_state().as_slice(), hex(c[8]), "{}", c[0]);
        }
        tested += 1;
    }
    assert_eq!(tested, 67);
}
