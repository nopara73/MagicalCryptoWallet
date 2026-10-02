# Single-wallet storage and API migration

MCW opens and synchronizes its configured wallet whenever the application starts, including silent desktop startup. The dashboard opens directly, shows placeholders until local data is known, then shows cached balance/history while synchronization continues. Detailed progress and retry appear in the expandable status surface. Protected operations validate their own passphrases. Any successful authorization also authorizes CoinJoin for this application run; automatic CoinJoin starts when synchronization and send/shutdown restrictions allow. This never skips authorization for later spending or private information. Chinese password masking and Lurking Wife Mode remain supported.

## Storage and setup

One wallet is configured per active network and data directory. New creation, recovery, hardware connection, and explicit import create unsaved drafts and commit `Wallet.json` through a serialized, recoverable journal. Import sources stay unchanged; destination collisions and concurrent setup cannot overwrite stored keys or accept a second wallet.

Existing filenames and the `.wallet` marker stay unchanged. A compatibility reader honors that marker, otherwise adopts the former last-used file if it exists, then the first filename in ordinal order. Additional files remain untouched. Missing, corrupt, invalid, or traversing configured paths produce a recovery error; another file is never substituted. Interrupted setup only rolls forward a file matching the recorded hash.

Filenames are storage details. There is no naming, renaming, switching, replacement, or manual loading UI. Use a fresh explicit `--datadir=<path>` and first-run setup for an independently configured wallet. Mainnet uses `Wallets/`; other networks use `Wallets/<network>/`.

## RPC

Every application wallet operation uses the root endpoint, such as `http://127.0.0.1:38128/`. Retired methods, old name arguments, and named endpoint paths fail explicitly.

| Interface | Current contract |
|---|---|
| `createwallet` | Required `password`; configure and start automatically. |
| `recoverwallet` | Required `mnemonicStr`, optional `password`; configure and start automatically. |
| `getwalletinfo` | Works before setup and during startup; reports state, cached-data availability, synchronization, heights, account coverage, and CoinJoin authorization. |
| `loadwallet` | Removed. Observe readiness instead. |
| `walletName`, `loaded` | Removed from wallet information. |
| Wallet URL paths and CLI selection arguments | Removed. The configured session is implicit. |
| Signing/private operations | Validate the supplied passphrase for each operation. CoinJoin authorization cannot authorize unrelated requests. |

An initial status response includes:

```json
{
  "state": "Unconfigured",
  "hasCachedData": false,
  "synchronized": false,
  "syncHeight": null,
  "targetHeight": null,
  "coinJoinRequiresAuthorization": false,
  "publicMetadataRequiresAuthorization": false,
  "error": null,
  "balance": null,
  "accounts": []
}
```

`balance` is in satoshis and remains `null` until public local state is initialized. A numeric balance with `synchronized: false` is cached. Heights may be unavailable. `Ready` means known accounts have caught up; a legacy account needing metadata authorization never reports complete synchronization. Mutations fail clearly while unready, offline, or unauthorized. CoinJoin authorization lasts until process shutdown and does not cause hidden startup prompts.

Scripts should poll status with a deadline, stop on `Unconfigured`, `Faulted`, or `Stopping`, and fail on timeout. The bundled payment helpers use a two-minute readiness deadline, prompt without echo for protected operations, and check RPC errors. `wcli.sh --json` reads a sensitive JSON request from stdin so passphrases need not enter command history or process arguments. Use `MAGICALCRYPTOWALLET_DATADIR` or `MAGICALCRYPTOWALLET_CONFIG` to choose the explicit context.

## Scheme and diagnostics

`(wallet)` accesses the configured wallet. `(wallet-status)` exposes the session snapshot and `(wallet-info)` works before setup. `open-wallet`, `__start_wallet`, and wallet-name accessors are removed. Diagnostics no longer take wallet-name arguments. Transaction graph generation still takes its transaction ID.

Bitcoin Core retains its own wallet-management commands in test infrastructure. Regtest CoinJoin participants are independent single-wallet clients, each with its own data directory and RPC port; they do not represent multiple wallets in one MCW process.

See [architecture and deletion summary](SingleWalletArchitecture.md) for lifecycle, authorization, CoinJoin coordination, and activation behavior.
