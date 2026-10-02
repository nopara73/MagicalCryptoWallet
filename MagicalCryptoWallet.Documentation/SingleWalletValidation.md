# Single-wallet verification

All destructive and signing tests use synthetic wallets and isolated data directories. No public release, announcement, real wallet, or separate repository is modified.

Changes are committed and pushed directly to `master`. The repository's [CI checks](https://github.com/nopara73/MagicalCryptoWallet/actions/workflows/build.yml) contain platform results and downloadable unsigned packages. CI builds Windows x64, Linux x64/arm64, and macOS x64/arm64; each platform runs wallet tests, real packaged-process tests, managed/native cryptography and wire interoperability, source/generated/symbol audits, extracted-package inspection, and actual Avalonia rendering. Linux x64 additionally exercises seven encrypted clients through Send authorization and a real local CoinJoin. Windows tests the desktop and daemon; other platforms test the daemon.

## Verified application snapshot, 2026-10-02

[Run 37003880808](https://github.com/nopara73/MagicalCryptoWallet/actions/runs/37003880808) completed successfully for application revision `d60e01b1b3e27af4aba789cde8ccddfba11952f9`. Windows x64, Linux x64/arm64, macOS x64/arm64, Nix, and the coordinator container all passed. Packages use the development version `99.99.99` and are unsigned snapshots.

| Verification | Result |
|---|---|
| Complete wallet unit / Bitcoin integration suites | 1,207 / 45 passed |
| Managed cryptography / native-managed interoperability / native CTest | 125 / 34 / 1 of 1 passed |
| Windows packaged application lifecycle | All 15 checks passed, including setup authorization, passwordless synchronization, reorg recovery, independent Send confirmation, offline history, activation, locking, and credential disposal |
| Seven encrypted clients | One confirmed 35-input CoinJoin; every client's measured anonymity score was 7 |
| Send authorization and first-round payments | Correct passwords started automatic CoinJoin; subsequent incorrect passwords were rejected; all seven payments reconciled exactly once |
| Actual application views | 248 captures per platform in both themes at 100%, 125%, 150%, and 200%; Chinese masking and Lurking Wife Mode checks passed |
| Removal, trust, generated-code, resource, symbol, and extracted-package audits | Passed; all 50 retired-interface negative fixtures rejected |
| macOS disk-image creation | Completed and verified; four regression checks cover bounded busy-error retries, unrelated errors, persistent failure, and invalid images |

Accepted GUI authorization and passwords supplied through RPC, creation, or recovery retain separate CoinJoin credentials for that process. Dismissing an authorization dialog during password derivation does not authorize CoinJoin. Every later signing or private-information operation checks its own password; stopping the process clears retained credentials.

The Windows ZIP and MSI were also downloaded in the background and independently checked against the package inspection report: all 451 payload files and package hashes matched. SHA-256: ZIP `d4caeb42f4566c03c13e4461ff76834ee5f2ff999bbcc646e9571407607e7bc4`; MSI `23fb5fb83f363773f9356bfc1558d10697b42abe5f937ce302ef14462797a7d8`.

## Automatic CoinJoin verification, 2026-10-02

| Verification | Result |
|---|---|
| Complete wallet unit suite | 1,194 passed |
| Setup/operation authorization, actor, RPC, legacy-file and round-cache regressions after reconciling `master` | 72 passed |
| Bitcoin integration suite | 44 passed |
| Seven independent encrypted regtest clients | One 35-input CoinJoin broadcast and confirmed; every client's measured anonymity score was 7 |
| First-round payments | All seven reconciled exactly once in the confirmed CoinJoin |
| Managed cryptography / native-managed interoperability / native CTest | 125 passed / 34 passed / 1 of 1 passed |
| Avalonia CoinJoin views and controls | 48 captures, both themes at 100%, 125%, 150% and 200%; pause/resume and critical-phase progress checks passed |
| Source, active generated code, assembly, symbol and package audits | Passed |
| Bash helpers and Python harness syntax | Passed |

The seven-client harness uses the fixed target and minimum input count without policy overrides. Encrypted startup requires authorization, Send starts automatic CoinJoin, and a later incorrect passphrase is rejected. All signing and broadcast verification uses synthetic wallets and a local regtest node.

## Previous single-wallet baseline, 2026-10-02

These results describe the preceding single-wallet change. The software-wallet-only change is validated separately by the same platform workflow plus the rejection tests and UI checks described in [SoftwareWallet.md](SoftwareWallet.md).

| Verification | Result |
|---|---|
| Complete wallet unit suite | 1,139 passed |
| Bitcoin Core integration suite | 44 passed, including reorg retirement with normal and canceled shutdown waits |
| Desktop activation regression after transport fixes | 2 passed |
| Forked managed cryptography and published vectors | 125 passed |
| Native/managed wire interoperability | 34 passed |
| Native CTest | 1/1 passed |
| Actual packaged Windows process lifecycle | All 11 checks passed |
| Avalonia views | 128 captures, both themes at 100%, 125%, 150% and 200% |
| Source/generated/assembly/symbol/package removal audits | Passed |

The process checks cover encrypted first-run setup, zero-height regtest, synchronization without authorization or a visible window, cached offline balance/history, network recovery, operation authorization followed by a wrong passphrase, duplicate silent launches, same-process activation, hide/reopen, exact-network and daemon conflicts, credential reset, clean exit and lock release. Screenshots contain only synthetic masked data. The visual checks include unknown/syncing/offline/faulted dashboards, keyboard access, Chinese masking/IME, first-frame Lurking Wife Mode, and automatic coin selection.

CoinJoin tests cover the fixed anonymity target of 2, batches up to ten wallet inputs, first-round payments, advertised and actual 21-input boundaries, fee and balance safeguards, immediate authorization/resume, quiet completion, new funds and payments, independent send/shutdown restrictions, pause, and failure backoff. Legacy settings import/save and rejected obsolete RPC/CLI parameters are covered. The seven-client process harness requires at least 21 actual inputs, confirmed mixed outputs for every client, and exactly-once payment reconciliation; it fails if a prior successful Send bypasses a later incorrect password.

Final lifecycle regressions drain queued reorg and mempool work before closing SQLite, reject late callbacks, and retire idle mailbox workers. Each test process has a separate synthetic data root. P2P tests wait for Core's compact-filter index before requesting freshly mined filters. Lock-time boundary tests reject future heights and unsigned underflow on short chains.

Production platform certificates were not exercised; packages are unsigned snapshots, not production releases. Installer coexistence runs only on the ephemeral Windows CI runner, without changing this machine's installed wallets or startup preferences.

The removal audit rejects retired types, generated navigation, wallet-indexed orchestration, name routing, old commands, and logged-in authorization shortcuts. Exact compatibility, migration-documentation, negative-test, and Bitcoin Core fixture exceptions are recorded individually in `Contrib/SingleWallet/exceptions.json`.

## Reproduce

```powershell
dotnet test --project MagicalCryptoWallet.Tests -c Release --filter-namespace '*UnitTests*' --no-progress --no-ansi --output Normal
dotnet test --project MagicalCryptoWallet.IntegrationTests -c Release --no-progress --no-ansi --output Normal
dotnet test --project ThirdParty/WabiSabi/csharp/WabiSabi.Tests -c Release --no-progress --no-ansi --output Normal
dotnet test --project ThirdParty/WabiSabi/interop/WabiSabiInterop.Tests -c Release --no-progress --no-ansi --output Normal
dotnet run --project ThirdParty/WabiSabi/interop -c Release
dotnet run --project Contrib/VisualPreview -c Release -- .artifacts/screenshots
python Contrib/SingleWallet/audit.py --artifacts .artifacts/packages/win-x64/MagicalCryptoWallet
python Contrib/Tests/test-single-wallet-process.py --package .artifacts/packages/win-x64/MagicalCryptoWallet --bitcoind MagicalCryptoWallet.IntegrationTests/bin/Release/net10.0/BundledApps/Binaries/win-x64/bitcoind.exe
python Contrib/Tests/test-single-wallet-coinjoin.py --package .artifacts/packages/win-x64/MagicalCryptoWallet --bitcoind MagicalCryptoWallet.IntegrationTests/bin/Release/net10.0/BundledApps/Binaries/win-x64/bitcoind.exe
```

See [the test harness](../Contrib/Tests/README.md), [architecture](SingleWalletArchitecture.md), and [API migration](SingleWallet.md).
