# Single-wallet verification

All destructive and signing tests use synthetic wallets and isolated data directories. No public release, announcement, real wallet, or separate repository is modified.

Validation results are recorded here after the final branch build. CI builds Windows x64, Linux x64/arm64, and macOS x64/arm64 packages, checks native and managed cryptography/interoperability, audits extracted packages, renders the actual Avalonia views, and uploads package/verification artifacts. The Windows process test runs the real packaged desktop and daemon against an isolated Bitcoin Core node.

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
```

See [the test harness](../Contrib/Tests/README.md), [architecture](SingleWalletArchitecture.md), and [API migration](SingleWallet.md).
