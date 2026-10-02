# Bounded Nostr event-ID replacement

## Scope and activation gate

This replaces only canonical NIP-01 serialization and SHA-256 in the retained update client's authentication guard. The exact production leaf is `MagicalCryptoWallet/WebClients/MagicalCryptoWalletNostrClient.cs`. New implementation leaves are `mcw/src/nostr_event_id.rs` and `MagicalCryptoWallet/Mcw/Nostr/McwNostrEventId.cs`; tests and the two explicit patches below are owned by the WebSocket worker. The existing WebSocket wire codec remains a separate checkpoint.

The first publication adds the unused leaf/adapter and reviewed patches. **The live caller is not yet replaced by that publication.** QR owns shared `mcw/src/lib.rs` and `mcw/src/app.rs` and must incorporate `Contrib/McwMigration/Patches/nostr-event-id.patch` when safely idle. The worker then activates its exact caller with `nostr-event-id-caller.patch` and verifies the incorporated host/caller. The scoped `Patches/.gitattributes` keeps only these two patch assets in LF on Windows, so their context matches the repository's LF C# source. This ordered additive cutover avoids calling an unsupported host operation. It is not a package-removal or application-release claim.

The broad `mcw/src/nostr_service/*` and `McwNostrUpdateClient.cs` drafts remain preserved, unpublished, and unregistered under the human scope correction. Their six hashes are recorded in ignored `.artifacts/mcw-websocket-evidence/preserved-drafts.json`; they must not be compiled or incorporated for this assignment. Full relay, networking, Schnorr, or update-service rewrites are excluded.

## Retained responsibilities

NNostr.Client 0.0.55 continues NIP-19 `npub` decoding with curve validation, relay subscriptions/event deserialization, relay state and publication. The retained managed NBitcoin.Secp256k1 backend still parses/validates the public curve point and signature and performs BIP340 verification. .NET WebSocket/TLS, update-channel/EOSE behavior, trusted author/kind/subscription filtering, release version/tag parsing, manifest requirements, repository destination checks, deduplication and downstream update selection/download behavior remain with their existing implementations. All related package references and locks remain.

The pinned [NNostr source](https://github.com/Kukks/NNostr/tree/46fc4020b381e6581a5d65ff52a16aa8208dd79c) establishes that `Verify()` recomputes the canonical digest. Consequently the supplied caller patch removes both `ComputeId()` and `Verify()` from the chosen guard. The patched guard computes the Rust digest once, compares its lowercase hexadecimal spelling to the event ID with ordinal equality, and supplies those same 32 bytes directly to the existing BIP340 backend. There is no legacy digest fallback or production shadow call in that patched flow. The legacy methods appear only as independent test oracles/producers and in separately retained NNostr callers such as release signing/publishing. Activation on master remains gated as described above.

## Typed operation 0x0C00

Request payload, little endian:

| Field | Encoding |
| --- | --- |
| Payload version | u8, exactly 1 |
| Public key | exactly 32 bytes; adapter accepts lowercase 64-character hex |
| Created timestamp | i64 Unix seconds |
| Kind | i32, preserving the retained representation; live caller filters kind 1 |
| Tag count | u32 |
| Each tag | u32 string count, followed by each string |
| Each string | u32 UTF-8 byte length followed by bytes |
| Content | one final string |

Success is exactly 32 raw digest bytes. The shared patch returns existing bridge error code 1 with a static message for invalid requests. No event content is logged by the leaf. The request cap is 1,048,560 bytes, matching the existing one-MiB frame minus its sixteen-byte header. Counts are checked against remaining bytes before allocation, strings require valid UTF-8, versions/truncation/trailing bytes are rejected, and worst-case canonical escaping is capped at `6 * MAX_REQUEST_BYTES + 256` bytes. JSON depth, nodes, entries, decoded bytes and strings have explicit limits consistent with the request cap rather than smaller generic defaults.

Rust constructs the ordered six-element [NIP-01 tuple](https://github.com/nostr-protocol/nips/blob/master/01.md), uses the actual first-party JSON module's Compact/Minimal serialization, then the actual first-party `bitcoin_encoding::sha256`. It introduces no JSON escaping, hash, Bech32 or curve implementation. The JSON owner confirmed the stable public API and reuse; NIP-19 remains outside this chosen scope.

Compatibility preserves tag order and duplicates without sorting, all tag elements, lowercase pubkey representation, integer timestamps without floating-point conversion, no Unicode normalization, minimal JSON escapes, literal slash/angle brackets/apostrophe/U+2028/U+2029/noncharacters and UTF-8 supplementary scalars. The managed adapter preserves existing nullable timestamp/content defaults (0/empty), omitted null tag identifiers and empty null data strings. It also mirrors NNostr's UTF-8 replacement behavior for malformed managed UTF-16. The actual caller's existing acceptance/rejection policy remains authoritative for tags and release metadata; this hash leaf does not add a timestamp trust policy.

The existing managed host timeout/cancellation/shutdown behavior applies. Failure, cancellation, invalid replies or an unavailable host never use NNostr hashing. The retained caller's existing exception guard rejects the event. A synchronous event callback waits for the host's bounded request; the actual managed host independently drains the reply pipe.

## Verified candidate evidence

Component source was checked with Rust 1.99.0, edition 2024, warnings denied, Clippy all denied and rustfmt. There are 12 leaf tests plus one existing SHA-256 length-atomicity test: **13 debug and 13 optimized tests pass, none ignored**. Tests include independent frozen NIP-01 literals/digests, escaping, tag order/duplicates, all truncated prefixes, UTF-8 invalidity, counts, trailing bytes, integer boundaries, exact maximum request/worst-case escaping and maximum empty-tag count.

Verifier provenance was reconciled by actual reruns in both the worker and isolated candidate checkouts. The published verifier at commit `d40c3908d7475afeced8400b73c8ca804ae8db24` has SHA-256 `0c4d57c76de13af424fc1aedb808d6988e569bb23de6cca6e6e896047d2e95cf`; its exact Git-blob bytes were checked before execution, and both new records pin that hash. Each rerun passed formatting, warnings, Clippy, 13 debug tests, 13 optimized tests and the independent oracle. The other seven recorded component source hashes are unchanged.

The earlier records pinned verifier SHA-256 `5442ed9e445d79b0290cbf9df70ccff8ab4b589d93ed5eab776fa614f94cb480`. That version predates adding `--config skip_children=true` to two formatter invocations. Those flags were checked separately after the earlier full runs, so those records did **not** establish execution of the final published script. Both complete earlier evidence trees were archived byte-for-byte before rerunning; their hashes were not rewritten. A clearly labelled reconstruction of the earlier script removes exactly those two flag pairs and matches its recorded SHA. Archive manifests, the extracted published script and immutable new-run snapshots are in `.artifacts/mcw-websocket-evidence/nostr-provenance/20261002T130143Z-a139b537/`. Current component records now reflect completed executions of the final script, without changing the existing host candidate evidence or claiming activation.

The independent Python stdlib JSON/UTF-8/hashlib/struct oracle passes **1,467 comparisons** (1,353 valid and 114 malformed), including **all 1,112,064 Unicode scalars** in bounded disjoint pages. The large maximum-payload case compares the full independent digest; ordinary cases compare canonical bytes and digest. Input SHA-256 is `38c7526343dc7f8bda5ec0081803e5defc2cd581ff2bd47be7ec08c6072da95c`; expected and actual output SHA-256 are `9b49463a65c123f7a514368dbdf5c6bc0da7707e00a3678cd1f4e1cc611ee549`.

A clean isolated candidate at published master `7ae424b5f5f3734ca1870962a2d913769c59b26d` (containing QR foundation `989cf2a2df22d23837c1aa328e29abfd33c9b9c8`) was compiled with only these owned leaves plus the proposed shared/caller patches. The **actual mcw debug executable, actual retained core project/update caller, and actual ManagedApplicationHost source** ran locally with a synthetic test child and relay-event emitter. Core and test compilation completed with zero warnings/errors. It passed **62 pinned-NNostr digest comparisons, 12 signature checks, 16 retained-caller scenarios and 6 host/failure checks**. Cases cover null/timestamp/UTF-16 edge behavior, real synthetic signatures, invalid curve points, modified signatures/content/authors, ID casing, kind/subscription filters, missing/duplicate/empty metadata, repository destination trust, deduplication, old timestamps, pre-cancelled/malformed/oversized requests, continued operation after rejection and no fallback after host disposal. The cancellation case uses an already-cancelled token before request transmission; cancellation during computation and request timeouts were not exercised and remain unverified. No real wallet, credentials, relay connection, HTTP request, release publication or installation was used.

Test child files under `mcw/tests/nostr_host` are **test artifacts only**, not new shipping executables or dependencies. Legacy canonical hashing used by the independent test oracle never substitutes for the Rust operation. Cargo dependency/dev-dependency/build-dependency sections remain empty. Native evidence is Windows x86_64 debug integration only. Custom-runtime Release execution/packaging, five-target acceptance, real relay behavior, regular-suite actual-host binding/execution, successful whole-application CI and NNostr package removal remain unverified. QR owns foundation CI repairs and shared test-entry coordination independently.

Ignored evidence locations:

- `.artifacts/mcw-websocket/.artifacts/nostr-event-id-verification/{verification.json,oracle.json,debug-results.txt,optimized-results.txt}`
- `.artifacts/mcw-nostr-verify-c6303cc043cf/.artifacts/nostr-event-id-host-verification/{verification.json,host-results.json,managed-build.txt,host-run.txt}`
- `.artifacts/mcw-websocket-evidence/nostr-integration-root.json`
- `.artifacts/mcw-websocket-evidence/nostr-provenance-current.json` and its referenced `manifest.json`, earlier evidence archives and final-script rerun snapshots

## Reproduction and incorporation

Use the installed toolchain; do not install/copy one. Every build is single-job and holds one of the two shared FileShare.None build-slot locks with at least two GiB free. Publication/reconciliation holds the exclusive `git-publish.lock`, verifies the shared/private indexes are empty and stages only intended owned paths. Do not edit the active QR checkout or force-push.

```powershell
# Component, in the source checkout:
& ./mcw/tests/nostr_event_id_verify.ps1
# QR incorporates the shared patch, respecting its exact-file ownership:
git apply --check --unidiff-zero ./Contrib/McwMigration/Patches/nostr-event-id.patch
git apply --unidiff-zero ./Contrib/McwMigration/Patches/nostr-event-id.patch
# Worker activates its caller only after the host operation is incorporated:
git apply --check --unidiff-zero ./Contrib/McwMigration/Patches/nostr-event-id-caller.patch
git apply --unidiff-zero ./Contrib/McwMigration/Patches/nostr-event-id-caller.patch
# In an isolated checkout with both patches and the actual published host:
& ./mcw/tests/nostr_event_id_host_verify.ps1
```

Portable component verification uses `rustc --edition=2024 --test mcw/tests/nostr_event_id_conformance.rs`, both default and optimized profiles, and a compiled `nostr_event_id_oracle.rs` passed to `python mcw/tests/nostr_event_id_reference.py --binary <path> --output <evidence-dir>`. Host verification uses the existing platform host/managed adapter; this Windows script is not a substitute for other-target runtime evidence.

Before marking this bounded assignment complete, verify published host registration/dispatch, activate the exact retained caller, run actual-host synthetic checks on the incorporated source and ancestry-verify the activation commit on remote master. This first publication remains a verified incorporation candidate until those gates pass.
