//! Test-only stdin/stdout driver of actual first-party source, never shipped.
#![forbid(unsafe_code)]
#[path = "../src/websocket.rs"]
pub mod websocket;
use std::io::{self, BufRead, Write};
use websocket::*;

fn bytes(text: &str) -> Vec<u8> {
    assert!(text.len().is_multiple_of(2));
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn direction(text: &str) -> Direction {
    match text {
        "C" => Direction::ClientToServer,
        "S" => Direction::ServerToClient,
        _ => panic!("test direction"),
    }
}
fn handle(line: &str) -> String {
    let parts: Vec<_> = line.split('\t').collect();
    match parts[0] {
        "H" => {
            let nonce = bytes(parts[1]).try_into().unwrap();
            let handshake = ClientHandshake::new(nonce, &[]).unwrap();
            format!("{}|{}", handshake.key(), handshake.expected_accept())
        }
        "F" => {
            let input = bytes(parts[3]);
            match parse_frame(&input, direction(parts[1]), parts[2].parse().unwrap()) {
                Ok(None) => "MORE".into(),
                Ok(Some(parsed)) => format!(
                    "OK|{}|{}|{}|{}",
                    parsed.frame.header.opcode as u8,
                    u8::from(parsed.frame.header.final_fragment),
                    parsed.consumed,
                    hex(&parsed.frame.payload().unwrap())
                ),
                Err(error) => format!("ERR|{error:?}"),
            }
        }
        "E" => {
            let op = Opcode::from_byte(parts[2].parse().unwrap());
            let mask = if parts[4] == "-" {
                None
            } else {
                Some(bytes(parts[4]).try_into().unwrap())
            };
            let payload = bytes(parts[5]);
            match op.and_then(|op| {
                encode_frame(
                    direction(parts[1]),
                    op,
                    parts[3] == "1",
                    &payload,
                    mask,
                    parts[6].parse().unwrap(),
                )
            }) {
                Ok(frame) => format!("OK|{}", hex(&frame)),
                Err(error) => format!("ERR|{error:?}"),
            }
        }
        "G" => {
            let offered: Vec<String> = if parts[2] == "-" {
                vec![]
            } else {
                parts[2]
                    .split(',')
                    .map(|p| String::from_utf8(bytes(p)).unwrap())
                    .collect()
            };
            let offered_refs: Vec<_> = offered.iter().map(String::as_str).collect();
            let fields: Vec<(String, String)> = if parts[3] == "-" {
                vec![]
            } else {
                parts[3]
                    .split(',')
                    .map(|p| {
                        let (name, value) = p.split_once(':').unwrap();
                        (
                            String::from_utf8(bytes(name)).unwrap(),
                            String::from_utf8(bytes(value)).unwrap(),
                        )
                    })
                    .collect()
            };
            let field_refs: Vec<_> = fields
                .iter()
                .map(|(name, value)| Header { name, value })
                .collect();
            match ClientHandshake::new(*b"the sample nonce", &offered_refs)
                .and_then(|h| h.validate_response(parts[1].parse().unwrap(), &field_refs))
            {
                Ok(negotiated) => format!(
                    "OK|{}",
                    negotiated
                        .subprotocol
                        .map_or("-".into(), |p| hex(p.as_bytes()))
                ),
                Err(error) => format!("ERR|{error:?}"),
            }
        }
        _ => panic!("unknown test command"),
    }
}
fn main() {
    let mut output = io::BufWriter::new(io::stdout().lock());
    for line in io::stdin().lock().lines() {
        writeln!(output, "{}", handle(&line.unwrap())).unwrap();
    }
}
