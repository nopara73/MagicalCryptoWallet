# Compatibility

Wallet JSON, the wallet's JSON-RPC API, hardware-wallet formats, and WabiSabi wire messages retain their behavior. Brand-bearing public CLR names and assemblies are renamed without compatibility aliases.

Clients synchronize compact filters and download blocks through Bitcoin P2P. Bitcoin Core RPC endpoints, credentials, connection checks, and sampled CoinJoin input verification have been removed from the client. Existing version-4 settings remain readable: obsolete Core fields are ignored and omitted when settings are saved. Retired Core CLI and environment overrides are ignored like other unrecognized options. The backend, coordinator, and isolated test infrastructure retain their Core RPC support.

Application data, startup registration, update trust keys, installer component/upgrade IDs, and application-specific ports are independent. Start with fresh configuration and import a wallet file explicitly through the first-run Set Up Wallet screen. Existing installations are not upgraded or migrated automatically. Imported watch-only and signing wallets retain their existing recovery requirements.
