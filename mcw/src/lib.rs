//! Application services shared by the host, CLI and eventually the native UI.
pub mod app;
#[path = "bitcoin_encoding/address_service.rs"]
pub mod bitcoin_address_service;
pub mod bitcoin_block;
pub mod bitcoin_block_service;
pub mod bitcoin_encoding;
pub mod bitcoin_script;
pub mod bitcoin_wire;
pub mod bridge;
pub mod command;
pub mod compact_filters;
pub mod compression;
pub mod content_service;
pub mod http1;
pub mod json;
pub mod markdown;
pub mod nostr_event_id;
pub mod payment_uri;
pub mod platform;
pub mod png;
pub mod privacy_service;
pub mod psbt;
pub mod psbt_metadata;
pub mod psbt_metadata_service;
pub mod qr;
pub mod round_hash;
pub mod safe_file_service;
pub mod scan_service;
pub mod script_service;
pub mod script_text;
pub mod socks5;
pub mod wallet_hash_service;
pub mod wallet_hashes;
pub mod websocket;

#[path = "privacy_service/control_codec/mod.rs"]
pub mod tor_control;

pub const VERSION: &str = match option_env!("MCW_VERSION") {
    Some(version) => version,
    None => "99.99.99",
};
