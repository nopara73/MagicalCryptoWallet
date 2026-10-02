//! Application services shared by the host, CLI and eventually the native UI.
pub mod app;
pub mod bitcoin_block;
pub mod bitcoin_encoding;
pub mod bitcoin_script;
pub mod bitcoin_wire;
pub mod bridge;
pub mod command;
pub mod compact_filters;
pub mod compression;
pub mod http1;
pub mod json;
pub mod payment_uri;
pub mod platform;
pub mod png;
pub mod privacy_service;
pub mod psbt;
pub mod psbt_metadata;
pub mod psbt_metadata_service;
pub mod qr;
pub mod script_service;
pub mod script_text;
pub mod socks5;
pub mod wallet_hash_service;
pub mod wallet_hashes;
pub mod websocket;

pub const VERSION: &str = match option_env!("MCW_VERSION") {
    Some(version) => version,
    None => "99.99.99",
};
