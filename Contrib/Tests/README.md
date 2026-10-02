# Single-wallet regtest verification

Both harnesses use packaged executables, synthetic wallets and new UUID data directories. RPC ports are allocated dynamically; wallet clients use the standard regtest P2P endpoint `127.0.0.1:18444`, with compact-filter serving enabled. The harness fails if that port is occupied and never stops the existing process. Core RPC is used by the harness and coordinator for mining, funding and independent assertions; wallet clients receive no Core credentials. Synthetic sends use explicit fee rates so no public fee service is required. They keep evidence and stop only their own child processes. No installation, startup registration, public broadcast or existing wallet is modified. CI uploads result JSON, logs and screenshots; wallet files stay local to the ephemeral runner.

The harnesses hold a shared temporary-file lock for port 18444 across checkouts. During the offline check, an exclusive socket reserves that port without accepting connections. This keeps another synthetic node from replacing the intended peer during downtime. The offline/reconnection check allows six minutes for the existing P2P reconnect cooldown and verifies the reconnected chain height against the harness's Core RPC. The shared `RegTestRpcProviders.cs` oracle is compiled only into the two test projects, so test filter/block adapters are excluded from application assemblies.

## Automatic CoinJoin authorization

`test-single-wallet-coinjoin.py` starts Bitcoin Core, a local coordinator and five independent encrypted single-wallet daemons. Each loads and synchronizes without a passphrase. Sending a synthetic payment authorizes automatic CoinJoin; a subsequent incorrect passphrase is rejected. The test requires an actual broadcast CoinJoin and confirmed mixed outputs in every participant. It never issues an application load or manual CoinJoin-start command.

```powershell
python Contrib/Tests/test-single-wallet-coinjoin.py --package .artifacts/packages/win-x64/MagicalCryptoWallet --bitcoind MagicalCryptoWallet.IntegrationTests/bin/Release/net10.0/BundledApps/Binaries/win-x64/bitcoind.exe
```

`regtest-coinjoin-test.sh` forwards the same arguments to this harness on systems with Python 3. The default CoinJoin deadline is 600 seconds; `--timeout` overrides it. Python 3 and a built snapshot package are required. Bitcoin Core's bundled binary is available after building the integration-test project.

## Packaged application lifetime

`test-single-wallet-process.py` creates an encrypted regtest wallet in a data directory with spaces. It verifies automatic first-setup synchronization, cached offline balance/history, reconnection, operation authorization, clean quit, credential disposal, and lock ownership. On Windows it also launches the actual desktop hidden, verifies no window or passphrase prompt, activates the same process in the foreground, captures its masked dashboard, and checks hide/reopen, duplicate silent launches, and daemon/network conflicts. Other platforms exercise the same lifecycle through the packaged daemon.

```powershell
python Contrib/Tests/test-single-wallet-process.py --package .artifacts/packages/win-x64/MagicalCryptoWallet --bitcoind MagicalCryptoWallet.IntegrationTests/bin/Release/net10.0/BundledApps/Binaries/win-x64/bitcoind.exe
```

The Windows screenshot check requires Pillow. `--output <directory>` chooses an evidence directory. Failed checks terminate only child processes launched by that run. Synthetic test directories are deliberately preserved for diagnosis.
