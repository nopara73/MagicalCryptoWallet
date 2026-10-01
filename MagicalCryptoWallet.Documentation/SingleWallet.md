# One wallet

Magical Crypto Wallet manages one wallet in the active network's data directory. First-run **Set Up Wallet** offers creation, hardware-wallet connection, explicit JSON import, and recovery. Once setup is complete, the sidebar has one wallet home button; there is no add-wallet entry, wallet list, or switching control. Multi-share recovery backups remain supported.

Coinjoin returns wallet outputs to the same wallet. Payment batching and payments to external Bitcoin addresses remain available. There is no output-wallet selector or cross-wallet sweep.

## Existing application data

On the first start after this change, an installation with several wallet files adopts its last-used wallet from `UiConfig.json`, if that wallet exists in the active network directory. Otherwise it adopts the first filename in ordinal name order. Other wallet files are left untouched. The chosen name is saved atomically in `Wallets/.wallet` (or `Wallets/<network>/.wallet`), so file timestamps, directory enumeration, and later changes to old UI settings cannot change it.

A missing or corrupt configured wallet stops startup with an error instead of silently opening a different wallet. Restore the configured file from a backup. Renaming the wallet updates the saved identity.

Application data stays separate from other wallets' application data. To import or recover a different wallet, use a fresh data directory with `--datadir=<path>` and complete initial setup there. Keep existing wallet files and recovery backups. Do not overwrite a configured wallet to replace it.

## RPC and daemon

The daemon starts the configured wallet automatically. Wallet operations use the root RPC endpoint, for example `http://127.0.0.1:38128/`. `loadwallet` takes no parameters. `createwallet` and `recoverwallet` configure the first wallet and reject further creation before writing a file. Wallet-name URL paths, named-wallet CLI arguments, `listwallets`, and `startcoinjoinsweep` are removed.

The CLI helpers and Scheme `(wallet)` function use the configured wallet directly. The regtest harness runs separate clients with one wallet, data directory, and RPC port each to simulate independent Coinjoin participants.
