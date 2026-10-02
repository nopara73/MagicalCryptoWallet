//! Development-only protocol driver; never a Cargo target or shipping executable.
#![forbid(unsafe_code)]
#[allow(dead_code)]
#[path = "../../src/scan_service/mod.rs"]
mod scan_service;
use scan_service::{
    Control, Result,
    matrix::{self, Decoded},
    raster,
};
use std::io::{self, BufRead};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

fn from_hex(s: &str) -> Vec<u8> {
    s.as_bytes()
        .chunks_exact(2)
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect()
}
fn main() {
    let cancel = AtomicBool::new(false);
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        let mut words = line.splitn(3, ' ');
        let kind = words.next().unwrap_or("");
        let value: Result<Decoded> = if kind == "M" {
            let size = words.next().unwrap().parse().unwrap();
            let bits = words
                .next()
                .unwrap()
                .as_bytes()
                .iter()
                .map(|&b| b == b'1')
                .collect::<Vec<_>>();
            matrix::decode_modules(size, &bits)
        } else if kind == "I" {
            let bytes = std::fs::read(words.next().unwrap()).unwrap();
            let mut splits = bytes.splitn(4, |&b| b == b'\n');
            assert_eq!(splits.next().unwrap(), b"P5");
            let dims = std::str::from_utf8(splits.next().unwrap())
                .unwrap()
                .split(' ')
                .map(|s| s.parse::<usize>().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(splits.next().unwrap(), b"255");
            raster::decode_control(
                raster::Image {
                    width: dims[0],
                    height: dims[1],
                    stride: dims[0],
                    luminance: splits.next().unwrap(),
                },
                Control::new(&cancel, Instant::now() + Duration::from_secs(2)),
            )
        } else if kind == "T" {
            let eci = words.next().unwrap().parse().unwrap();
            match scan_service::text::decode(&from_hex(words.next().unwrap()), Some(eci)) {
                Ok(t) => {
                    println!(
                        "TEXT {}",
                        t.as_bytes()
                            .iter()
                            .map(|b| format!("{b:02x}"))
                            .collect::<String>()
                    );
                    continue;
                }
                Err(_) => {
                    println!("ERR");
                    continue;
                }
            }
        } else {
            panic!("unknown oracle command");
        };
        match value {
            Ok(d) => println!(
                "OK {} {} {} {}",
                d.version,
                d.level,
                d.corrected_symbols,
                d.text
                    .as_bytes()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            ),
            Err(_) => println!("ERR"),
        }
    }
}
