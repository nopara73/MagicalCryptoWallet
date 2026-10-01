# Reproducing packages

Check out the exact commit, install the SDK selected by `global.json`, and use the source compiler and platform tools recorded by the workflow. Restore committed NuGet lockfiles. Native credentials use the pinned WabiSabi source and checksum-pinned secp256k1 archive.

```sh
python3 Contrib/Releases/package.py --rid linux-x64 --version 99.99.99
python3 Contrib/Rebrand/audit.py --artifacts .artifacts/packages/linux-x64/MagicalCryptoWallet
```

Build each architecture on its corresponding platform. Other targets are `win-x64`, `linux-arm64`, `osx-x64`, and `osx-arm64`. Compare extracted file hashes, assembly metadata, resources, installer identities, and protocol test results. Packaging timestamps and production signatures can vary; reproducibility of complete installers has not been established by this rebrand. See [manifest verification](../../Contrib/Signing/README.md).
