# Single-wallet storage and desktop startup

MCW opens and synchronizes its configured wallet whenever the application starts, including silent desktop startup. The dashboard opens directly, shows placeholders until local data is known, then shows cached balance/history while synchronization continues. Detailed progress and retry appear in the expandable status surface. Protected operations validate their own passphrases. Any successful authorization also authorizes CoinJoin for this application run; automatic CoinJoin starts when synchronization and send/shutdown restrictions allow. This never skips authorization for later spending or private information. Chinese password masking and Lurking Wife Mode remain supported.

Passwords supplied during creation or recovery also authorize CoinJoin for that run, through desktop setup. Importing an encrypted file without entering its password leaves CoinJoin awaiting authorization. Restarting always discards retained authorization.

## Storage and setup

One wallet is configured per active network and data directory. New creation, recovery, and explicit software-wallet import create unsaved drafts and commit `Wallet.json` through a serialized, recoverable journal. Import sources stay unchanged; destination collisions and concurrent setup cannot overwrite stored keys or accept a second wallet.

Existing filenames and the `.wallet` marker stay unchanged. A compatibility reader honors that marker, otherwise adopts the former last-used file if it exists, then the first filename in ordinal order. Additional files remain untouched. Missing, corrupt, invalid, or traversing configured paths produce a recovery error; another file is never substituted. Interrupted setup only rolls forward a software-wallet file matching the recorded hash and passing key validation. Unsupported wallets and malformed chain codes preserve files, markers and setup journals and enter recovery; retry cannot adopt different keys.

Filenames are storage details. There is no naming, renaming, switching, replacement, or manual loading UI. Use a fresh explicit `--datadir=<path>` and first-run setup for an independently configured wallet. Mainnet uses `Wallets/`; other networks use `Wallets/<network>/`.

## Desktop configuration and operation

Use the desktop for setup, public wallet information, sending, recovery, and CoinJoin payments. Basic startup arguments such as `--datadir`, `--network`, `--config`, logging options, and `startsilent` remain supported. Environment overrides use the `MAGICALCRYPTOWALLET_` prefix.

The desktop owns one session even when its window is hidden. Closing with background operation enabled preserves synchronization and automatic CoinJoin. A foreground launch reopens the existing window; a repeated silent launch exits quietly. Quit stops services and releases the data-directory lock. Restart discards operation and CoinJoin authorization.

The scripting console, wallet automation interfaces, payment shell tools, and separate background executable have been removed. Existing wallet files and user-written scripts remain untouched. Configuration version 4 is retained; retired settings are ignored on loading and omitted on saving, without resetting other preferences.

Bitcoin Core retains its own wallet-management commands in isolated test infrastructure, the backend, and the coordinator. Regtest CoinJoin participants have independent encrypted wallets and data directories.

See [architecture](SingleWalletArchitecture.md) and [automation removal](AutomationRemoval.md) for the retained lifecycle and verification contracts.
