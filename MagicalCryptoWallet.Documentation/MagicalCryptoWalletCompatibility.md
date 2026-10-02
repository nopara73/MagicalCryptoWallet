# Compatibility

Software-wallet JSON, supported CLI options, raw transactions and WabiSabi wire messages retain their behavior. [Software-wallet-only compatibility](SoftwareWallet.md) documents rejected wallet files, removed RPC/Scheme indicators and external PSBT workflows. Brand-bearing public CLR names and assemblies are renamed without compatibility aliases.

Application data, startup registration, update trust keys, installer component/upgrade IDs, and application-specific ports are independent. Start with fresh configuration and import a wallet file explicitly through the first-run Set Up Wallet screen. Existing installations are not upgraded or migrated automatically. Imported software wallets retain their existing recovery derivation, encryption and account paths. Hardware, watch-only and skeleton files are unsupported and are preserved on rejection.
