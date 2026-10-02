# Test-only HMAC caller proof

This project compiles the actual retained domain classes, typed adapter and managed
host transport. It does not ship, implement HMAC, inspect wallets or add a Cargo
package. The verifier creates an ignored project from `HmacProbe.csproj.inc` and
`packages.lock.json.inc`, which describe the existing core's retained dependencies.
These templates leave the application's project/package inventory unchanged.

Before running, apply `wallet-hmac-callers.patch` and `wallet-hmac-host.patch` from
`Contrib/McwMigration/Handoffs` in an isolated verification checkout. The shared
host owner controls actual incorporation. The verifier rejects unchanged managed
HMAC callers and never patches source itself.

On Windows, run `./Contrib/Mcw/HmacProbe/verify.ps1`. It acquires an exclusive shared
build slot, requires 2 GiB free RAM and uses single-job .NET builds and the existing
Rust 1.99/MSVC tools. `-SharedRoot` and `-Python` select existing tools. Optional
`-ManagedHostSource` and `-RustSourceRoot` select reviewed snapshot source; such runs
are marked as staged evidence. Release the build slot before review or publication.

The verifier builds a test-only Rust host into ignored artifacts and copies this
probe as its managed daemon child. The probe exercises 441 offline independent
fixtures and public SLIP19/SLIP21 paths through 462 actual domain calls. Six adapter
fault checks, three actual-caller boundary checks and 37 scripted transport fault
checks cover rejection, cancellation, error redaction and buffer lifetime. The
scripted replies are dummy fault buffers; successful HMAC outputs come from Rust.
Real malformed and canceled Rust requests must leave the same connection usable.

Evidence is written under `.artifacts/wallet-hmac-evidence/caller-probe` with source,
fixture and binary hashes. No supplied payload goes through CLI arguments or logs;
only synthetic fixtures, file paths and test counts are used. Best-effort clearing
does not prove formal erasure or constant-time behavior.

The ignored host uses a tooling-only static CRT and does not set the production
Windows runtime cfg. This proves Windows composition only. It does not prove the
shipping runtime, packaging, five native targets or published caller activation.
Regular wallet unit/integration suites still require a real host binding; the
shared test-launch/CI owner must provide it atomically with activation. There is
no managed hash fallback.
