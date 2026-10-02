# Software wallets only

Create, Import and Recover configure one encrypted software wallet. Setup presents a full-width Create tile above equal Import and Recover tiles. Sending, transaction replacement and cancellation use local passphrase authorization. Automatic CoinJoin, Tor, Chinese password masking and Lurking Wife Mode retain their existing checks and behavior.

Every wallet requires encrypted private-key material and a valid 32-byte chain code. Public account metadata, master fingerprints, derivation paths, encryption and recovery formats are retained. Existing software-wallet JSON containing obsolete `Icon` or `PreferPsbtWorkflow` properties is accepted; these fields are ignored and omitted on the next save.

Hardware wallets, watch-only wallets and Coldcard skeleton files are unsupported. Import validates before committing; startup validates before initializing or moving a setup candidate. Rejection leaves the source, stored files, configuration marker and setup journal unchanged. The recovery screen reports the error. Retry cannot choose a different wallet automatically. Open an unsupported wallet in a compatible application; recovery words may be used to create a software wallet in a separate explicit data directory.

The PSBT preference, export action, dashboard PSBT broadcaster, device address verification and external PSBT import/paste are removed. Raw transaction broadcasting still accepts transaction hex or a text file containing it. Internal PSBTs remain part of unsigned previews and reviewed local signing. An ordinary unsigned preview does not decrypt private keys; final signing validates its own passphrase without modifying the original preview.

## API changes

`getwalletinfo` no longer returns `isHardwareWallet` or `isWatchOnly`. Scheme no longer exports `wallet-hardware-wallet?` or `wallet-watch-only?`. Hardware-specific constructors, model interfaces, setup options, navigation, address operations and wallet-type flags are removed, without constant compatibility stubs.

## Verification and snapshots

`SoftwareWalletTests` uses deterministic synthetic software wallets for rejected imports, startup with and without configuration markers, retries, interrupted setup before and after moving the candidate, constructor validation, obsolete-field import, RPC/Scheme removal, rejected external PSBTs, raw transaction parsing, unsigned previews and passphrase signing. Existing suites cover encrypted creation, recovery, fee replacement/cancellation and automatic CoinJoin.

The build workflow runs unit and Bitcoin Core integration suites, native and managed cryptographic interoperability, packaged-process lifecycle checks and five independent encrypted CoinJoin clients. The removal audit covers tracked source, generated navigation, assembly metadata/resources, symbols and extracted installer/archive payloads. Synthetic Avalonia views include setup, dashboard states, settings, send, receive, recovery and recovery words in both themes at 100%, 125%, 150% and 200%, with accessible setup controls and Enter/Space activation.

Snapshot artifacts are produced for Windows x64, Linux x64/arm64 and macOS x64/arm64. Consult the exact source commit's completed workflow for results and packages. Snapshots are unsigned; production release signing and publication are outside this change.
