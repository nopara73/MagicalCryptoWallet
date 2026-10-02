# Isolated wallet verification

The C# integration suite uses isolated Bitcoin Core nodes and synthetic encrypted wallets. Bitcoin Core RPC is confined to test infrastructure; production desktop synchronization uses P2P.

`AutomaticCoinJoinTests` starts seven independent encrypted wallet sessions and a real coordinator arena. Signing a normal payment authorizes automatic CoinJoin for the current run. A subsequent incorrect password is rejected. The test requires the first broadcast to include at least 21 inputs, confirmed private outputs in every wallet, and exactly one output for each queued payment, with a single finished state per payment.

`DesktopLifecycleTests` seeds a synthetic encrypted wallet and launches `test-single-wallet-process.py` when `MCW_DESKTOP_PACKAGE` points to a Windows package. The harness controls only its own processes and observes window visibility, synchronization logs, persisted wallet height, and normal UI shutdown. It verifies hidden startup, foreground activation, hide/reopen, duplicate silent launches, network conflicts, and lock release. It does not register OS startup. The background preference is toggled through the owned Settings window using Windows UI Automation before normal window close.

The Windows harness leases the standard regtest P2P port. An occupied port causes a clear failure; existing processes are never stopped. The C# integration nodes use independent random ports and unique directories. All five platforms run service-level synchronization and authorization checks; the native packaged-window test runs on Windows.

```powershell
dotnet test --project MagicalCryptoWallet.IntegrationTests -c Release --no-progress --no-ansi --output Normal
$env:MCW_DESKTOP_PACKAGE = (Resolve-Path .artifacts/packages/win-x64/MagicalCryptoWallet).Path
dotnet test --project MagicalCryptoWallet.IntegrationTests -c Release --filter-class '*DesktopLifecycleTests' --no-progress --no-ansi --output Normal
```

`RegTestRpcProviders.cs` remains test-only, as enforced by `client-core-policy.json`. Packaged desktop assemblies must not expose Bitcoin Core adapters.
