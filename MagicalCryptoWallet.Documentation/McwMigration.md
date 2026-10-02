# Dependency migration ledger

The reproducible package/edge/role inventory is [dependencies.json](../Contrib/McwMigration/dependencies.json). Generate it with `python Contrib/Mcw/dependencies.py`; CI's `--check` rejects stale records. It includes every direct reference, transitive framework-lock package and version, central unused pins, copied source, native libraries, bundled executable hashes and libraries embedded in bundled programs. Mutable per-RID lock sections duplicate the framework graph; extracted package audits verify the target-specific payloads separately. Native/bundled versions are platform-specific; a Windows Tor version is not evidence for the other four packages.

Executable and library hashes describe exact file bytes. License notices use UTF-8 with LF line endings so Git's platform-specific text checkout does not change their inventory; each record states its hash scope. Repository dotfiles are excluded from bundled runtime payloads.

## Current ownership

| Responsibility | Preserved behavior and compatibility | Rust ownership/interface | Status and required evidence |
|---|---|---|---|
| Application lifetime | Flags, silent startup, instance activation, exit codes, app/data identity, restart, crash reporting, authenticated installer handoff | `app`, `command`, `platform`; bridge lifecycle operations | Implemented host. Managed startup, instance lock and wallet cleanup remain transitional. Five-target process/package checks are the release gate. |
| QR generation | Exact decoded text, all ECCs, versions 1–40, URI/Unicode, async receive UI, cancellation | `qr::encode`; bridge operation 1; managed bool[x,y] adapter | Gma.QrCodeNet copied encoder removed. Accept independent decoding, capacity/malformed tests, source/assembly/package audit, rendered and exported four-module-margin PNGs. |
| PNG/rendering/camera decoding | Native Save dialog, PNG export, sharp modules, camera scan and orientation | Receive control remains managed, acquired-image decoding now uses Rust | Avalonia, Skia and camera capture remain. QRackers supplies both the FlashCap capture APIs and the ZXing test-oracle namespaces, so the package remains until capture migrates. |
| Formats/parsers | JSON exact numbers/legacy converters; Bitcoin encodings, URIs, transactions/PSBTs, config semantics | Future bounded portable services; immutable typed values, no platform handles | Newtonsoft/System.Text.Json, NBitcoin format callers remain until every caller is migrated. Use published vectors and differential tests; reject unintended format changes. |
| Wallet state, cryptography and recovery | Keys, encrypted storage, address derivation, signing, fee selection, automatic CoinJoin, recovery formats and authorization | Future wallet owner with typed commands/events; reviewed first-party crypto in mcw | NBitcoin/Secp256k1, WabiSabi source/native library/embedded secp256k1 remain. Require independent vectors, synthetic regtest, recovery and crash-safety evidence before ownership transfer. |
| Persistent storage | Existing wallet/config/SQLite formats and journal recovery; no data changes in this milestone | Future single storage owner with transactional/crash-safe boundary | SQLite and managed serializers remain. Prove read/write compatibility, interrupted writes, corruption handling and export/recovery before switching. |
| Synchronization and privacy | P2P/filter synchronization, reorg/broadcast, coordinator wire behavior, private Tor routes, direct public-data routes and proxy isolation | Future owned network connections and privacy services, preserving the documented [routing policy](NetworkRouting.md) | Managed HTTP/Bitcoin/Tor clients and bundled Tor remain. Tor includes Libevent, OpenSSL, zlib and platform native payloads; migrate these capabilities into mcw as well. Prove routes, isolation, deadlines, reconnect/reorg and leak behavior. |
| Hardware integration | Device discovery, user-approved signing, address confirmation and recovery compatibility if reintroduced | Future native OS USB/HID bindings, device protocol services, typed approval boundary | Current branch removed hardware/watch-only/HWI support. Do not restore it as part of QR work. Future hardware scope needs product compatibility decisions and a fresh HWI/device/Python/native inventory. |
| Native UI | Established interaction, privacy masking, accessibility, themes, scaling, clipboard, notifications and dialogs | Future native platform controls bound directly to application services | Avalonia/ReactiveUI/DynamicData/Skia/HarfBuzz/Inter/fonts/native backends remain. Verify real desktop UI; remove bridge only after the final managed caller is gone. |
| Installation and launch | Existing MSI/DEB/AppImage/DMG identities, startup/relaunch paths, exact arguments and packaged resources | `mcw` is the payload entrypoint; future platform packaging must preserve these contracts | AppImage's outer ELF runtime and generated AppRun remain separate dependencies. Its upstream static link declares Squashfuse, libfuse3, Zstandard, zlib and mimalloc, built with musl. Verify the actual embedded source/version and component versions per package before declaring this wrapper removed. |
| External coordinator | Existing WabiSabi/coordinator protocol and authenticated trust | External service, outside client executable ownership | ASP.NET/coordinator dependencies retain their explicit external-service role. |
| Verification and releases | Independent decoders, Bitcoin Core regtest, signatures, platform packages | Test/build tooling is separate from shipping mcw | xUnit/coverlet/Roslyn/SDK/CMake/WiX/Python/Nix and Bitcoin Core remain tooling/oracles. Embedded Bitcoin Core libraries are tracked as verification-only; do not infer their removal from a client migration. |

## Removal rule

The [current incorporation record](../Contrib/McwMigration/incorporated-flows.json)
records fourteen bounded flows whose callers now use the permanent application
host. PSBT metadata, ownership/SLIP21 MACs, staged safe-file writes, client round
fingerprints, fee content decoding, the application HTTP factory contract, Tor
control/readiness, acquired-image decoding, release Markdown, address validation,
block cache header identity, client script text, filter matching and Nostr event
IDs have coupled native registration and real caller verification. Each entry
identifies the preserved managed authority and dependency roles. The exact-source
five-target workflow is still required for every release qualification.

The [integration handoffs](../Contrib/McwMigration/Handoffs) preserve historical
component-only evidence. Current incorporation status comes from the current
record and dependency inventory; a historical pending flag does not describe the
later composition. JSON caller replacement, payment URI, PNG, native transport,
wallet/recovery state, SQLite, Tor runtime and whole native UI remain awaiting
migration. No component implementation by itself is dependency removal.

The Markdown package closure and the unused HTTP factory package are retired
from current source references, restored lock graphs and Nix inputs only after
the audit confirms their absence. Packaged-target audits enforce absence of their
assemblies. Avalonia/Skia/QRackers (embedded FlashCap), NBitcoin/Secp256k1, Newtonsoft/NNostr,
SQLite, bundled Tor and the managed framework retain their other responsibilities.
The external coordinator's HTTP/Tor/reference hashing roles remain explicit.

The safe-file replacement preserves main/.new/.old formats and the managed
reader's recovery choice. Synthetic interruption tests and native flushes do not
certify physical power-loss persistence. The replacement owns byte writes and
temporary streams; it does not acquire wallet schema or recovery authority.

The later daemon/automation API removal superseded the proposed RPC migration.
Retiring its unused adapters and activation patches is obsolete-code removal;
it does not replace Newtonsoft.Json. The generic Rust JSON component remains
prepared, with retained production callers and dependencies still recorded.

The AppImage launcher is shipping code, while `appimagetool` is a build tool.
The [upstream runtime build](https://github.com/AppImage/type2-runtime/blob/8f39b89e2ac31e1640b3d3f7e9a5108e6ce805fa/src/runtime/Makefile)
declares its static implementation libraries; these are outside the `mcw` ELF
runtime-import audit. [appimagetool downloads a separate runtime](https://github.com/AppImage/appimagetool/blob/main/README.md)
unless supplied `--runtime-file`, which the current packaging command does not
provide. Consequently, its pinned tool checksum does not establish the embedded
runtime's identity. The inventory records this retained wrapper and its declared
upstream dependencies without treating that source revision as proof of the
current package's embedded versions. Whole-package hashes and extraction tests
remain the current package evidence; runtime-specific source/version attribution
is a separate requirement for future replacement.

Mark a dependency removed only when **all callers, copied source, transitive inclusion, generated bindings, native imports and packaged references are gone**. Check every shipped target, including dependencies statically embedded in Tor/native credential libraries and those carried by NuGet native bundles. Preserve upstream license notices where attribution remains applicable.

Record a migration's source commit, portable API, managed callers switched, input/output compatibility, independent standards/vectors, synthetic behavior tests, runtime graph and extracted package evidence. A codec that is merely compiled into `mcw` is “implemented, callers retained”; it is not package removal. Keep error, cancellation and ownership contracts versioned during transition. Reserve operation ranges through the host owner before wiring a new service; never create a second IPC connection or executable for a migrated dependency.

## Verification locations

- `mcw` Rust tests cover capacity boundaries, deterministic symbols, fragmented/invalid frames and UTF-8/content rejection.
- `Contrib/Mcw/BridgeProbe` plus `test-host.py` exercise the real managed adapter/root executable, 160 version/ECC independent decodes, Unicode/emoji/URI/numeric maxima, multiple pending calls, cancellation, lifecycle handoff, EOF and parent cleanup.
- `Contrib/VisualPreview --qr-only` checks the actual receive control/PNG, orientation, four white modules, integral scaling, opaque black/white pixels and independent decoding.
- Packaged synthetic-wallet tests cover normal/silent startup, activation, desktop locking, data paths and recovery; they inspect the host's owned process tree.
- The five-platform build workflow is the shipping gate. Local Windows evidence is not proof of completed Linux/macOS builds or a signed production release.
- `Contrib/Mcw/test-migrations.py` verifies actual bounded callers against the supplied shipping native binary, with source and binary hashes. The retained managed suites also run through that binary; they have no fallback implementation.
- The content portable host proof requires a private cancellation reply with positive partial-work counters. Its schedule search and deterministic current-Inbox component proof remain separate evidence.
