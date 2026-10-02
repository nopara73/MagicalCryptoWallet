#[path = "../src/bitcoin_encoding.rs"]
pub mod bitcoin_encoding;
#[path = "../src/json.rs"]
pub mod json;
#[path = "../src/nostr_event_id.rs"]
pub mod nostr_event_id;

use std::io::{self, BufRead, Write};

fn main() {
    let mut out = io::BufWriter::new(io::stdout().lock());
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        let payload = bitcoin_encoding::hex_decode(&line).unwrap();
        match nostr_event_id::canonical_preimage(&payload) {
            Ok(preimage) => {
                let hash = nostr_event_id::digest(&payload).unwrap();
                if preimage.len() > bitcoin_encoding::MAX_DATA_BYTES {
                    writeln!(
                        out,
                        "HASH\t{}",
                        bitcoin_encoding::hex_encode(&hash).unwrap()
                    )
                    .unwrap();
                    continue;
                }
                writeln!(
                    out,
                    "{}\t{}",
                    bitcoin_encoding::hex_encode(preimage.as_bytes()).unwrap(),
                    bitcoin_encoding::hex_encode(&hash).unwrap()
                )
                .unwrap();
            }
            Err(_) => writeln!(out, "ERR").unwrap(),
        }
    }
}
