//! Development-only line protocol for the independent Python oracle.
use mcw::bitcoin_encoding;
use mcw::bitcoin_script::*;
use std::io::{self, BufRead, Write};

pub fn run() {
    let stdout = io::stdout();
    let mut output = io::BufWriter::new(stdout.lock());
    for line in io::stdin().lock().lines() {
        let line = line.expect("probe input");
        let fields: Vec<_> = line.split('\t').collect();
        let response = match fields[0] {
            "P" => {
                let bytes = bitcoin_encoding::hex_decode(fields[1]).unwrap();
                match validate(&bytes, ValidationPolicy::default()) {
                    Ok(s) => format!(
                        "OK:{}:{}:{}:{}",
                        s.instruction_count,
                        s.nonminimal_push_count,
                        s.maximum_push_bytes,
                        u8::from(s.is_push_only)
                    ),
                    Err(Error::TruncatedLength {
                        offset,
                        needed,
                        available,
                    }) => format!("ERR:length:{offset}:{needed}:{available}"),
                    Err(Error::TruncatedPush {
                        offset,
                        declared,
                        available,
                    }) => format!("ERR:push:{offset}:{declared}:{available}"),
                    error => panic!("unexpected probe parse result {error:?}"),
                }
            }
            "N" => {
                let bytes = bitcoin_encoding::hex_decode(fields[1]).unwrap();
                match decode_script_number(&bytes, fields[2].parse().unwrap(), fields[3] == "1") {
                    Ok(n) => n.to_string(),
                    Err(Error::NumberTooLarge { .. }) => "size".to_owned(),
                    Err(Error::NonMinimalNumber) => "minimal".to_owned(),
                    Err(Error::NumberOverflow) => "overflow".to_owned(),
                    error => panic!("unexpected probe number result {error:?}"),
                }
            }
            "E" => bitcoin_encoding::hex_encode(&encode_script_number(fields[1].parse().unwrap()))
                .unwrap(),
            "F" | "D" => {
                let script = Script::from_hex(fields[1]).unwrap();
                let text = if fields[0] == "F" {
                    script.to_format_asm().unwrap()
                } else {
                    script.to_core_asm().unwrap()
                };
                bitcoin_encoding::hex_encode(text.as_bytes()).unwrap()
            }
            _ => panic!("unknown probe command"),
        };
        writeln!(output, "{response}").unwrap();
    }
}
