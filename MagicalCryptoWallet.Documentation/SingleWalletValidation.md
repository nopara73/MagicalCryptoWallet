# Single-wallet verification

All signing and destructive checks use synthetic wallets and isolated directories. The [build workflow](https://github.com/nopara73/MagicalCryptoWallet/actions/workflows/build.yml) builds Windows x64, Linux x64/arm64, and macOS x64/arm64, and provides exact-commit results and unsigned packages.

Every platform runs wallet unit and Bitcoin Core integration suites, managed/native cryptography and interoperability, source/generated/assembly/symbol audits, extracted-package inspection, and actual Avalonia rendering. The integration suite runs a real coordinator with seven encrypted wallets. Each synchronizes without authorization, then authorizes by signing a normal payment. The first broadcast CoinJoin must contain at least 21 inputs, confirmed private outputs for every wallet, and all seven queued payments exactly once. Incorrect later passphrases remain rejected.

Windows additionally seeds an encrypted synthetic wallet and runs the packaged desktop. Observations cover hidden P2P synchronization, no authorization window, duplicate silent launches, foreground activation of the same process, hide/reopen while synchronization continues, network conflicts, normal UI shutdown, and lock release for a new process. The fixture disables startup registration and uses isolated regtest storage. Existing user scripts and encryption remain intact.

Direct wallet-service tests cover cached offline state, synchronization, authorization, automatic input selection, operation signing, creation/recovery, and CoinJoin restrictions. Startup-registration coverage remains. Visual checks cover both themes and common display scales, first-frame privacy masking, dashboard states, keyboard navigation, and absence of the retired action from search and navigation.

Configuration tests preserve version 4 and retained settings when obsolete automation fields are enabled or malformed; new saves and help omit them. Removal audits inspect tracked source, fresh generated code, dependencies, assembly metadata and symbols, and packaged payloads.

## Reproduce

```powershell
dotnet test --project MagicalCryptoWallet.Tests -c Release --filter-namespace '*UnitTests*' --no-progress --no-ansi --output Normal
dotnet test --project MagicalCryptoWallet.IntegrationTests -c Release --no-progress --no-ansi --output Normal
# Set this only for the packaged Windows desktop lifecycle test:
$env:MCW_DESKTOP_PACKAGE = (Resolve-Path .artifacts/packages/win-x64/MagicalCryptoWallet).Path
dotnet test --project MagicalCryptoWallet.IntegrationTests -c Release --filter-class '*DesktopLifecycleTests' --no-progress --no-ansi --output Normal
python Contrib/Mcw/run-managed-tool.py --host .artifacts/packages/win-x64/MagicalCryptoWallet/mcw.exe --project Contrib/VisualPreview -- .artifacts/screenshots
python Contrib/SingleWallet/audit.py --artifacts .artifacts/packages/win-x64/MagicalCryptoWallet
python Contrib/AutomationRemoval/audit.py --artifacts .artifacts/packages/win-x64/MagicalCryptoWallet
```

Consult the exact commit's completed workflow for results. Snapshot packages are unsigned; production signing and release publication are separate.

See [test infrastructure](../Contrib/Tests/README.md), [architecture](SingleWalletArchitecture.md), and [compatibility](MagicalCryptoWalletCompatibility.md).

## Previous verified application snapshot, 2026-10-02

This historical snapshot precedes the automation removal and describes its original verification interfaces.

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

That snapshot's GUI authorization, creation/recovery, and retired automation interfaces retained separate CoinJoin credentials for that process. Dismissing an authorization dialog during password derivation does not authorize CoinJoin. Every later signing or private-information operation checks its own password; stopping the process clears retained credentials.

The Windows ZIP and MSI were also downloaded in the background and independently checked against the package inspection report: all 451 payload files and package hashes matched. SHA-256: ZIP `d4caeb42f4566c03c13e4461ff76834ee5f2ff999bbcc646e9571407607e7bc4`; MSI `23fb5fb83f363773f9356bfc1558d10697b42abe5f937ce302ef14462797a7d8`.

## Previous CoinJoin strategy verification, 2026-10-02

These component results precede this interface removal. The historical shell harness has since been replaced by the C# suite.

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
