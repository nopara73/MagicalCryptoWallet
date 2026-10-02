# Bounded HTTP factory contract removal

Worker `01a0fc46-15ed-7fe3-88dc-bba09fad5d45`, slug `http1`.
Assignment: `MCW-BOUNDED-ASSIGNMENT-20261002-http1`.

This change replaces only the redundant application use of
`System.Net.Http.IHttpClientFactory` with the application-owned transition
contract `MagicalCryptoWallet.Mcw.Network.IMcwHttpClientFactory`.
Its single operation remains `HttpClient CreateClient(string name)`.
The existing managed transport, routing, identity credentials, caching/lifetime,
retry/backoff, cancellation, redirects and automatic decompression remain in
their existing implementations. No Rust HTTP parser, TLS, Tor, curve primitive,
host operation or new service is activated by this change.

## Concrete caller replacement

The complete live contract inventory contains these existing source files:

| Role | Caller |
|---|---|
| Core factories | `MagicalCryptoWallet/WebClients/MagicalCryptoWallet/MagicalCryptoWalletHttpClientFactory.cs` |
| Core fees | `MagicalCryptoWallet/FeeRateEstimation/FeeRateProviders.cs` |
| Core exchange | `MagicalCryptoWallet/Wallets/Exchange/ExchangeRateProvider.cs` |
| Core CPFP | `MagicalCryptoWallet/Wallets/CpfpInfoProvider.cs` |
| Core release downloads | `MagicalCryptoWallet/Services/UpdateManager.cs` |
| Core transaction broadcast | `MagicalCryptoWallet/Blockchain/TransactionBroadcasting/TransactionBroadcaster.cs` |
| Core coordinator client | `MagicalCryptoWallet/WabiSabi/Client/WabiSabiHttpApiClient.cs` |
| Client factories | `MagicalCryptoWallet.Client/Global.cs` |
| UI service field | `MagicalCryptoWallet.Fluent/Services.cs` |
| Coordinator DI | `MagicalCryptoWallet.Coordinator/Startup.cs` |
| Test mock | `MagicalCryptoWallet.Tests/UnitTests/Mocks/MockTorTcpConnectionFactory.cs` |

Each of these eleven files changes only the contract type and its namespace
import. Canonical source comparison, reversing those substitutions, matched the
original files exactly. Existing loopback/SOCKS tests also use the new interface
for real synthetic request execution. JSON, Nostr, CoinJoin and Script owners
retain the separate semantic leaves in their files; their interface-hunk
agreements are recorded in coordinator messages. Coordinator delegated only the
two Global properties, one Fluent field and two Startup DI type arguments/imports
after a fresh comparison with the published host foundation.

## QR-owned package patch and retained framework role

`Contrib/McwMigration/Patches/http-factory.manifests.patch` is the exact reviewed
candidate for the QR-owned core PackageReference, central PackageVersion and
affected release lock files. Applying it is a separate integration step; the
source contract commit alone does not remove the package.

The patch touches two manifests and nine lock roles: wallet core, Client,
Fluent, desktop, daemon, integration tests, unit tests, VisualPreview and the
existing Mcw BridgeProbe. Nine restored release graphs remove 57 unused
package nodes in total, with no added packages or changed versions. The probe
role was introduced by the published host foundation and is audited separately
from the earlier eight-role inventory.

Before applying a checked-out patch on Windows, normalize its line endings to LF
in memory or in a temporary copy. Git's default text checkout may turn the patch
itself into CRLF, which causes literal context mismatches. This changes no patch
content. Check it against the latest source before applying; never force a stale
lock-file patch over another owner's graph changes.

The audit found no application use of AddHttpClient, default-factory handlers,
factory options, named-client builders or other package behavior. The current
local factories implement their own behavior already. The only remaining live
framework use is `services.AddHttpClient()` in
`MagicalCryptoWallet.Tests/UnitTests/WabiSabi/Integration/WabiSabiApiApplicationFactory.cs`.
That test-host registration remains unchanged. Coordinator uses the Web SDK's
ASP.NET shared framework, and the test runtime retains
`Microsoft.AspNetCore.App`. The test assembly intentionally still references
`Microsoft.Extensions.Http` from that framework. The change must not be described
as globally eliminating the HTTP assembly, ASP.NET, .NET HTTP transport or Tor.

Release lock verification requires no new packages and no changed package
versions. It removes the redundant HTTP node and, where otherwise unused, its
Configuration, Binder, DependencyInjection, Diagnostics, Logging and
Options.ConfigurationExtensions nodes. Framework/test roles retain their
independently needed dependencies.

## Verification and evidence

All tests use synthetic data and loopback/mocked HTTP. Test processes receive
dedicated APPDATA/TEMP/TMP directories under the worker's ignored evidence root.
No production wallet or public Tor endpoint is accessed.

Pre-foundation candidate evidence: .NET SDK 10.0.401 on Windows x64, Debug and
Release builds with zero warnings/errors, 19 Debug routing/retry tests and 67
Release routing/retry/fee/CPFP cancellation/installer tests passed. These checks
are distinct from the later checks on the published host foundation; do not
label older binaries as foundation verification.

The source candidate was rebased onto published foundation-derived master
`e64c080096a44614a1bfb4778782e8f3d3554c5b`, which contains foundation
`989cf2a2df22d23837c1aa328e29abfd33c9b9c8`. The later actual-source Release
build with the exact shared patch passed with zero warnings/errors. Compiled
core/client/Fluent references contain no Microsoft.Extensions.Http, retain
System.Net.Http, and the test assembly/runtime retains the ASP.NET factory
assembly/framework. Foundation caller test counts are recorded separately in
`verification.json`: 52 selected Release routing/retry/fee/CPFP/installer cases
passed with zero failures. The earlier additional cancellation cases remain
identified as pre-foundation evidence. All QR-owned candidate manifest/lock
bytes were restored after verification; only the exact review patch is part of
the worker's publication.

Evidence root:
`C:/Users/user/OneDrive/Documents/ChatGPT/MagicalCryptoWallet/.artifacts/mcw-http1/.artifacts/http-factory-verification/`.
It contains build/restore/test logs, XML reports in the test output directories,
exact shared-file byte baselines, package graph diffs, compiled assembly
reference inspection and preserved draft hashes. Only actual-source builds are
used; no factory, TLS, native bridge or protocol implementation is stubbed.

Reproduction after applying the exact shared patch uses Release lock files:

```text
dotnet restore MagicalCryptoWallet.Tests/MagicalCryptoWallet.Tests.csproj --locked-mode --disable-parallel -p:Configuration=Release -p:BuildInParallel=false
dotnet build MagicalCryptoWallet.Tests/MagicalCryptoWallet.Tests.csproj -c Release --no-restore -m:1 -p:BuildInParallel=false -p:UseSharedCompilation=false
```

Run the existing TorRoutingTests, RetryHttpClientHandlerTests,
ExternalFeeRateProviderTests, CPFP provider/updater/cancellation tests and
ReleaseDownloaderTests with `--filter-class`, `--parallel none` and
`--max-threads 1`. Restore/check every affected application/probe lock role.
Heavy checks use the shared two-slot lock and at least 2 GiB free memory;
publication uses the exclusive Git lock with only explicit owned paths staged.

## Preserved and excluded work

The old `mcw/src/network_service/**`, `mcw/tests/http1_network.rs` and
`mcw/tests/http1_network_verify.ps1` drafts remain local, unchanged and
unregistered. No field25519, X25519, certificate, TLS or full network migration
work is resumed. The published HTTP wire codec remains a separately verified
checkpoint without a forced production caller.

The compression owner separately owns the bounded decoded-response handler.
The named proposed flow is `MempoolSpace-bitcoin-fee-rate-provider`; its factory
activation requires the real published handler and host dispatch evidence.
This factory-contract change does not enable it, alter decompression or claim
codec ownership. CI repairs and five-target/native packaging acceptance remain
QR-owned; no such pass follows from these Windows managed checks.
