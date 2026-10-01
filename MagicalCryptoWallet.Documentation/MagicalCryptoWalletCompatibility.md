# Compatibility

Wallet JSON, Bitcoin RPC interfaces, supported CLI options, hardware-wallet formats, and WabiSabi wire messages retain their behavior. Brand-bearing public CLR names and assemblies are renamed without compatibility aliases.

Application data, startup registration, update trust keys, installer component/upgrade IDs, and application-specific ports are independent. Start with fresh configuration and import a wallet file explicitly through Add Wallet. Existing installations are not upgraded or migrated automatically. Imported watch-only and signing wallets retain their existing recovery requirements.
