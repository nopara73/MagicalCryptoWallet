# Debugging

Build the native credential library using the [build guide](../README.md), then launch `MagicalCryptoWallet.Fluent.Desktop` with your IDE or `dotnet run`. The daemon and coordinator have separate projects and storage directories. Use `--help` for configuration overrides.

Use synthetic wallet files and a private regtest node for diagnostics. Do not paste recovery phrases, real wallet JSON, passwords, or full transaction logs into public issues. `MAGICALCRYPTOWALLET_DATADIR` selects an isolated client data directory for tests. UI resource previews can be reproduced with `dotnet run --project Contrib/VisualPreview -c Release` without initializing a wallet or networking.
