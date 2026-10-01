# Automatic coin selection

Send always opens the normal payment flow. The wallet chooses transaction inputs automatically, using the existing privacy-aware selection algorithms. Fees, privacy suggestions, multiple recipients, payjoin, silent payments, hardware-wallet signing, PSBT export, and Coinjoin payments remain available.

Manual Control, the Alt-key **Review coins** shortcut, and per-coin Coinjoin exclusions are removed. **Wallet Coins** remains a read-only view with expandable groups, sorting, status, privacy scores, labels, and address copying. It cannot select or exclude transaction inputs. Coinjoin still checks availability, confirmations, maturity, coordinator bans, and its automatic cooldowns.

Older wallet JSON files can still be imported. Obsolete `DefaultSendWorkflow` and `ExcludedCoinsFromCoinJoin` fields are ignored and omitted on the next save. Imported coins participate in the normal automatic selection rules. Keys, labels, wallet derivation, recovery material, and transaction wire formats are unchanged.

## RPC

`build`, `buildunsafetransaction`, and `send` accept `payments`, `feeTarget`, `feeRate`, and `password`. Payment amounts still use satoshis. They choose inputs from the configured wallet automatically. Requests containing `coins`, including the old five-argument positional form, return invalid parameters (`-32602`). `excludefromcoinjoin` is removed and returns method not found (`-32601`).

`listcoins` and `listunspentcoins` still expose read-only coin details; their exclusion flag is removed. Scheme coin inspection remains available without exclusion accessors. Raw transaction broadcasting and external PSBT workflows retain their existing transaction formats.

## Verification

Seven focused regression cases cover older-file import with unchanged keys, removed RPC capabilities, rejected explicit inputs, valid automatically selected and signed transactions, and read-only Scheme coin details. Scheme uses the shared RPC confirmation calculation for confirmed and unconfirmed coins. Headless checks exercise the actual Send, fee adjustment, confirmation, coin-details and settings controls; pressing Alt cannot reveal input selection. The source/generated/assembly audit rejects reintroduced selection controls, including compiled type/member names and symbol paths.
