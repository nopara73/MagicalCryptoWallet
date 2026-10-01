<picture>
  <source media="(prefers-color-scheme: dark)" srcset="Contrib/Assets/MagicalCryptoWallet-horizontal-lime.svg">
  <img alt="Magical Crypto Wallet" src="Contrib/Assets/MagicalCryptoWallet-horizontal-black.svg" width="360">
</picture>

# Magical Crypto Wallet

A privacy-focused, open-source, non-custodial Bitcoin wallet for Windows, Linux, and macOS. Coin control, hardware wallets, Tor, silent payments, and WabiSabi coinjoins are supported.

The original Chinese password box and **Lurking Wife Mode** are back. Use the eye toggle in the sidebar to hide balances, addresses, labels, and transaction details; hover briefly to reveal a hidden item.

This project has its own application storage, installers, update keys, and releases. It starts with fresh data. Import an existing wallet file explicitly through **Add Wallet → Import Wallet**; never copy another application's complete data directory.

- [Downloads](https://github.com/nopara73/MagicalCryptoWallet/releases)
- [Build, installation, configuration, and wallet import](MagicalCryptoWallet.Documentation/README.md)
- [Support and bug reports](https://github.com/nopara73/MagicalCryptoWallet/issues)
- [Security reporting](SECURITY.md)
- [Release signing and recovery](Contrib/Signing/README.md)

Build with the .NET SDK selected by `global.json`. Build the native credential library from source before running the wallet:

```sh
cmake -S ThirdParty/WabiSabi/c -B ThirdParty/WabiSabi/c/build -DCMAKE_BUILD_TYPE=Release
cmake --build ThirdParty/WabiSabi/c/build --parallel
dotnet run --project MagicalCryptoWallet.Fluent.Desktop -c Release
```

On Windows use a MinGW C compiler and `c/build-win` as the build directory. For complete platform packages, use `python Contrib/Releases/package.py --rid <rid>`; snapshots use development version `99.99.99` without needing historical Git tags.

WabiSabi is vendored as source at a pinned revision. Its protocol, domain-separation constants, serialization, and vectors remain unchanged. See [dependency provenance](ThirdParty/WabiSabi/UPSTREAM.json) and [local changes](ThirdParty/WabiSabi/CHANGES.md).

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="MagicalCryptoWallet.Documentation/Screenshots/welcome-dark-100.png">
  <img alt="Magical Crypto Wallet welcome screen" src="MagicalCryptoWallet.Documentation/Screenshots/welcome-light-100.png" width="800">
</picture>

The [rebrand validation report](MagicalCryptoWallet.Documentation/RebrandValidation.md) records tested behavior, package identities, and remaining platform signing requirements.

Licensed under the [MIT license](LICENSE.md), with original notices retained.
