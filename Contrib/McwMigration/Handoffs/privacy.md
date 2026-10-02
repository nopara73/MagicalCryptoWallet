# Privacy runtime migration

Worker: `01a0fc5c-d23b-7400-8819-68134acaf062`; operation reservation `0x0F00–0x0FFF`.
Initial current-remote source: `7f5f7ca8a297060d3826769aa9820948e89c7713`.
Isolated checkout: `.artifacts/mcw-privacy-20261002`.

**Full Tor/backend expansion stopped by the human scope correction on 2026-10-02.**
Published protocol checkpoint: `5d7d2c06de698ff3f5122b3c9e916c220f934135`,
verified as an ancestor of remote `master`. Production Tor remains
managed/bundled; no privacy dependency has been removed, and no self-contained or
wallet/mainnet-readiness claim is made. The daemon must not be retired on the strength of these tests. No further full
Tor, TLS, curve, storage, UI or CoinJoin implementation is authorized by this
checkpoint. Preserve the existing source and evidence.

## Ownership and caller declaration

Own `mcw/src/privacy_service/**`, `MagicalCryptoWallet/Mcw/Privacy/**`, uniquely
named `privacy_*` tests/fixtures and this document. QR owns shared Cargo/lib/main,
bridge dispatch/framing, platform/native bindings, app lifecycle and packaging.
Declared to QR/network/crypto/sync before edits: privacy responsibility is the
`MagicalCryptoWallet/Tor` manager/process/control/status leaves. Shared
`MagicalCryptoWallet.Client/Global.cs`, `Services/EventBus.cs`, manifests and
packaging require reviewable integration patches. Network owns WebClients/TLS;
sync owns P2P/DNS; Nostr owns discovery. No shared/peer checkout was edited.

The external coordinator uses `TorManager`, `TorProcessManager` and control onion
creation too. Its role must be separated before shared legacy files/packages can
be removed. The retained wallet itself also creates an ephemeral RPC onion service
in `MagicalCryptoWallet.Client/Global.cs`; it is part of privacy scope.

## Implemented checkpoint

- Safe portable Keccak-f1600, SHA3-256 and bounded SHAKE256. CoinJoin agreed reuse
  via `privacy_service::crypto::hash::keccak_f1600(&mut [u64;25])`; STROBE owns its
  distinct rate/domain padding. No second production permutation is needed.
- Exact bounded modern Tor cell framing/consumption (initial 16-bit VERSIONS,
  negotiated 32-bit circuit IDs, protocols4/5), commands/circuit checks, padding,
  CERTS containers and Ed25519 cert structures with duplicate/critical extension
  rejection. Client NETINFO sends neither local time nor client addresses.
- Tor v3 onion names, SHA3 checksum/version validation and redacted diagnostics.
- Relay envelope/BEGIN encoding, random-padding admission, authenticated circuit
  SENDME v1 tracking and bounded stream windows. These require real authenticated
  circuit/hop cryptography before network use.
- CTR mode over a typed AES block primitive, with counter exhaustion admission
  before mutation. No AES block or SHA1 implementation was duplicated: wallet
  crypto owns SHA1/HMAC/HKDF/AES, network owns X25519/P256/RSA/X509. CTR binding to
  the actual AES primitive is pending its verified publication.

There is no Tor controller wrapper, external-daemon substitution, cleartext TLS
downgrade, DNS resolution or direct-network fallback in this subtree. No fake
handler is registered. These modules are not yet the application's privacy backend.

## Verified checkpoint evidence

On native Windows x64, Rust1.99.0, edition2024: module compilation with warnings
denied, rustfmt check and Clippy all denied passed. `privacy_verify.ps1` ran
14 tests (9 privacy tests and5 existing peer hash boundary tests), none ignored.
Independent `hashlib` vectors cover 24 lengths, streaming SHA3 boundaries, and
300-byte SHAKE output. The Keccak team's 25-lane vector and three Tor-spec onion
addresses pass, with every tested one-symbol corruption rejected. Cell-prefix,
max-variable-length, certificate duplicate/unknown-critical-extension, relay
padding, clock/address suppression and SENDME-forgery/replay tests pass.

Reproduce with `python mcw/tests/privacy_reference.py`, then
`mcw/tests/privacy_verify.ps1`. The verifier takes one of the two exclusive shared
build slots, checks2GiB free memory, uses the installed toolchain/linker only and
writes uniquely scoped ignored evidence under `.artifacts/privacy-verification`.
The default-MSVC dev test executable imports must not be used as the shipping
runtime audit. Linux x64/ARM64 and macOS x64/ARM64 executions remain unverified.

`privacy_inventory.py` reads Git-tracked Tor payloads and source callers, and
parses PE/ELF/Mach-O imports without running Tor or using a public network. Exact
payload hashes/imports and caller lines are in `privacy_fixtures/inventory.json`.
The Windows Tor ledger reports0.4.9.9 plus embedded Libevent2.1.12/OpenSSL3.5.6/
zlib1.3.2; that is QR's separate version evidence, not a claim that the new source
removed those dependencies. Linux payloads retain libssl/libcrypto/libevent/
libstdc++; macOS payload retains libevent and the Tor executable. License notices
also require per-build provenance, particularly static libraries.

## Former broad cutover gates (audit only; not an active assignment)

The network peer confirmed authenticated TLS is **not implemented** yet. Tor needs
a distinct provisional TLS link returning bounded exact leaf DER, with no
resumption, compression, client auth, domain DNS/SNI or application/circuit cells
before CERTS proves the expected relay identity. Normal HTTPS keeps its separate
PKIX policy; no general certificate-bypass switch is acceptable. Missing actual
TLS/curve primitive APIs are a concrete upstream interface blocker for relay
execution. The human subsequently stopped the broad privacy-domain expansion;
these are recorded obligations rather than authorization to continue it.

Still required: real Ed25519 certificate authentication (network/crypto ownership
has no existing verifier), RSA authority/key-cert and consensus authentication,
microdescriptor binding, authenticated ntor/ntor-v3 and hop encryption, circuit
extension/stream lifecycle, persistent guard selection/recovery, path diversity,
onion descriptor/blinding/introduction/rendezvous/service hosting, padding and
Conflux, retries/cancellation/cleanup, exact retained privacy/isolation policy,
bridge transports (`obfs4`, Snowflake, WebTunnel) and their non-OS dependencies.
`TorSettings` currently requests three entry/primary guards, Conflux throughput,
and `ExtendedErrors KeepAliveIsolateSOCKSAuth`; silently dropping these is not a
compatible cutover. SOCKS-greeting success is not bootstrap readiness.

Production service/managed adapter/caller execution, synthetic independent Tor
network interoperability, privacy/security review, recovery/crash safety and all
five target runtime/package/import audits are required before deleting Tor,
OpenSSL, Libevent, zlib or any retained bridge implementation. Every state must
have one authoritative owner. Tor state/files/guard authority has not moved here.

## Bounded caller replacement audit after scope correction

The coordinator was sent the concrete small candidate: Tor control reply framing
and parsing in `MagicalCryptoWallet/Tor/Control/TorControlReplyReader.cs`, whose
production caller is `TorControlClient.ReaderLoopAsync`. It currently uses the
managed `PipeReaderLineReaderExtension.ReadLineAsync`. These leaves have not been
edited or registered pending the narrowed assignment. The old reply shape must
be preserved for `ProtocolInfoReply` (terminal raw `250 OK`) and
`GetInfoCircuitStatusReply` (data dot/terminal lines); ASCII bytes and literal
backslashes must survive. Replies/events, multi-line/data replies, exact
consumption, fragmentation, cancellation/EOF and bounded malformed inputs need
independent compatibility evidence. There is no need to replace Tor or change
transport/daemon/control authentication/wallet-state ownership for this candidate.

All14 verified tests and19 native-file/99 caller-line audit evidence are preserved.
No production service/legacy caller/package or shared host module was changed by
this checkpoint.
