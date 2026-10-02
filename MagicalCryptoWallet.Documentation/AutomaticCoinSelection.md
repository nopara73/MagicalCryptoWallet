# Automatic coin selection

Send always opens the normal payment flow. The wallet chooses transaction inputs automatically, using the existing privacy-aware selection algorithms. UI transactions use the highest available fee estimate, with no fee slider, custom fee entry, or saved confirmation target. Transaction previews, history, Coinjoin details, broadcasting, speed-ups, and cancellations show only the total fee in USD. Fee rates, breakdowns, and expected confirmation times are removed from transaction displays and status tooltips. Without a USD quote, fees show an em dash rather than a zero-dollar value. Missing estimates or insufficient funds produce an error instead of a lower fee. Privacy suggestions, multiple recipients, payjoin, silent payments, local passphrase signing, and Coinjoin payments remain available.

Manual Control, the Alt-key **Review coins** shortcut, and per-coin Coinjoin exclusions are removed. **Wallet Coins** remains a read-only view with expandable groups, sorting, status, privacy scores, labels, and address copying. It cannot select or exclude transaction inputs. Coinjoin still checks availability, confirmations, maturity, coordinator bans, and its automatic cooldowns.

Older software-wallet JSON files can still be imported. Obsolete `DefaultSendWorkflow` and `ExcludedCoinsFromCoinJoin` fields are ignored and omitted on the next save. Imported coins participate in the normal automatic selection rules. Keys, labels, wallet derivation, recovery material, and transaction wire formats are unchanged.

## RPC

`build`, `buildunsafetransaction`, and `send` accept `payments`, `feeTarget`, `feeRate`, and `password`. Payment amounts still use satoshis. They choose inputs from the configured wallet automatically. Requests containing `coins`, including the old five-argument positional form, return invalid parameters (`-32602`). `excludefromcoinjoin` is removed and returns method not found (`-32601`).

`listcoins` and `listunspentcoins` still expose read-only coin details; their exclusion flag is removed. Scheme coin inspection remains available without exclusion accessors. Raw transaction broadcasting retains its transaction format. External PSBT import, paste and export are removed; internal PSBTs still support unsigned previews, local signing and Payjoin.

## Verification

Seven focused regression cases cover older-file import with unchanged keys, removed RPC capabilities, rejected explicit inputs, valid automatically selected and signed transactions, and read-only Scheme coin details. Scheme uses the shared RPC confirmation calculation for confirmed and unconfirmed coins. Fee tests cover highest-rate selection, missing and sparse estimates, and ignored legacy fee preferences without resetting other settings. Headless checks exercise the actual Send, confirmation, coin-details and settings controls; pressing Alt cannot reveal input selection. The visual harness's `--fees-only` mode checks all transaction fee displays in light and dark themes, including USD copying, missing and changing quotes, and the absence of rates and confirmation estimates. The source/generated/assembly audit rejects reintroduced selection controls, including compiled type/member names and symbol paths.
