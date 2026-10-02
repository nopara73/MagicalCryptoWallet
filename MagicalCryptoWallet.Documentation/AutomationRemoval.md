# Desktop automation removal

The Scripting action, Scheme interpreter and library generator, wallet JSON-RPC server, shell payment tools, RPC-based diagnostics, and separate `magicalcryptowalletd` executable have been deleted. The desktop and coordinator are the remaining packaged applications. No replacement scripting action or automation endpoint is registered.

Desktop OS startup registration, `startsilent`, hidden synchronization, automatic CoinJoin, same-process foreground activation, hide/reopen, and normal Quit remain supported. Basic configuration and logging arguments remain; Bitcoin Core RPC in the backend, coordinator and isolated tests remains separate.

Configuration version 4 is unchanged. Legacy `ExperimentalFeatures`, `JsonRpcServerEnabled`, `JsonRpcUser`, `JsonRpcPassword`, `JsonRpcServerPrefixes`, and `RpcOnionEnabled` fields are ignored, even if enabled or malformed. New saves and help omit those options. Other preferences are retained. Existing wallet files and user-written scripts are never deleted.

`WalletAutomationRemovalTests` verifies configuration compatibility, omitted overrides, and absent runtime types. Retained authorization, synchronization, transaction, and CoinJoin assertions call wallet services directly.

`AutomaticCoinJoinTests` runs seven encrypted wallets against isolated Bitcoin Core and the real coordinator arena. Signing an ordinary payment authorizes automatic CoinJoin. The first broadcast must have at least 21 inputs, confirmed private outputs in every wallet, and exactly-once queued payments.

`DesktopLifecycleTests` runs the Windows package against a preseeded synthetic encrypted wallet. It observes P2P synchronization while hidden, foreground activation, hide/reopen, duplicate silent launches, exact-network conflicts, normal UI shutdown and subsequent lock acquisition. Neither startup registration nor real wallet data is changed.

The five-platform build workflow runs revised suites, both-theme visual checks, and `Contrib/AutomationRemoval/audit.py`. The audit examines tracked source, generated code, dependency manifests, compiled types/resources/symbols and package contents. Exact negative-test and migration-documentation exceptions are recorded by line hash; its negative fixtures also prove that retained Bitcoin Core and startup interfaces are accepted.
