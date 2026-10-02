# JSON engine integration handoff

Status: implementation and independent component verification complete;
production caller integration and package removal remain pending. This is an
internal module for the one `mcw` executable, not a new Cargo package, library
distribution, managed service or companion executable.

Owner: JSON worker `01a0fc27-0df2-75e3-b877-1ab40561bb59`.
Coordinator: `01a0fc1e-7c20-76d3-bf81-cb1f68c9adb7`.
Integrator: `01a0fbf5-89e2-7e90-9b98-50e3ff9bb5bc`.
Implementation baseline: `82127991068522210cdcf77080dc9b819502e486`.
Implementation commit: `d25363713c1dd270c131c4ead17c9ef1140221fe`, pushed directly
to origin master and verified against its remote ref. This document is published
in a subsequent handoff commit; its exact revision is recorded in the shared
ready-for-integration handoff JSON.

## Owned files and dependencies

- `mcw/src/json.rs`, `mcw/src/json/compat.rs`: complete portable implementation.
- `mcw/tests/json_conformance.rs`: tests compile the actual module by path.
- `mcw/tests/json_verify.py`, `json_managed_reference.cs`: repeatable independent
  verification; executable harnesses/projects are generated under ignored
  `.artifacts/json-verification`, never as another shipping package.
- `mcw/tests/json_vectors/`, `json_vectors.sha256`: pinned reference fixtures,
  provenance/license and byte-preserving Git attributes.
- `mcw/tests/json_verification.json`, `json_caller_inventory.json`: source hashes,
  results and managed caller snapshots.
- This handoff document. The ignored shared `handoffs/json.json` announces the
  verified publication; only the coordinator dispatches incorporation.

Implementation imports only Rust `std` and its own `compat` child. It has no
external crates, floats, unsafe code, native handles, platform bindings, managed
types, IPC or runtime installers. No Cargo manifest, package lock, `lib.rs`,
command/bridge/host, managed adapter, production caller or wallet file was edited.

Read-only `cargo metadata --offline --locked` of the current QR-owner host found
one `mcw` package and zero dependencies; its manifest/lock hashes are recorded in
the evidence. That provisional owner snapshot is not this worker's completed
application integration. NBitcoin 10.0.13 and its resolved Newtonsoft.Json
13.0.4 remain in managed `MagicalCryptoWallet/packages.lock.json`. Central
Newtonsoft 13.0.3 is a minimum pin; it is not the resolved version.

## Interface and representation

The integrator adds `pub mod json;` in its own module declarations. No other
module is needed beyond the owned `json/compat.rs` child.

| API | Contract |
| --- | --- |
| `parse(&[u8], &ParseOptions) -> Result<Document, Error>` | Exactly one UTF-8 root value; primitive roots are valid. Rejects malformed UTF-8 before interpreting text, extra root values, invalid grammar and resource excess. |
| `Document::{source, root, into_root, extensions_used}` | `source()` replays input bytes exactly, including permitted comments/BOM/whitespace. Root is immutable while source is retained. Fresh serialization is a separate explicit operation. |
| `Value::{Null, Bool, Number, String, Array, Object}` | Typed recursive tree; preserves absent-versus-null distinctions and all unknown members. No automatic dates, enum conversion or `$type`/`$ref` execution. |
| `JsonString::{new, as_str, original_token, into_string}` | Decoded Unicode plus optional original validated token. Original escapes/casing survive default serialization. Equality compares decoded scalars without normalization. |
| `Number::{parse, as_str}` | Validated lexical number, including exponent case/sign, trailing fraction zeros and negative zero. Never converts through `f64`, `f32` or a managed float. Equality is lexical. |
| `Number::{to_i128_exact, to_i64_exact, to_u64_exact, to_scaled_i128}` | Checked exact conversions; fractional loss, integer overflow and unrepresentable conversion exponents are distinct errors. `to_scaled_i128(8)` yields exact satoshis, followed by the caller's Bitcoin range checks. |
| `Number::from_scaled_i128`, `From<i128/i64/u64>` | Construct numbers from integers; fixed-point construction retains requested scale and checks token size before allocating padding. |
| `Object::{new, members, into_members, get_unique}` | Ordered members, including preserved duplicates when requested. `get_unique` errors on ambiguous decoded names; it never chooses first/last silently. |
| `serialize(&Value, &SerializeOptions) -> Result<String, EncodeError>` | Entire bounded output buffer or error, never an exposed partial prefix. Deterministic member/array order and exact number spelling. Includes nulls and unknown fields. |
| `Layout`, `StringEncoding` | Compact or 1–8 space/LF pretty layout. Default preserves original string tokens; Minimal/Ascii rewriting is explicit. Ascii emits validated UTF-16 surrogate pairs for non-BMP scalars. |

All nine JSON string escapes work. Literal UTF-8 and escaped scalar values are
validated; high surrogates require a following low surrogate, and lone low
surrogates fail. Unicode noncharacters remain valid scalars. No case folding,
NFC/NFD normalization, key sorting or numeric canonicalization occurs. This is
not an RFC 8785 signed-transcript format; protocol owners define their transcript
encoding separately.

`Error` reports kind, zero-based byte offset and one-based line/byte column (LF
starts a line). Both error types omit input text and keys. Number conversion
errors never round, saturate, guess or repair a value. Numbers with enormous
exponents remain lexically round-trippable even if conversion to i128 fails.

## Duplicate, extension and resource policies

Default parsing/serialization rejects duplicate **decoded** names, so `"a"` and
`"\u0061"` conflict. `DuplicatePolicy::Preserve` keeps every member in order and
sets `extensions_used().duplicate_keys`; it is for explicit archival inspection
or an RFC corpus check. Schema lookup still refuses ambiguity. Preserve is not
an implicit last-wins migration.

Strict defaults reject comments, trailing commas and a leading BOM.
`ParseOptions::legacy_config()` explicitly skips `//`/`/* */` comments, matching
current managed JsonDecoder syntax, and still rejects trailing commas/BOM.
`Extensions` allows comments, trailing commas and a leading UTF-8 BOM separately;
each actually used extension is reported. Single quotes, unquoted keys,
constructors, undefined, NaN/Infinity and locale numbers remain unsupported.
Serializing an extended document's root produces strict JSON; `source()` remains
the original extended text. The caller decides whether to persist such a rewrite.

| Limit | Default | Hard ceiling |
| --- | --- | --- |
| Input UTF-8 bytes | 8 MiB | 64 MiB |
| Output UTF-8 bytes | 16 MiB | 128 MiB |
| Nested containers | 64 | 128 |
| Nodes, including object keys | 100,000 | 1,000,000 |
| Entries per container | 50,000 | 1,000,000 |
| Decoded bytes per string/key | 1 MiB | 16 MiB |
| Number token bytes | 4,096 | 65,536 |
| Total decoded string/key bytes | 8 MiB | 64 MiB |

Zero limits are meaningful; an empty container still has a node and depth, while
a scalar has depth zero. Requests exceeding hard ceilings fail as InvalidOptions
without silent clamping. Limits bound text, nodes, nesting and output, not total
process RSS or caller-created AST size. Standard allocator metadata/collection
overhead remains; fallible String/Vec reservation failures have a typed error,
while Rust's global allocator can still abort on fatal process-wide OOM. Callers
must enforce the body/file limit before accumulating transport input themselves.

## Managed compatibility boundary and remaining callers

`json/compat.rs` implements real checked primitives: exact/initial-Pascal/ASCII
insensitive field matching; required/optional fields with missing distinct from
null; strict string/bool/object access; explicit integer-or-decimal-string input;
exact numeric/string fixed point; and the existing UseTor bool/string union.
Both alias spellings or any duplicate match fail even when values are identical.
It does not invent default values or bypass domain validators.

Current `Encode.MoneyBitcoins`/`Decode.MoneyBitcoins` use **JSON strings**;
MoneySatoshis uses JSON integers. Use `scaled_decimal_string_i128(value, 8)` and
`fixed_point_string(units, 8, true)` to retain that schema, with independent
Bitcoin amount/range checks. The string decoder deliberately accepts plain JSON
decimal spelling and rejects exponent/whitespace/plus/locale/leading-zero
leniency from user-input Money.Parse. Existing encoded wallet/config amounts
match that stricter format. These syntax fixtures do not validate key material.

The baseline inventory has 11 Newtonsoft/NBitcoin JSON runtime source files and
24 System.Text.Json/typed JSON source files. The concurrent shared working copy
also contains pending legacy removals, captured separately in
`json_caller_inventory.json`; neither snapshot is an application removal claim.

| Caller group retained | Concrete integration route and acceptance boundary |
| --- | --- |
| `Rpc/JsonRpcRequest.cs`, `JsonRpcResponse.cs`, `JsonRpcRequestHandler.cs`, all five `Rpc/JsonConverters/*.cs` | Strict parse into Value; explicitly validate JSON-RPC envelope, batch, method, params and notification rules before dispatch. Retain number/string/null/absent ID types rather than the old string coercion. Build result/error objects in declared field order. Replace uint256/address/destination/outpoint/transaction converters with first-party Bitcoin domain codecs; case-insensitive OutPoint property matching is available with collision rejection. Method dispatch/auth/session ownership stays with its owner. |
| `Client/Bootstrap.cs` Scheme `ToJson`, plus Fluent Scheme console | Domain adapter constructs acyclic Value trees, explicitly omits nulls only where existing Scheme settings require it, uses pretty layout, and applies declared depth/domain policies. Preserve unsupported-type/Script capability policy separately; JSON introduces no reflection or object activation. Replace all registered NBitcoin converter routes before removing settings. |
| `Wallets/Exchange/ExchangeRateProvider.cs` | Replace the four fixed JsonPath strings with checked Object member/array index access for `.USD.buy`, `.USD`, `[0].current_price`, `.bid`. Quote number/string format is provider-specific and exact; do not embed an unnecessary general JsonPath interpreter. Keep HTTP/Tor/fallback policy in the owner. |
| `BitcoinRpc/RpcClientBase.cs` and NBitcoin RPC responses | Replace JToken fee-array/object access with exact typed members, or accept the client-Core cleanup owner's verified removal. Regtest/server/test callers must be audited before declaring this path dead. NBitcoin remains for its many non-JSON Bitcoin callers. |
| `Serialization/Primitives.cs`, `Client/Configuration/{Serialization,PersistentConfigManager}.cs`, `Fluent/UiConfig/{Serialization,UiConfig}.cs` | Use legacy_config for persisted input, then explicit version/default/enum schema adapters. Retain UseTor bool/string, config-version rules, decimal fee limits and string BTC amounts. Replace current Integral's GetDouble conversion with exact integer helpers; the managed oracle confirms 9007199254740993 currently becomes 9007199254740992 in that float path. UI geometry conversion belongs to UI code, never amounts. No automatic file rewrite. |
| `Blockchain/Keys/KeyManager.cs`, `Serialization/{Bitcoin,Client,Config}.cs`, wallet import/discovery/setup journals and `WalletDirectories.cs` | JSON parses syntax only; wallet owner validates encrypted secrets, exact chain-code sizes, public keys, fingerprint/key paths, version/network, heights, labels and coinjoin costs using first-party codecs. Preserve local-private-key restrictions and atomic storage/journal ownership. No real wallet file was read or changed by tests. |
| `Serialization/{Coordination,WabiSabi,External}.cs`, `WabiSabi/Client/WabiSabiHttpApiClient.cs`, `Extensions/{HttpContent,HttpResponseMessage}Extensions.cs`, coordinator JSON input/output formatters and `WabiSabi/Coordinator/WabiSabiConfig.cs` | Explicit whitelisted discriminators (`Type`), Pascal aliases, array limits and exact integer/decimal fields into first-party protocol types. Replace base64/hex/group-element/proof/transaction codecs with their owning first-party modules; reject alias collisions. Validate date/time/TimeSpan/Guid strings in domain codecs, preserving original JSON strings until that conversion. Keep network/cancellation and cryptographic validation outside JSON. |
| `WabiSabi/Client/Banning/CoinPrison.cs`, `Serialization/Client.cs`, `Models/SerializableException.cs`, fee/CPFP/exchange payloads | Explicit record decoders with date/amount/range checks, nullable inner exceptions and bounded arrays; write only through the storage owner. Fee helpers use exact numbers; string formatting cannot authorize payments. |
| Shared-copy legacy `WebClients/PayJoin/PayjoinClient.cs`, `Helpers/ImportWalletHelper.cs`, Hwi parser JSON, peer cache/setup journal JSON | Respect current feature/storage owners. Refresh remote and live caller graph after their pending removals; route any retained parser to this engine plus its typed domain schema. The worker neither restores removed features nor claims another worker's unpublished removal. |
| `ThirdParty/WabiSabi/csharp/WabiSabi.Tests/Crypto/{StrobeOperation,StrobeTestVector,StrobeTestSet,StrobeTests}.cs` | Newtonsoft test fixtures remain development-only managed callers. Move retained vectors into first-party Rust tests during that protocol migration; do not count a test-package removal as application runtime removal. |

Other current managed consumers such as peer-address caches and setup journals
are System.Text.Json clients rather than Newtonsoft; this engine provides their
syntax foundation but does not acquire their storage or service ownership.
The engine treats ISO dates and `$type`/`$ref` as ordinary data. Newtonsoft's
implicit Date conversion, last-wins keys, quoted/unquoted property leniency,
reference-loop/null omission and runtime type converters require explicit owner
decisions, not switches in the strict wire parser. LF pretty output is an
explicit formatting contract, while the managed Windows formatter can use CRLF.

## Proposed bridge operations

Reserved `0x0100–0x01FF`; proposals only. The integrator owns framing, option
encoding, error-code mapping and production routes. No bridge is implemented or
registered by this worker.

| Proposed operation | Behavior |
| --- | --- |
| `0x0100` | Validate bounded strict UTF-8 JSON and return typed location errors. |
| `0x0101` | Parse then serialize with explicit layout/string/duplicate policy; never invoke a schema or save a file implicitly. |
| `0x0102` | Inspect persisted legacy-comment JSON, returning detected extensions and preserving original bytes for owner-controlled migration. |
| `0x0103` | Exact lexical number to signed integer units at an explicit decimal scale; distinguish range and fractional failure. |
| `0x0104` | Exact integer units to number or decimal-string token with explicit scale/trimming. |

Permanent Rust callers can call the functions directly inside `mcw`; no IPC or
managed assembly is a dependency of the domain code.

## Verification and acceptance checks

Verified with Rust 1.99.0 (`b940084d7`, edition 2024) on Windows x64:

- 29 Rust tests, actual module source, `-D warnings`; rustfmt check and
  clippy::all with warnings denied passed.
- [RFC 8259](https://www.rfc-editor.org/rfc/rfc8259) grammar and the complete
  independently maintained [JSONTestSuite](https://github.com/nst/JSONTestSuite/tree/1ef36fa01286573e846ac449e8683f8833c5b26a/test_parsing):
  95 required accept, 188 required reject, 10 optional accept, 25 optional reject.
  Every accepted vector replays source exactly and reserializes deterministically.
- Every Unicode scalar through Minimal/Ascii serialization and parsing;
  surrogate/invalid-UTF-8/control/escape rejection, duplicate policies, all limit
  boundaries and the non-removable 128-container depth cap.
- 10,055 exact fixed-point results matched Python stdlib Decimal independently;
  i64/u64/i128 and satoshi precision boundaries also have direct Rust tests.
- 12 deliberately synthetic application-shaped payloads matched both cached
  Newtonsoft 13.0.4 (Decimal, DateParseHandling.None) and .NET 10.0.12
  System.Text.Json (comments skip), including order, all members, nulls, strings
  and exact numeric meaning. This is syntax/representation evidence, not full
  application DTO/key/protocol validation or execution against real data.
- 20,000 malformed byte inputs, 1,000 generated trees across six output modes,
  and 6,000 corresponding single-byte mutations ran without panics or
  invalid successful output.
- Windows target metadata/typechecking passed. Linux x64/ARM64 and macOS
  x64/ARM64 std targets are not installed in the shared toolchain; target compile
  and native execution remain unverified. Source has no platform branches or
  handles, but that fact does not certify those platforms.

`json_verification.json` pins source/test hashes and reference DLL/fixture/log
hashes. Downloaded JSONTestSuite bytes were compared with the vendored corpus;
fixtures are test data under the upstream MIT license, not implementation or
runtime dependencies.

From the isolated repository root, with the existing shared toolchain environment
and developer linker environment configured:

```powershell
$env:CARGO_BUILD_JOBS = '1'
python mcw/tests/json_verify.py --rustc "$env:CARGO_HOME/bin/rustc.exe" --rustfmt "$env:CARGO_HOME/bin/rustfmt.exe"
# Optional independent managed oracle; take build-slot-1.lock or build-slot-2.lock
# with FileShare.None and require at least 2 GiB free before the run:
python mcw/tests/json_verify.py --rustc "$env:CARGO_HOME/bin/rustc.exe" --rustfmt "$env:CARGO_HOME/bin/rustfmt.exe" --dotnet 'C:/Program Files/dotnet/dotnet.exe' --newtonsoft-dll 'C:/Users/user/.nuget/packages/newtonsoft.json/13.0.4/lib/net6.0/Newtonsoft.Json.dll'
& "$env:CARGO_HOME/bin/clippy-driver.exe" --edition=2024 --crate-type=lib --emit=metadata .artifacts/json-verification/json-metadata.rs -D warnings -W clippy::all -o .artifacts/json-verification/clippy.rmeta
```

The installed MSVC layout supplies developer libraries in `lib/onecore/x64`,
rather than `lib/x64`. This test run used that directory plus the installed
Windows SDK `10.0.26100.0/{ucrt,um}/x64` in command-local LIB. It did not install
anything or modify shared toolchain/system configuration. Test executables and
managed oracle artifacts are ignored development evidence; the integrator must
audit the final one-executable runtime/import graph after its own host linking.

Before accepting integration/removal:

1. Wire the module and actual production Rust/managed transitional routes only
   in integrator-owned files; retain all current ownership and fail-closed wallet
   authorization/storage rules. Verify the bridge with actual module bytes.
2. Run schema-specific tests for every caller group, including decimals/strings,
   exact RPC ID types, missing/null/default fields, unknowns, case collisions,
   dates/offsets, cryptographic codecs, malformed data and caller body limits.
   Require synthetic import/export preservation and durable journal behavior;
   parser tests alone do not authorize rewriting existing wallets.
3. Run the five-target build/native CI matrix and final package/import audit
   under the one `mcw` executable's platform/allocator/CRT policies. Add no Rust
   crate or third-party runtime to make the matrix pass.
4. Refresh `rg` caller inventory and managed package dependency graphs. Remove
   Newtonsoft settings/types/attributes only after all retained runtime users
   are routed; remove package references/central pins and lock entries only when
   their full transitive graph permits it. NBitcoin's non-JSON callers remain
   outside this track. Keep test-only dependencies labelled accurately.
5. Verify actual integrated production paths and packaging against the published
   implementation source hashes before reporting dependency removal or release.

The coordinator owns idle-only integration dispatch. This worker leaves its
ready handoff and does not send an incorporation request to active QR work.
