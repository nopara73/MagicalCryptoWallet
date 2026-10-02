# Magical Crypto Wallet

The app manages [one wallet](SingleWallet.md) and uses [automatic coin selection](AutomaticCoinSelection.md). Wallet creation, hardware connection, recovery and explicit file import are available during setup.

## Build

Install the .NET SDK selected by `global.json`, CMake 3.22 or newer, and a C11 compiler (MinGW on Windows, GCC on Linux, Clang on macOS).

```sh
cmake -S ThirdParty/WabiSabi/c -B ThirdParty/WabiSabi/c/build -DCMAKE_BUILD_TYPE=Release
cmake --build ThirdParty/WabiSabi/c/build --parallel
ctest --test-dir ThirdParty/WabiSabi/c/build --output-on-failure
dotnet build MagicalCryptoWallet.Fluent.Desktop -c Release
dotnet test --project MagicalCryptoWallet.Tests -c Release --filter-namespace '*UnitTests*'
dotnet test --project ThirdParty/WabiSabi/csharp/WabiSabi.Tests -c Release
dotnet test --project ThirdParty/WabiSabi/interop/WabiSabiInterop.Tests -c Release
```

Use `c/build-win` on Windows. `python Contrib/Releases/package.py --rid win-x64` builds unsigned packages. Other supported targets are `linux-x64`, `linux-arm64`, `osx-x64`, and `osx-arm64`. `--version` overrides the development version; source files are not modified.

## Installation and storage

Download packages from [GitHub Releases](https://github.com/nopara73/MagicalCryptoWallet/releases) and verify their manifest using [the release signing guide](../Contrib/Signing/README.md). Production Windows/macOS packages require configured platform certificates.

| Component | Windows | Linux and macOS |
|---|---|---|
| Client | `%APPDATA%\MagicalCryptoWallet\Client` | `~/.magicalcryptowallet/client` |
| Backend | `%APPDATA%\MagicalCryptoWallet\Backend` | `~/.magicalcryptowallet/backend` |
| Coordinator | `%APPDATA%\MagicalCryptoWallet\Coordinator` | `~/.magicalcryptowallet/coordinator` |

The application ID is `io.github.nopara73.magicalcryptowallet`. Executables are `magicalcryptowallet`, `magicalcryptowalletd`, and `magicalcryptowallet-coordinator`. Use `--help` for supported options. Environment overrides use `MAGICALCRYPTOWALLET_`, for example `MAGICALCRYPTOWALLET_DATADIR`.

## Wallet import

The application manages [one wallet](SingleWallet.md). During initial setup, use **Set Up Wallet → Import a wallet** to choose an existing wallet JSON file explicitly. Creation, hardware wallets, and recovery are also available during setup. Existing application data with several wallets adopts one without deleting the other files. Wallet formats and recovery procedures remain compatible.

## Lurking Wife Mode

Use the eye toggle labeled **Lurking Wife Mode** in the sidebar to hide balances, addresses, labels, and transaction details. Hover over a hidden item for a quarter of a second to reveal it briefly. It hides again when the pointer leaves or after ten seconds. The enabled setting is remembered across restarts, including configurations saved under the previous display name.

## Coordinators

No public coordinator is enabled by default. Choose a coordinator you trust and configure its URI in Settings. The default regtest coordinator listens at `http://localhost:38126/`.

Coordinator fee collection is disabled by default. To enable it, set `CollectCoordinatorFees` to `true` and supply your own `CoordinatorExtPubKey` in the coordinator configuration. Startup rejects enabled fees without an xpub. This setting governs collection of available coordinator outputs; WabiSabi's cryptographic protocol is unchanged.

## Ports

Application-specific defaults use the `381xx` range: coordinator 38126, backend 38127, client JSON-RPC 38128, RPC onion service 38129, Tor SOCKS/control 38150/38151, fallback Tor 38152/38153, and coordinator Tor 38155/38156. Standard Bitcoin peer/RPC and system Tor ports retain their existing values.

## Support

[Issues](https://github.com/nopara73/MagicalCryptoWallet/issues), [security reporting](../SECURITY.md), and [source](https://github.com/nopara73/MagicalCryptoWallet) are maintained in this repository.
