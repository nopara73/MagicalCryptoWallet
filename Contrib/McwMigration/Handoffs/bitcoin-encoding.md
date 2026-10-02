# Bitcoin address and data encoding handoff

State: portable implementation and bounded address-validation handoff prepared
for application-host integration. The published handler and deferred caller patch
are separate from incorporation into the production host. The host, bridge,
managed adapters and shipping launch paths remain owned by the QR/application-host
task. The bounded patch passed the Windows checks below. No managed package
references have been removed by this worker.

Worker: bitcoin-encoding, Codex thread 01a0fc27-21f9-7901-abff-f8a7acdaacdf.
Implementation commit: 6365f3244d23b801c0bb36581967caad8aae968e.
Source SHA-256: 4e59e9210317f6c75f1196d22892202b3c72f62adcf9d05be88c4c3c66925ec0 (canonical LF Git blob).
Caller inventory snapshot: 2f4867c188b49d61d2eded702975fca111d3bbe4.

## Owned files

- mcw/src/bitcoin_encoding.rs
- mcw/tests/bitcoin_encoding_conformance.rs
- mcw/tests/bitcoin_encoding_fixtures/ (offline vectors, sources, retained-caller inventory)
- mcw/tests/bitcoin_encoding_prepare_fixtures.py
- mcw/tests/bitcoin_encoding_reference.py
- mcw/tests/bitcoin_encoding_inventory.py
- mcw/tests/bitcoin_encoding_verify.ps1
- mcw/src/bitcoin_encoding/address_service.rs
- mcw/tests/bitcoin_encoding_service.rs
- mcw/tests/bitcoin_encoding_prepare_integration.py
- mcw/tests/bitcoin_encoding_integration.patch
- mcw/tests/bitcoin_encoding_managed_probe.cs
- mcw/tests/bitcoin_encoding_integration_verify.ps1
- Contrib/McwMigration/Handoffs/bitcoin-encoding.md

No Cargo package, library artifact, shipping executable or external Cargo
dependency is added. Test executables and reference downloads exist only in this
worker's ignored .artifacts/bitcoin-encoding-evidence directory. The implementation
has a forbid(unsafe_code) directive and imports only std::fmt; its other types and
collections come from the Rust standard library. No platform handles, native
bindings, C# types, Avalonia or IPC enter the domain API.

## Public domain API

The application integrator adds the module declaration to its existing mcw crate.
All names below live in bitcoin_encoding. Every fallible function returns the
module's typed Error, which implements std::error::Error and Display.

| API | Arguments | Result |
| --- | --- | --- |
| hex_encode | &[u8] | Result<String, Error>, lowercase |
| hex_decode | &str | Result<Vec<u8>, Error>, ASCII digits in either case |
| base58_encode / base58_decode | &[u8] / &str | Result<String / Vec<u8>, Error> |
| base58check_encode / base58check_decode | &[u8] / &str | Result<String / Vec<u8>, Error> |
| sha256 / double_sha256 | &[u8] | Result<[u8; 32], Error>, raw digest order |
| Sha256::new / Default | none | Sha256, cloneable streaming state |
| Sha256::update | &mut self, &[u8] | Result<(), Error> |
| Sha256::finalize | self, consumed | [u8; 32] |
| bech32_encode | hrp: &str, symbols: &[u8], ChecksumVariant | Result<String, Error> |
| bech32_decode | &str | Result<Bech32Data, Error> |
| convert_bits | &[u8], from: u8, to: u8, pad: bool | Result<Vec<u8>, Error> |
| legacy_address_encode | Network, LegacyKind, &[u8; 20] | Result<String, Error> |
| legacy_address_decode | &str, Network | Result<LegacyAddress, Error> |
| witness_address_encode | Network, version: u8, program: &[u8] | Result<String, Error> |
| witness_address_decode | &str, Network | Result<WitnessAddress, Error> |
| address_decode | &str, Network | Result<Address, Error> |
| address_encode | &Address | Result<String, Error> |

Network variants are Mainnet, Testnet, Testnet4, Signet and Regtest. Address is
Legacy(LegacyAddress) or Witness(WitnessAddress). LegacyAddress has network,
kind: LegacyKind (P2pkh or P2sh), and hash: [u8; 20]. WitnessAddress has network,
version: u8, and program: Vec<u8>. Encoding revalidates public mutable fields.

Bech32Data contains hrp: String preserving the input's original spelling,
data: Vec<u8> of five-bit symbols with its checksum removed, and
variant: ChecksumVariant (Bech32 or Bech32m). Generic Bech32 does not interpret
witness rules; use the witness/address API for Bitcoin address validation.

The URI worker consumes address_decode through its own typed validator adapter
and preserves the original URI/address spelling separately. The transaction and
compact-filter workers consume sha256/double_sha256 or the streaming state.
Digest bytes are never reversed by this module. Callers must apply display or
wire-order conventions explicitly at their own boundaries.

## Behavior and bounds

Inputs are never trimmed, silently repaired, Unicode-normalized or lossily
converted to ASCII. Hex accepts both letter cases. Base58 is case-sensitive and
rejects whitespace, non-ASCII and excluded 0/O/I/l. Leading zero bytes round trip
as leading '1' symbols. Empty data round trips in the generic data codecs.
Base58Check appends/removes exactly four bytes from double-SHA256(payload);
payload includes any caller-supplied version byte(s) and otherwise stays opaque.
Empty Base58Check payloads are supported; Bitcoin addresses require their own
nonempty payload format.

Bech32 decoders reject mixed case, invalid HRP/alphabet, missing separators,
invalid checksums and more than 90 total ASCII bytes. They accept complete
uppercase input as specified, preserve HRP spelling, and fold case only during
checksum calculation. Encoding requires an explicitly lowercase HRP and emits
lowercase output. convert_bits supports 1..=8-bit unsigned symbols, rejects
out-of-range values, and with pad=false rejects nonzero or excessive residual
padding. Eight-to-five conversion pads with zero; the reverse conversion does
not permit extra full symbols.

Witness addresses use bc, tb or bcrt for the explicitly selected network.
Version 0 uses Bech32 and exactly 20 or 32 program bytes. Versions 1..=16 use
Bech32m with 2..=40 bytes, retaining future-version compatibility. A codec-valid
address does not prove key ownership, script satisfaction or spendability;
script/consensus/wallet policy remain outside this module.

Standard legacy addresses require a one-byte P2PKH/P2SH prefix and 20-byte hash:
mainnet prefixes 00/05, all supported test-network prefixes 6f/c4.
Network selection is mandatory. Testnet, Testnet4 and Signet share tb; all
non-mainnet networks share legacy prefixes. The text cannot distinguish those
networks. Decoders return the supplied, validated network rather than guessing.

| Bound | Value |
| --- | --- |
| Hex bytes | 1,048,576; text is at most twice that |
| Generic bit-conversion input and output symbols | 1,048,576 each |
| Base58 decoded bytes | 4,096 |
| Base58Check payload bytes | 4,092 |
| Base58 text bound | 5,653 |
| Generic Bech32 total bytes | 90 |
| Standard legacy address text | at most 35 |
| Witness version / program | 0..=16 / 2..=40, plus v0 restrictions |
| SHA-256 total streamed bytes | u64::MAX / 8 |

Base58 radix conversion is quadratic and bounded separately. SHA state and its
compression schedule use fixed arrays, with no heap allocation. Length overflow
is rejected before state mutation. Finalization consumes the state. Allocation
failure uses normal Rust allocator behavior; the codec does not claim recovery
from process-wide out-of-memory failures.

Errors distinguish size limits, invalid character and byte offset, odd hex
length, checksum, separator/HRP/case, symbol/bit-width/padding, witness version,
program length and checksum variant, network mismatch, legacy version/length,
and SHA length overflow. No source text or wallet secret is included in an Error.

## Proposed bridge operations

Only range 0x0200–0x02FF is reserved. This is a proposal, not implemented framing.
The host integrator defines stable operation IDs, payload layout and error mapping.

| Proposed operation | Service |
| --- | --- |
| 0x0200 / 0x0201 | hex encode / decode |
| 0x0202 / 0x0203 | Base58 encode / decode |
| 0x0204 / 0x0205 | Base58Check encode / decode |
| 0x0206 / 0x0207 | generic Bech32 encode / decode |
| 0x0208 / 0x0209 | witness encode / decode |
| 0x020A / 0x020B | legacy encode / decode |
| 0x020C | address validation with explicit network, returning typed payload |
| 0x020D / 0x020E | SHA-256 / double-SHA256 |
| 0x020F | optional bit conversion |

The domain's hex bound can produce two MiB of text. A bridge with one-MiB frames
must reject requests whose encoded response will not fit, accounting for framing
overhead before allocating/copying. The domain bound does not override transport
limits. Native future callers can use the same API directly. Payloads and keys
must never be placed in command arguments or logs by a managed adapter.

## Verification

Rust 1.99.0, edition 2024. Final source passes rustfmt and Clippy with warnings
denied, standalone library metadata and test metadata. Sixteen Rust tests pass
in debug and optimized builds with overflow checks enabled, including:

- all 79 published BIP173/BIP350 valid and invalid vectors, including rejection
  of the three superseded BIP173 v1+ Bech32 examples;
- 21 Bitcoin Core Base58 pairs, 54 valid addresses and 70 invalid inputs;
- every witness version 0..=17, length 0..=41 and supported network;
- mixed case, non-ASCII, alphabets, leading zeros, checksum substitutions,
  invalid/excessive padding, wrong networks/variants and allocation bounds;
- known SHA messages, one million a bytes, binary data, padding boundaries,
  cloned prefix state, fragmented input, and synthetic atomic length overflow.

Independent differential verification: 8,466 cases pass, seed 0x173350.
Python hashlib validates 23 one-shot and 23 double hashes plus 230 fragmented
streams. Independent arbitrary-precision Base58 validates 300 encode/decode
pairs and 300 Base58Check pairs plus 300 invalid checksums. The hash-pinned BIP
Python reference validates generic and witness encoding/decoding, uppercase
input, randomized programs, mutated strings and malformed version/padding data.
Exact source/reference hashes, primary-source URLs, fixture counts and licenses
are recorded in mcw/tests/bitcoin_encoding_fixtures/SOURCES.md and ignored
differential-results.json.

Peer combined evidence: the compact-filter worker reported 19 debug/optimized
tests passing against this exact final SHA source, including all ten official
BIP158 filters, their SHA256d hashes and BIP157 headers. Its implementation
commit is e06082994fbf44e2b66cdffad66d3bb98f4e0215. This evidence concerns the
actual portable modules and synthetic fixtures, not production synchronization.

Windows x64: actual debug/optimized test execution verified. Static-CRT optimized
test binary imports only api-ms-win-core-synch-l1-2-0.dll, bcryptprimitives.dll,
KERNEL32.dll, ntdll.dll and USERENV.dll; no VC++ redistributable or third-party DLL.
These imports belong to the test harness/Rust standard library, not an OS crypto
implementation of this module's SHA. The hash implementation is portable Rust.

Linux x64/ARM64 and macOS x64/ARM64: not executed here. Their target standard
libraries are absent from the shared Windows toolchain, so metadata checks are
recorded unavailable, not passed. Source has no cfg branches or platform binding,
but that fact is not a substitute for each target's compilation and execution.
The integrator must run the conformance suite in its target release matrix.

Reproduce from the worker checkout, using the existing shared toolchain and
native MSVC/SDK build libraries:

    $taskBin = 'C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet\.artifacts\mcw-tools\rustup\toolchains\1.99.0-x86_64-pc-windows-msvc\bin'
    $taskSlots = 'C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet\.artifacts\mcw-coordination'
    $taskLibs = @('C:\Program Files\Microsoft Visual Studio\18\Community\VC\Tools\MSVC\14.51.36231\lib\onecore\x64', 'C:\Program Files (x86)\Windows Kits\10\Lib\10.0.26100.0\ucrt\x64', 'C:\Program Files (x86)\Windows Kits\10\Lib\10.0.26100.0\um\x64')
    .\mcw\tests\bitcoin_encoding_verify.ps1 -ToolchainBin $taskBin -CoordinationRoot $taskSlots -ReferenceDirectory .artifacts/bitcoin-encoding-evidence -NativeLibraryPaths $taskLibs

ReferenceDirectory is optional for the offline Rust suite. For differential
checks, download the exact primary reference into that ignored directory and
verify its hash. The script takes one shared exclusive build slot, checks two GiB
free memory, uses one build job, and returns a deferred result when slots/memory
are unavailable. It does not install, mutate or replace the shared toolchain.

Direct portable test command after host integration:

    cargo test --manifest-path mcw/Cargo.toml --locked --offline --test bitcoin_encoding_conformance

Before that, rustc --edition=2024 --test can compile the conformance source
directly; no second Cargo package or shipping executable is required.

## Retained callers and dependencies

NBitcoin 10.0.13 and NBitcoin.Secp256k1 3.1.6 remain. The complete mechanical
snapshot is mcw/tests/bitcoin_encoding_fixtures/managed_callers.tsv: 493 direct
NBitcoin source references in 382 files, 116 address/data symbols in 42 files,
6 package references/version entries in 5 files, and 56 lock-file references in
14 files. Global imports, including WabiSabi/GlobalUsings.cs, are recorded; the
list does not imply implicitly imported types elsewhere have been removed.
Regenerate using bitcoin_encoding_inventory.py after future managed migrations.
The inventory reads immutable Git blobs at HEAD in one batch, so its revision
stays accurate even if an unrelated caller has uncommitted local changes.

Managed mapping for the integrator:

| Managed responsibility/callers | Rust service and retained work |
| --- | --- |
| Userfacing/AddressParser.ParseBitcoinAddress and Extensions/NBitcoinExtensions.TryParseBitcoinAddressForNetwork | address_decode(text, explicit network); managed result/error adapter remains |
| Userfacing/Bip21/Bip21UriParser; send/recipient/CoinJoin payment/paste/camera callers of AddressParser | payment-uri worker owns URI parsing; feed its isolated address string to address_decode |
| Rpc/JsonConverters/BitcoinAddressJsonConverter and DestinationJsonConverter; search transaction address query | address_decode and address_encode; preserve or deliberately adapt existing network-selection policy in managed boundary |
| Fluent/Models/Wallets/Address.Text and receive address presentation | witness/legacy encoding from existing managed hash/program bytes; HD/curve derivation stays managed |
| HdPubKey.GetP2wpkhAddress and KeyManager's HdPubKey GetAddress extension | replace only final payload-to-text encoding after existing key/hash/taproot computation |
| CoinModel, wallet transaction/destination display, SmartTransactionExtensions, Client/Bootstrap script-to-address | managed script inspection remains; encode extracted standard legacy/witness payloads |
| Serialization/Bitcoin.cs Encoders.Hex.DecodeData and hex presentation | hex_decode/hex_encode on existing bytes; transaction parsing/validation remains outside |
| NBitcoinExtensions.ToZPrv | opaque base58check_encode(version || existing payload); key derivation, payload production and keys remain managed |
| Test fixture Encoders.Hex and other address constructors | migrate tests to native codecs where applicable; do not treat test changes as production migration |

Current managed AddressParser trims input, and the JSON converter trims and tries
multiple networks. The Rust domain intentionally rejects surrounding whitespace
and requires a network. Any retained UI compatibility transform must be explicit
at the caller boundary. Standard output payloads need the existing managed script
construction until the script/transaction layer migrates. The bounded deferred
adapter below covers only the common public-address parsing helper; private keys
do not move into the Rust service.

Direct NBitcoin package references remain in MagicalCryptoWallet.csproj,
Contrib/Releases/Publisher/MagicalCryptoWallet.ReleaseTools.csproj and
ThirdParty/WabiSabi/interop/WabiSabiInterop.Tests/WabiSabiInterop.Tests.csproj.
ThirdParty/WabiSabi/csharp/WabiSabi/WabiSabi.csproj retains NBitcoin.Secp256k1.
The central version entries and all locked/transitive consumers are included
in the caller snapshot. KeyManager/HD keys, curve/signatures, transactions/PSBT,
script/consensus policy, P2P/RPC synchronization, filters, CoinJoin ownership and
other uses continue to require the existing package. Other rewrite workers own
some of these responsibilities; their uncommitted work is not claimed here.

The current managed NBitcoin lock also retains Newtonsoft.Json 13.0.4 and
Microsoft.Extensions.Logging.Abstractions (minimum 1.0.0 requested, resolved
10.0.12), which in turn retains DependencyInjection.Abstractions 10.0.12.
NBitcoin.Secp256k1 has no further locked package dependencies in the WabiSabi
project, and is also retained transitively through other managed packages such
as NNostr.Client. None of these dependencies is called or linked by this Rust
module; migrating a codec does not eliminate their other managed consumers.

The pinned published host at 7ae424b5f5f3734ca1870962a2d913769c59b26d has empty
dependencies/dev-dependencies/build-dependencies, and Cargo.lock names only mcw.
The new encoding module itself has no external dependencies. The transitional
managed application and its existing dependencies are explicitly retained until
their respective migrations finish.

## Bounded address-validation incorporation

The assigned follow-up is the actual common production address parser, not a new
CLI or a substitute wallet implementation. The exact deferred patch changes four
paths owned by the host integrator: mcw/src/lib.rs, mcw/src/app.rs,
MagicalCryptoWallet/Mcw/BitcoinAddressValidation.cs and
MagicalCryptoWallet/Extensions/NBitcoinExtensions.cs. It replaces the existing
Network.Parse<BitcoinAddress> call in TryParseBitcoinAddressForNetwork with the
typed native service, followed by Script.GetDestinationAddress(network) on the
validated script bytes. The latter retains managed address-object ownership and
its supported witness-address policy. No managed address text is decoded as a
fallback. Invalid addresses return false; unavailable, failed or malformed service
responses propagate an exception rather than pretending an address was invalid.

AddressParser.ParseBitcoinAddress is the real downstream caller reached by send,
paste, camera and the existing BIP21 parser. AddressParser.Parse still performs
its existing explicit Trim; the common helper and native handler do not trim.
Existing unit callers must bind an explicit service fixture or run under the real
host after incorporation. Unbound callers deliberately fail; retaining a hidden
managed implementation solely for tests would violate the application boundary.
JSON converters, receive formatting, HD keys, transaction parsing and all other
NBitcoin responsibilities are outside this narrow cutover and remain retained.

The handler stays outside the portable domain module. The deferred lib declaration
imports it as bitcoin_address_service; only this transport adapter depends on
bridge::Frame. Operation 0x020C has this version-1 payload contract:

| Payload | Meaning |
| --- | --- |
| Request: network:u8 followed by exact UTF-8 address bytes | 0 Mainnet, 1 Testnet, 2 Testnet4, 3 Signet, 4 Regtest; maximum 90 text bytes |
| Response: 1 followed by scriptPubKey bytes | Exact P2PKH, P2SH or witness program script derived from validated text |
| Response: 0 followed by failure:u16 little endian | 1 format, 2 network, 3 checksum, 4 mixed case, 5 size |
| ERROR frame code 1 and static message | Invalid frame, missing/unknown network or invalid UTF-8; input is never echoed |

Apply only when the QR task is idle and the coordinator dispatches incorporation.
The checked-in patch is pinned to 7ae424b5f5f3734ca1870962a2d913769c59b26d.
Run git apply --check before applying it to the latest published host. If exact
shared context has moved, regenerate with:

    python mcw/tests/bitcoin_encoding_prepare_integration.py --base-commit <published-host-commit> --output <review-patch-path>

The generator reads immutable Git blobs, requires unique exact contexts and
does not write shared host/managed files. Review the generated patch; no active QR
checkout was edited to produce or test it. Verification uses a dedicated ignored
checkout with the patch applied and the published handler/test copied in:

    & mcw/tests/bitcoin_encoding_integration_verify.ps1 -IntegrationRoot <private-checkout> -RustToolchain <Rust-1.99.0-bin> -SharedProject <shared-project-root>

The verifier takes one exclusive shared build slot, requires two GiB free memory,
uses one build job, and returns BUILD_SLOTS_BUSY/LOW_MEMORY rather than competing
with peer builds. The temporary managed test project has no PackageReference;
it references the actual core project and links the actual ManagedApplicationHost
source. The probe runs as a synthetic child of the actual mcw executable through
the persistent binary channel. Its retained NBitcoin parser is an independent
test oracle only; production uses the native validator. Neither the test project
nor its managed child executable is a shipping artifact.

Native Linux/macOS runners, host-owner incorporation and final package inspection
remain the integrator's acceptance checks. Windows integration evidence for this
bounded patch cannot by itself establish a five-target native release or removal
of NBitcoin, Secp256k1 or their managed dependencies.

Verified bounded evidence is published in
mcw/tests/bitcoin_encoding_fixtures/address_service_evidence.json. It fingerprints
the seven exact source/tooling files and ten saved evidence files, and records the
actual Windows host binary hash. Native tests used the patched published host
base above; applicability was also checked against published master
b28331b8dfae53acdd1b25c77780f1a47762df2a. The codec blob still has the original
4e59e9210317f6c75f1196d22892202b3c72f62adcf9d05be88c4c3c66925ec0 hash.

- Four handler tests passed in debug and optimized profiles with overflow checks,
  covering all 54 Core valid-address scripts, all five network values and witness
  versions 0 through 16, typed rejection, malformed frames, bounds and redaction.
- Rustfmt and Cargo Clippy -D warnings passed. Cargo metadata contains one mcw
  package and zero external dependencies, including dev/build dependencies.
- The actual core project and production ManagedApplicationHost source compiled
  with zero warnings/errors. The real mcw-to-managed caller probe passed 1,139
  assertions: 54 native valid fixtures with explicit network bytes; 70 invalid
  fixtures across three managed networks; 60 supported address/object cases and
  26 retained unsupported witness cases, including uppercase variants; eight
  lexical comparisons with the retained parser; existing UI trim and BIP21
  behavior; malformed-response and transport failure handling; four malformed or
  unknown requests followed by valid requests; pre-canceled request handling; and
  64 concurrent requests on the persistent pipe. Core's existing TestNet alias is
  patched to TestNet4, so the report records that alias explicitly.
- Windows executable import descriptors are kernel32.dll/KERNEL32.dll,
  shell32.dll, api-ms-win-core-synch-l1-2-0.dll and ntdll.dll. No external runtime
  DLL or companion native shipping executable was introduced by this handler.
- Actual retained managed assets still resolve NBitcoin 10.0.13, Secp256k1 3.1.6,
  Newtonsoft.Json 13.0.4 and Logging/DependencyInjection.Abstractions 10.0.12.

Saved logs and the synthetic managed executable remain only in the ignored
.artifacts/mcw-bitcoin-address-integration/.artifacts/bitcoin-address-evidence
directory. The publication includes the deferred patch and owned handler/tests,
not edits to the integrator's active host/managed checkout. Production integration,
old-implementation retirement and dependency removal are all still false in the
evidence manifest until the integration owner completes those acceptance gates.

## Integration and removal acceptance checks

1. Import the verified module into the one mcw package, retaining empty external
   Cargo dependency sections. Add no separate shipping library, binary or copied
   reference implementation.
2. Run this offline conformance suite and warnings-as-errors checks across the
   five target builds and native test runners. Audit the final mcw executable's
   runtime imports/package contents, including static Windows CRT linkage.
3. Wire bridge operations in the reserved range with explicit typed payloads and
   errors, rejecting transport/response overflow before dispatch. Domain source
   must remain independent of the bridge.
4. Wire actual managed address/data callers, preserving their existing observable
   UI/RPC compatibility policy explicitly. Verify receive/QR text, parsing,
   serializer round trips, error behavior, and retained key/script ownership using
   synthetic data only; no user wallet state or key operations.
5. URI, transaction-wire and compact-filter workers must compile and run against
   this exact published module, rather than a stub or duplicated crypto.
6. Mark only address/data codec responsibility migrated once all corresponding
   production callers use mcw. Retain NBitcoin and Secp256k1 until their unrelated
   callers, locks and packaged references are actually eliminated.
7. Coordinator dispatches the integration request when the QR/application-host
   task is idle. The worker leaves an atomic ignored JSON handoff record; it does
   not interrupt active QR work or edit its shared integration surfaces.
