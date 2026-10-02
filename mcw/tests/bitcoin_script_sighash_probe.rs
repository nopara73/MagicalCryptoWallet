use mcw::bitcoin_encoding::{hex_decode, hex_encode};
use mcw::bitcoin_wire::{Limits, Transaction, TxOut};
use mcw::script_service::sighash::*;
use std::io::{self, BufRead, Write};

pub fn run() {
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        let f: Vec<_> = line.split('\t').collect();
        let tx = Transaction::decode(&hex_decode(f[1]).unwrap(), &Limits::default()).unwrap();
        let index = f[3].parse().unwrap();
        let response = match f[0] {
            "L" => hex_encode(
                &legacy_sighash(
                    &tx,
                    index,
                    &hex_decode(f[2]).unwrap(),
                    f[4].parse().unwrap(),
                    &Limits::default(),
                )
                .unwrap(),
            )
            .unwrap(),
            "S" => hex_encode(
                &segwit_v0_sighash(
                    &tx,
                    index,
                    &hex_decode(f[2]).unwrap(),
                    f[4].parse().unwrap(),
                    f[5].parse().unwrap(),
                    &Limits::default(),
                )
                .unwrap(),
            )
            .unwrap(),
            "T" => {
                let spent: Vec<_> = f[2]
                    .split(';')
                    .map(|item| {
                        let (value, script) = item.split_once(':').unwrap();
                        TxOut {
                            value: value.parse().unwrap(),
                            script_pubkey: hex_decode(script).unwrap(),
                        }
                    })
                    .collect();
                let annex = if f[5] == "-" {
                    None
                } else {
                    Some(hex_decode(f[5]).unwrap())
                };
                let extension = if f[6] == "-" {
                    None
                } else {
                    Some(TapScriptExtension {
                        tapleaf_hash: hex_decode(f[6]).unwrap().try_into().unwrap(),
                        key_version: 0,
                        codesep_position: f[7].parse().unwrap(),
                    })
                };
                match taproot_sighash(
                    &tx,
                    &spent,
                    index,
                    f[4].parse().unwrap(),
                    annex.as_deref(),
                    extension,
                    &Limits::default(),
                ) {
                    Ok(digest) => hex_encode(&digest).unwrap(),
                    Err(Error::MissingSingleOutput { .. }) => "single".to_owned(),
                    e => panic!("unexpected Taproot result {e:?}"),
                }
            }
            _ => panic!("unknown probe command"),
        };
        writeln!(out, "{response}").unwrap();
    }
}
