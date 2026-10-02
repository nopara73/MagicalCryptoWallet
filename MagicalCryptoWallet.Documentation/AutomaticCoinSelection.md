# Automatic coin selection

Send always opens the normal payment flow. The wallet chooses transaction inputs automatically, using the existing privacy-aware selection algorithms. UI transactions use the highest available fee estimate, with no fee slider, custom fee entry, or saved confirmation target. Transaction previews, history, Coinjoin details, speed-ups, and cancellations show only the total fee in USD. Fee rates, breakdowns, and expected confirmation times are removed from transaction displays and status tooltips. Without a USD quote, fees show an em dash rather than a zero-dollar value. Missing estimates or insufficient funds produce an error instead of a lower fee. Privacy suggestions, multiple recipients, local passphrase signing, and Coinjoin payments remain available.

Manual Control, the Alt-key **Review coins** shortcut, and per-coin Coinjoin exclusions are removed. **Wallet Coins** remains a read-only view with expandable groups, sorting, status, privacy scores, labels, and address copying. It cannot select or exclude transaction inputs. Coinjoin still checks availability, confirmations, maturity, coordinator bans, and its automatic cooldowns.

Older software-wallet JSON files can still be imported. Obsolete `DefaultSendWorkflow` and `ExcludedCoinsFromCoinJoin` fields are ignored and omitted on the next save. Imported coins participate in the normal automatic selection rules. Keys, labels, wallet derivation, recovery material, and transaction wire formats are unchanged.

The standalone transaction broadcaster, transaction-file import/paste and PSBT export are removed. Internal PSBTs support unsigned previews and reviewed local signing; ordinary Send, transaction replacement/cancellation and CoinJoin retain the shared broadcast engine.

## Verification

Direct wallet-service regression tests cover older-file import with unchanged keys, valid automatically selected and signed transactions, confirmations, amounts, and labels. Fee tests cover highest-rate selection, missing and sparse estimates, and ignored legacy preferences without resetting other settings. Headless checks exercise the actual Send, confirmation, coin-details and settings controls; pressing Alt cannot reveal input selection. The visual harness's `--fees-only` mode checks all transaction fee displays in light and dark themes, including USD copying, missing and changing quotes, and the absence of rates and confirmation estimates. Source, generated code, assembly, and symbol audits reject reintroduced selection controls.
