# Release tools

`MagicalCryptoWallet.ReleaseTools.csproj` signs the checksum manifest, verifies an update signature, and prepares a signed Nostr announcement locally. It never broadcasts an announcement or publishes a release.

```sh
dotnet run --project Contrib/Releases/Publisher/MagicalCryptoWallet.ReleaseTools.csproj -- sign-manifest SHA256SUMS.asc SHA256SUMS.magicalcryptowalletsig
dotnet run --project Contrib/Releases/Publisher/MagicalCryptoWallet.ReleaseTools.csproj -- verify-manifest SHA256SUMS.asc SHA256SUMS.magicalcryptowalletsig <public-key>
dotnet run --project Contrib/Releases/Publisher/MagicalCryptoWallet.ReleaseTools.csproj -- prepare-announcement 99.99.99 ReleaseNote.md packages
```

Private keys come from `MAGICALCRYPTOWALLET_UPDATE_SIGNING_KEY` and `MAGICALCRYPTOWALLET_NOSTR_ANNOUNCEMENT_KEY`, never command-line arguments. Only public notes/signatures are emitted. See [signing](../../Signing/README.md).
