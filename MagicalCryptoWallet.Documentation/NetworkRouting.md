# Network routing

With Tor enabled, wallet transactions, transaction-specific CPFP lookups, CoinJoin requests, and wallet-selected block downloads use Tor. Onion addresses always require Tor. Exact loopback destinations (localhost, 127.0.0.1, and ::1) connect locally.

Public prices, fee estimates, release announcements, authenticated installer downloads, and Tor outage reports use direct connections by default. Installer signatures, announcement authentication, URL restrictions, and checksum verification still apply. Release checks also work with Tor disabled; regtest skips them.

With Tor enabled, public Bitcoin headers and compact filters use a separate direct peer pool. That pool cannot download wallet-selected blocks or handle wallet transactions. The protected pool supplies block downloads and broadcasts, and the pools reserve distinct peer hosts, including aliases using another port or IPv4-mapped IPv6. The synchronization pool targets six connections and retains the five compact-filter-peer requirement; the wallet pool targets three connections. Existing network-group diversity limits still apply. Tor-disabled sessions, including local regtest, share one direct pool for synchronization and wallet operations.

Set `"UseTorForPublicData": true` in the client Config.json, pass `--usetorforpublicdata=true`, or set `MAGICALCRYPTOWALLET_USETORFORPUBLICDATA=true` to route public data through Tor too. This mode shares a protected pool for synchronization and wallet traffic. Tails and Whonix automatically retain this policy. Direct public connections expose the client IP and request timing to those public services.

When a configured Bitcoin RPC node supplies compact filters, the public P2P pool stays idle. In block-only mode the wallet pool also stays idle until a block needs P2P fallback. Local RPC remains the preferred block source.

Discovery uses four crawlers per active pool, a bounded deduplicated queue, cooldowns, and adaptive DNS seed rounds. It pauses after enough connections and spare candidates exist and resumes when needed. Separate Peers-public.json and Peers-wallet.json caches store at most 128 recent addresses and service hints, expire after seven days, and recheck capabilities during each handshake. These files contain no wallet addresses, transaction IDs, or selected block hashes.

The coordinator has its own `UseTorForPublicData` setting for public fee queries, independent of `PublishAsOnionService`. Setting it to false allows those queries to connect directly while continuing to publish an onion service. For existing coordinator files without the new setting, fee-query routing preserves the previous policy: onion-publishing coordinators use Tor and other coordinators connect directly.
