//! Development-only line protocol; no package or shipping executable.
use mcw::bitcoin_encoding;
// The source needs this crate-root alias when Cargo compiles this test alone;
// the embedded development harness already supplies it at its outer root.
#[allow(unused_imports)]
use mcw::bitcoin_script;
#[path = "../src/script_text.rs"]
mod script_text;
use std::io::{self, BufRead, Write};

pub fn run() {
    let mut output = io::BufWriter::new(io::stdout().lock());
    for line in io::stdin().lock().lines() {
        let line = line.expect("probe input");
        let fields: Vec<_> = line.split('\t').collect();
        let bytes = bitcoin_encoding::hex_decode(fields[1]).unwrap();
        let result = match fields[0] {
            "P" => script_text::parse_utf8(&bytes),
            "R" => script_text::render(&bytes).map(String::into_bytes),
            _ => panic!("unknown test-only operation"),
        };
        let response = match result {
            Ok(bytes) => format!("OK:{}", bitcoin_encoding::hex_encode(&bytes).unwrap()),
            Err(_) => "ERR".to_owned(),
        };
        writeln!(output, "{response}").unwrap();
    }
}
