# Payment URI migration handoff

State: ready for integration once this handoff is published. This is a verified
domain-module delivery; the managed callers and release host still require the
QR/application-host owner's integration. No package has been removed by this track.

Implementation commit: `bd47d2cc945d3036875e7be17dc6fa09547b4936`.
Publication commit: recorded in the ignored coordination handoff JSON, since a
document cannot contain the hash of its own commit.
Baseline inspected: `82127991068522210cdcf77080dc9b819502e486` (`origin/master`).
Thread: `01a0fc2a-52bd-7461-9c8a-1f15bdc04520` (registry and process thread ID).

## Owned files

- `mcw/src/payment_uri.rs`
- `mcw/tests/payment_uri_semantics.rs`
- `mcw/tests/payment_uri_addresses.rs`
- `mcw/tests/payment_uri_reference.py`
- `mcw/tests/payment_uri_verify.ps1`
- `Contrib/McwMigration/Handoffs/payment-uri.md`

No Cargo manifest, `lib.rs`, host/bridge/command, managed adapter, package/launch
file or shared migration ledger was edited. The module is intended to be compiled
inside the single `mcw` application, with no extra shipping crate or executable.
Temporary rustc harnesses and executables live only in this checkout's ignored
`.artifacts/payment-uri-evidence` directory.

## Internal API

Import `crate::payment_uri` alongside the first-party `crate::bitcoin_encoding`.
`BitcoinAddressValidator` implements `AddressValidator` using exactly
`bitcoin_encoding::address_decode(&str, Network) -> Result<Address, Error>`.
The production adapter returns a checked address and typed error rather than a
Boolean; it never infers a network from an address. `Network` is owned by `bitcoin_encoding` and
includes Mainnet, Testnet, Testnet4, Signet and Regtest.

```rust
let result = payment_uri::parse_input(
    text,
    selected_network,
    &payment_uri::BitcoinAddressValidator,
    payment_uri::ParsingMode::ManagedCompatibility,
)?;
// ParsedDestination::{Address, PaymentUri}; no transaction or protocol action.
```

Use `parse` for strict BIP21 URI input, `parse_compatible` for retained managed URI
semantics, or `parse_with_mode` for an explicit mode. `parse_input` is the retained
bare-address/URI entry point: it trims surrounding whitespace and applies the
1,000 UTF-16-code-unit limit used by the managed AddressParser.

`PaymentUri<A>` holds a `ValidatedAddress<A>`, `PaymentDetails`, opaque optional
parameters, and the exact parsed URI. `destination().text()` preserves the original
address, including uppercase witness QR spelling; `destination().address()` carries
the actual checked address, and `network()` carries the selected network. For the
managed app's canonical address representation, explicitly call
`bitcoin_encoding::address_encode(request.destination().address())`; this emits
lowercase witness text while leaving the original request available.

`PaymentDetails` contains optional `Amount`, UTF-8 label and UTF-8 message.
`new_request(address, network, details, validator)` constructs a validated receive
request. `to_uri()` formats only retained amount/label/message fields, omitting
opaque optional extensions. `to_uri_with_optional_parameters()` is an explicit
metadata-preserving serializer; it never activates an extension. Both return a
typed `FormatError` if the canonical encoding would exceed the input limit.
There is no trailing `?` for a parameter-free request. Present zero amounts and
present empty labels/messages remain distinct from missing values.

`Amount::parse_btc`, `Amount::from_satoshis`, `satoshis` and `to_btc` implement
decimal BTC/satoshi conversion using integers only. The valid range is
0..=2,100,000,000,000,000 satoshis. There is no rounding or floating point.
At most eight fractional digits are allowed; signs, exponents, grouping separators,
non-ASCII digits and whitespace are rejected. BIP21's `.1` and `1.` grammar is
accepted and canonicalized as `0.1` and `1`. Nine fractional digits are rejected
even when the last digit is zero. Leading decimal zeros are accepted.

`percent_encode` produces RFC3986 unreserved ASCII plus uppercase `%HH` escapes of
UTF-8 bytes, using `%20` for spaces. `percent_decode` decodes once, validates UTF-8,
rejects malformed escapes, and bounds its allocations. Codec error offsets are
documented as component-relative byte offsets.

## Semantics and retained compatibility

Both modes accept mixed-case `bitcoin:` schemes while preserving payload case and
content. Strict BIP21 mode treats keys as case-sensitive, keeps literal `+`, and
requires raw query syntax to be RFC3986 ASCII; non-ASCII metadata must be encoded.
ManagedCompatibility preserves the retained parser's ASCII case-insensitive keys,
case-insensitive duplicate/`req-` checks, form-style `+` spaces, and raw spaces/UTF-8
in values. Encoded `%2B` remains a plus in both modes. Values are never lowercased.
ManagedCompatibility is the integration choice for existing caller continuity.

Decoded duplicate names, including encoded aliases such as `am%6funt`, are rejected
before they can overwrite data. Unknown optional fields are preserved as opaque
metadata, including the distinction between a bare flag and an empty value. Empty
query segments are ignored. Empty names and missing `=` for label/message are
rejected. A bare or empty amount is rejected as missing. All `req-` names are
unsupported, including `req-amount` and `req-label`, matching the retained parser.

Malformed escapes, invalid UTF-8, fragments, authority (`bitcoin://...`), address
repair and fractional-satoshi rounding are intentionally rejected. These are
explicit hardening changes from implicit System.Uri/HttpUtility/Money behavior.
Address validation occurs before query validation, preserving existing error
precedence for a wrong-network or invalid address. Error codes 1–9 and generic
messages match the retained managed parser; 10 and 11 cover the existing entry
point's length/empty-input errors. Messages do not embed supplied URI data.

No PayJoin, silent-payment, BIP321 alternative destination or Lightning behavior
exists. Optional `pj`, `pjos`, `sp` and `lightning` are opaque BIP21 unknown fields
with only the checked on-chain address used as the destination. A missing address
is rejected even if an alternative destination field is present. The retained
formatter drops these fields, as the current managed Address formatter does.

Mainnet mismatches are rejected. Testnet/Testnet4/Signet share the `tb` witness HRP;
non-main networks share legacy prefixes. Address encoding cannot distinguish these
shared-prefix networks; the explicitly selected network is preserved. Regtest
witness requests require `bcrt`. Checksums, witness versions, program sizes and
padding are checked exclusively by the first-party address service.

## Reserved bridge operation proposal

Range `0x0400–0x04FF` is reserved for this track. The host owner chooses the actual
binary framing, network IDs, error mapping and message schemas.

| Proposal | Responsibility |
| --- | --- |
| `0x0400` | Parse destination input with explicit network and parsing mode |
| `0x0401` | Format a validated retained request from integer satoshis/metadata |
| `0x0402` | Parse decimal BTC to exact integer satoshis |
| `0x0403` | Format integer satoshis as canonical decimal BTC |
| `0x0404` | Percent-encode UTF-8 metadata |
| `0x0405` | Percent-decode metadata with explicit URI/form mode |

No bridge operation was implemented in a shared host file. A bridge must return
the typed validation/error results and use integer satoshis, not binary floats.

## Verification and independent evidence

The actual peer address source was snapshotted in this checkout for tests, with
SHA256 `4e59e9210317f6c75f1196d22892202b3c72f62adcf9d05be88c4c3c66925ec0`.
Published address implementation commit: `6365f3244d23b801c0bb36581967caad8aae968e`.
That worker owns its implementation and separate published handoff at
`Contrib/McwMigration/Handoffs/bitcoin-encoding.md`; this track claims only the URI
integration tests. No stub was used for the real-address suite.

Verified on Windows x64 using Rust 1.99.0, edition 2024:

- 21 URI/amount/address integration tests plus the peer module's internal SHA
  boundary test: 22 passed in the combined harness.
- 14,102 independent differential cases: 6,026 decimal parses, 5,009 satoshi
  formats, 1,011 percent encodes, 1,028 URI percent decodes and 1,028 form decodes.
  Oracle: Python standard-library Decimal and urllib.parse with strict UTF-8;
  fixed seed `11608324`. This is test tooling, not an application dependency.
- Exact range/sub-satoshi boundaries, Unicode and delimiter fuzz combinations,
  30,002 amount round trips, retained managed address fixtures, BIP350 valid/invalid
  addresses, real Base58Check failures, uppercase QR spelling, and synthetic
  witness versions 0–16 on every supported network were covered.
- `rustfmt --check`, `rustc -D warnings`, and Clippy all with `-D warnings` passed
  for the combined actual source, without style-lint suppression.
- A static-CRT test executable imports only Windows OS DLLs:
  `api-ms-win-core-synch-l1-2-0.dll`, `bcryptprimitives.dll`, `KERNEL32.dll`,
  `ntdll.dll`, `USERENV.dll`. This is harness evidence, not a shipping-host audit.

Debug and optimized Rust test harnesses both passed, as did the independent
14,102-case oracle in both builds. The optimized run against the final address
source completed its 22 Rust tests in 0.12 seconds. The URI source SHA256 was
`46ab76e8cc25dc1d5f1d40c10ac1d56b8ae236a89919bef8ddf954a1321ed5cd`.

Only the Windows target standard library was available in the shared toolchain at
verification time. Linux x64/ARM64 and macOS x64/ARM64 compilation/runtime results
are not claimed. This source has no target conditionals, unsafe/native bindings or
OS handles; the host's five-target release verification remains an acceptance
check for integration.

Reproduce after the address service is in the checkout, from a Windows developer
PowerShell with native MSVC/SDK library paths configured:

```powershell
& .\mcw\tests\payment_uri_verify.ps1
& .\mcw\tests\payment_uri_verify.ps1 -Optimized
```

Before integration, pass `-BitcoinEncodingPath` pointing at the actual address
worker source. The runner records its source hash, copies it into this checkout's
ignored evidence directory, holds one of the two shared build-slot locks, checks
free memory, uses one build job, and generates only temporary rustc harnesses.
For a shell without a VS developer environment, supply `-NativeLibraryPaths`.
Verified native library directories on this machine were MSVC
`14.51.36231/lib/onecore/x64` and SDK `10.0.26100.0/{ucrt,um}/x64`; these are build
tools and introduce no runtime dependency. No compiler installation was changed.

The ordinary Cargo tests after host integration are:

```text
cargo test --manifest-path mcw/Cargo.toml --offline --test payment_uri_semantics
cargo test --manifest-path mcw/Cargo.toml --offline --test payment_uri_addresses
```

These Cargo commands are proposed integration checks, not pre-integration results:
the baseline remote checkout had no Cargo host or module declarations. The host
owner's then-current worktree manifest had empty dependencies/dev-dependencies/
build-dependencies tables, but its uncommitted manifest is not delivered by this
track. This track adds no manifest, external Cargo dependency, native library,
runtime installer, C#/Avalonia/IPC dependency or companion shipping executable.

Reference sources inspected on 2026-10-02:

- [BIP21](https://github.com/bitcoin/bips/blob/master/bip-0021.mediawiki): URI
  syntax, UTF-8 encoding, decimal amounts, required/optional keys. BIP21 is now
  superseded; the assignment deliberately retains its feature scope.
- [RFC3986](https://www.rfc-editor.org/rfc/rfc3986): query grammar and percent
  encoding. This module does not implement general-purpose URL resolution.
- [Bitcoin Core v29.0 URI tests](https://github.com/bitcoin/bitcoin/blob/v29.0/src/qt/test/uritests.cpp):
  independent decimal examples and invalid commas. Core's duplicate-last-wins and
  required-known-key behavior are not copied because retained MCW rejects both.
- [Bitcoin Core v29.0 amount limits](https://github.com/bitcoin/bitcoin/blob/v29.0/src/consensus/amount.h):
  COIN and MoneyRange.
- [BIP350](https://github.com/bitcoin/bips/blob/master/bip-0350.mediawiki): published
  witness address fixtures used against the actual peer validator.

Downloaded reference snapshots and SHA256 values, verification output, static
imports, and source hashes are in the ignored evidence directory and coordination
JSON. BIP21's deliberately invalid example address is explicitly rejected by the
real validator; a syntax-only fixture never counts as address validation.

## Remaining production callers and retained dependencies

The host integrator must migrate the retained managed chain:

- `MagicalCryptoWallet/Userfacing/Bip21/Bip21UriParser.cs`: System.Uri,
  HttpUtility.ParseQueryString, NBitcoin Money and network/address parsing.
- `MagicalCryptoWallet/Userfacing/AddressParser.cs`: bare-address/URI dispatch,
  retained amount/label representation, Uri.EscapeDataString formatter, canonical
  address display, and `NBitcoinExtensions.TryParseBitcoinAddressForNetwork`.
- `MagicalCryptoWallet.Fluent/Behaviors/PasteButtonFlashBehavior.cs`: clipboard
  destination validation.
- `MagicalCryptoWallet.Fluent/ViewModels/Dialogs/ShowQrCameraDialogViewModel.cs`:
  decoded QR destination validation.
- `MagicalCryptoWallet.Fluent/ViewModels/Wallets/Send/SendViewModel.cs`: destination
  validation and filling amount/label from a request.
- `MagicalCryptoWallet.Fluent/ViewModels/Wallets/Send/RecipientRowViewModel.cs`:
  destination validation and existing rejection of BIP21 in multi-recipient flows.
- `MagicalCryptoWallet.Fluent/ViewModels/Wallets/CoinJoinPayment/AddCoinJoinPaymentViewModel.cs`:
  destination validation and filling a BIP21 amount.

NBitcoin 10.0.13 remains referenced by
`MagicalCryptoWallet/MagicalCryptoWallet.csproj` and centrally versioned in
`Directory.Packages.props`. Other live callers include KeyManager, WalletGenerator,
WpkhOutputDescriptorHelper, TransactionStore/TransactionSummary, P2pBehavior,
CompactFilterBehavior and WabiSabi models/protocol code. Replacing this feature
does not remove NBitcoin, NBitcoin.Secp256k1 or their transitive responsibilities.

System.Web.HttpUtility also remains used by
`MagicalCryptoWallet/Discoverability/CoordinatorConnectionString.cs`.
The managed GUI/runtime is a separately tracked transitional component. This
track has not changed or removed any of those upstream usages or dependencies.

## Integration and removal acceptance checks

1. Use the published first-party address service; wire `payment_uri` into the
   permanent mcw host with no external dependency tables populated.
2. Run the independent runner against the integrated actual address source and
   both ordinary Cargo integration test targets on the final host revision.
3. Bridge the managed feature using explicit selected networks, integer satoshis,
   ManagedCompatibility, stable error mapping and canonical-address conversion.
   Retain original request metadata separately and preserve multi-recipient URI
   rejection, clipboard/QR validation and amount/label autofill.
4. Verify receive/format and send/parse round trips with synthetic data; ensure
   optional extensions cannot execute or replace the on-chain destination.
5. Remove the managed BIP21 parsing/encoding implementation only after every
   caller above uses the first-party service. Update the shared migration ledger
   with actual caller removal evidence, while keeping NBitcoin/HttpUtility marked
   retained for unrelated live callers.
6. Complete Windows/Linux/macOS target, packaging and runtime audits on the final
   one-executable application. Component evidence alone is not a release claim.
