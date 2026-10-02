# Single-wallet verification

All destructive and signing tests use synthetic wallets and isolated data directories. No public release, announcement, real wallet, or separate repository is modified.

The repository's [CI checks](https://github.com/nopara73/MagicalCryptoWallet/actions/workflows/build.yml) contain the platform results and downloadable unsigned packages. CI builds Windows x64, Linux x64/arm64, and macOS x64/arm64; each platform runs wallet tests, real packaged-process tests, managed/native cryptography and wire interoperability, source/generated/symbol audits, extracted-package inspection, and actual Avalonia rendering. Linux x64 additionally exercises five encrypted clients through Send authorization and a real local CoinJoin. Windows tests the desktop and daemon; other platforms test the daemon.

## Single-wallet baseline results, 2026-10-02

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

CoinJoin tests cover concurrent starts, fresh trackers after cancellation, overlapping send/shutdown restrictions, canceled restarts, automatic authorization after Send, manual pause and disabled automatic settings. The five-client process harness requires confirmed mixed outputs for every independent client and fails if a prior successful Send bypasses a later incorrect passphrase.

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
