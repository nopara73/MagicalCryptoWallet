# WabiSabi

Source-built anonymous credentials over secp256k1, with pure-managed and native implementations. This directory contains the exact pinned source import described by [UPSTREAM.json](UPSTREAM.json), original license, native sources, managed tests, and interoperability tests. No upstream Git history or prebuilt binaries were imported.

Build C sources with CMake 3.22 or newer into `c/build` (`c/build-win` on Windows). The wallet references `csharp/WabiSabi/WabiSabi.csproj` directly. The fork's assembly is `MagicalCryptoWallet.WabiSabi`; public protocol namespaces remain `WabiSabi`.

`WalletRandom` is the renamed randomness abstraction. Cryptographic algorithms, domain separators, serialization, and published test vectors are preserved. See [CHANGES.md](CHANGES.md).
