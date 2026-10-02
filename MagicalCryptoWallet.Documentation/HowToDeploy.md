# Operator deployment

Build `MagicalCryptoWallet.Coordinator` in Release configuration. The [historical indexer backend](../MagicalCryptoWallet.Backend/README.md) is excluded from supported builds. Configure your own Bitcoin RPC connection, storage directory, network binding, and service manager. No upstream deployment hosts or website workflows are configured.

The coordinator rejects enabled fee collection without an operator-provided xpub. Validate against synthetic regtest wallets before exposing a service. Use the repository's [configuration guide](README.md) and [port list](Ports.md). Product release packages are prepared through the separate [release workflow](../.github/workflows/release.yml); that workflow does not deploy services or publish releases.
