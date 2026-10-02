#![forbid(unsafe_code)]
#[allow(dead_code)]
#[path = "../src/scan_service/mod.rs"]
mod scan_service;

use scan_service::{Control, Error, matrix, raster};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

#[test]
fn independent_golden_symbols_and_rasters() {
    for row in include_str!("qr_scanning_fixtures/golden.tsv").lines() {
        let fields = row.split('\t').collect::<Vec<_>>();
        let size = fields[0].parse::<usize>().unwrap();
        let modules = fields[2].bytes().map(|b| b == b'1').collect::<Vec<_>>();
        let decoded = matrix::decode_modules(size, &modules).unwrap();
        assert_eq!(decoded.text, fields[1]);
        let scale = 4;
        let side = (size + 8) * scale;
        let mut pixels = vec![245; side * side];
        for y in 0..size {
            for x in 0..size {
                if modules[y * size + x] {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            pixels[((y + 4) * scale + dy) * side + (x + 4) * scale + dx] = 15;
                        }
                    }
                }
            }
        }
        assert_eq!(
            raster::decode(raster::Image {
                width: side,
                height: side,
                stride: side,
                luminance: &pixels
            })
            .unwrap()
            .text,
            fields[1]
        );
    }
}

#[test]
fn cancellation_deadline_and_stride_fail_closed() {
    let cancelled = AtomicBool::new(true);
    let control = Control::new(&cancelled, Instant::now() + Duration::from_secs(2));
    assert_eq!(
        matrix::decode_modules_control(21, &[false; 441], control),
        Err(Error::Cancelled)
    );
    let not_cancelled = AtomicBool::new(false);
    let expired = Control::new(&not_cancelled, Instant::now() - Duration::from_millis(1));
    assert_eq!(
        raster::decode_control(
            raster::Image {
                width: 21,
                height: 21,
                stride: 21,
                luminance: &[255; 441]
            },
            expired
        ),
        Err(Error::Timeout)
    );
    assert!(
        raster::decode(raster::Image {
            width: 21,
            height: 21,
            stride: usize::MAX,
            luminance: &[]
        })
        .is_err()
    );
}
