# Bounded SafeFile writer handoff

This component replaces only the file-operation responsibility of `SafeFile.SafelyWriteAllText` and `SafelyWriteAllBytes`. It does not replace SQLite or acquire wallet state, keys, serialization, schema or read/recovery ownership. The revoked broad SQLite draft remains untouched in `.artifacts/mcw-storage-20261002` and is excluded from this handoff.

**Production integration is pending.** Master retains its existing SafeFile writer until the QR integrator applies the coupled registration and caller patch. No dependency is removed by this component. In particular, all SQLite/NBitcoin packages and the managed reader remain required.

The coordinator assigned `mcw/src/platform/safe_file/` to this existing bounded assignment with `MCW-SAFEFILE-PLATFORM-LEAF-20261002`. Implementation files are `mcw/src/safe_file_service/`, that disjoint native leaf, and `MagicalCryptoWallet/Mcw/Storage/McwSafeFile.cs`. No shared Cargo/lib/app/platform declarations, packaging or ledger files were edited.

The service preserves the existing names and sequence: ensure the directory, open/truncate `.new` with read sharing, write caller-owned bytes, flush/close it, delete a previous `.old` if the main file exists, move main to `.old` without overwrite, move `.new` to main without overwrite, and delete `.old`. Native Windows deletion deliberately refuses a read-only file like .NET File.Delete. The existing managed reader still chooses main versus `.old` and `.new`; abort/disconnect only closes the temporary stream. It never promotes partial bytes, deletes a recovery artifact or falls back to managed file writing.

The adapter keeps .NET encoding/BOM behavior, including directory creation before argument validation, short encoding errors after `.new` opens, large-input encoding validation before opening, and encoder state across 8192-character chunks. Large text reserves allocation without exposing zero-filled logical content; a fatal reservation failure closes/deletes `.new` like .NET. Unix applies the advisory shared lock before truncation, honors the framework disabling setting and skips NFS/SMB/CIFS shared write locks. Relative paths resolve in the caller's directory after appending `.new`. Diagnostics cross the bridge as typed stage/kind/native codes without paths or content.

## Wire contract

All fields are little-endian. Version `u16 = 1` accepts absolute strict UTF-8 paths. Windows BEGIN/PREPARE also accept version `2` with raw UTF-16LE code units, including unpaired surrogates; the managed Windows adapter uses this form without encoding replacement. Path length is the byte count, at most 128 KiB; paths contain no NUL. Odd UTF-16 byte counts, relative paths, unsupported versions and version-2 non-path requests are rejected. Unix retains version 1 and the framework's replacement fallback when converting managed path strings to UTF-8. All responses and APPEND/COMMIT/ABORT requests remain version 1. There are at most 16 open sessions and 256 KiB per data chunk. The shared bridge remains the sole framing/lifetime owner.

| Operation | Payload after version | Result |
| --- | --- | --- |
| `0x1004 PREPARE` | path length `u32`, path bytes | Create containing directory before caller validation; no stream |
| `0x1000 BEGIN` | expected size `u64`, reservation size `u64`, disable-file-locking flag `u8` (`0`/`1`), path length `u32`, path bytes | Open `.new`, reserve only for large text, return token `u64`; `u64::MAX` requires an explicit final size |
| `0x1001 APPEND` | token `u64`, exact offset `u64`, chunk bytes | Write only to `.new` |
| `0x1002 COMMIT` | token `u64`, final size `u64` | Verify exact completeness before the legacy rename/cleanup sequence |
| `0x1003 ABORT` | token `u64` | Close only; keep artifacts for the retained reader |

Responses contain version `u16`, status `u8` (`0` success or `1` error), then a BEGIN token or error stage `u8`, kind `u8`, native code `i32`. The service also accepts a token-only COMMIT for an exact-size session. It rejects malformed payloads, unknown tokens, offsets, overflowing/excess lengths and late writes after connection closure.

Host-facing API remains `Dispatch::new`, `request(request_id, operation, payload)`, `cancel(request_id)` and `close()`. The added `cancel_pending(operation, payload) -> bool` closes an existing token carried by a queued version-1 APPEND/COMMIT/ABORT that has not reached the dispatcher. The host must retain that queued payload when handling its CANCEL; merely passing the new request ID cannot find the earlier BEGIN session. It must also close on EOF/error/shutdown. Framing and queue lifetime remain integrator-owned.

## Shared cutover

`mcw/src/safe_file_service/integration.patch` is based on published `f5af3f9d616faac27b3250308cc980155bed921a`. It proposes four coordinated changes: register the domain module in `lib.rs`, register the native leaf in `platform.rs`, give each `app.rs` host connection one stateful dispatcher with CANCEL/SHUTDOWN/EOF/error/drop cleanup, and switch the existing SafeFile writer to the adapter. Review/reconcile it against the latest master instead of copying a stale shared file. Apply the caller change only together with working host registration.

The production leaves are unchanged `KeyManager.ToFileNoLock` and the existing block-header shutdown write in `MagicalCryptoWallet.Client/Global.cs`; both already call SafeFile. This avoids introducing any second wallet/state owner.

An additional shared prerequisite is a real Rust service binding for managed wallet tests. Existing KeyManager tests call `ToFile()` without `McwApplicationServices.Bind`; an unconditional helper cutover would fail those tests with an unavailable connection. The adapter intentionally has no managed I/O fallback. Those complete wallet tests and the shipping five-target/import/package checks are not certified by this component evidence.

For Cargo CI after registration, add a root integration-test wrapper that includes `tests/safe_file/safe_file_tests.rs`. The fixture directory is deliberately not auto-discovered before the shared module registration. The application dispatch tests in `safe_file_host_tests.rs` need a test-only child module of `app.rs` (as demonstrated by the ignored verification source copy).

## Verification

`mcw/tests/safe_file/safe_file_verify.ps1` contains the reproducible Windows developer workflow and takes a coordinator build slot. It records source hashes under ignored `.artifacts/safe-file-evidence/`. It generates the exact proposed managed caller in an ignored copy, so the checks also run before production activation; the retained reader stays in that copy unchanged. The original helper snapshot is a development-only first-party reference, not a production fallback. Python/.NET drivers and the ignored source copy do not ship and are not evidence of a shipping runtime-import audit.

The October 3 Singapore audit reproduced and fixed two compatibility defects: Windows unpaired-surrogate paths succeeded in the original helper but failed during strict UTF-8 conversion, and large-text preallocation called the string byte-count overload rather than the framework's span overload. The generated caller SHA-256 remains `1479a6f8add2075d48e5ab2b387ac8a22769be1e93d91fd2a4ce8b8de77d4d10`, matching the actual private caller used in the prior run. Exact source hashes, test output and synthetic corpus results remain under the ignored evidence directory. The component verifier tests the proposed registration from this isolated source; the integrator must verify its newer queue/transport composition separately.

Final audit reproduction: `run-20261002T162159291Z`, foundation source base `f5af3f9d616faac27b3250308cc980155bed921a` plus the owned audit changes. Direct driver requests: 7,580; the actual bridge repeats the same 307 cases.

Verified Windows development checks: Rust 1.99.0 with warnings denied and Clippy (including unsafe-block documentation); thirteen native service tests; 307 original-helper differential cases for bytes/artifacts and exception type/parameter/HRESULT; nine owned process-interruption boundaries; two actual application dispatch CANCEL/SHUTDOWN tests; and the same 307 differential cases through the published managed application host with the proposed Rust registration. Encoding failures, empty/BOM content, surrogate chunk boundaries, >1 MiB content, all artifact combinations, directory collisions, read-only cleanup and preallocation cleanup/length are covered. The added cases include 64 text/byte writes using raw UTF-16 surrogate leaf/parent names across all artifact states, span byte-count selection, malformed UTF-16 payloads, and canceled queued APPEND/COMMIT/ABORT tokens.

Linux/macOS runtime parity, shipping import/package verification and physical power-loss durability remain unverified. File flushes, macOS F_FULLFSYNC, Unix directory sync and Windows write-through moves are implemented; process interruption tests do not certify storage hardware persistence.

Readiness flags: `production_integrated=false`, `old_implementation_retired=false`, `dependency_removed=false`. This is a component plus a coupled cutover patch awaiting shared integration and its verification.
