# Release signing

Public update, Nostr, and GPG identities are pinned in [public-keys.json](public-keys.json), [the application constants](../../MagicalCryptoWallet/Helpers/Constants.cs), and [PGP.txt](../../PGP.txt).

The independent private keys are saved in an encrypted, current-user Windows DPAPI keystore at `%LOCALAPPDATA%\MagicalCryptoWallet\Signing\release-keys.dpapi`, outside OneDrive. File permissions are restricted to the current Windows account. DPAPI recovery depends on that account/profile; preserve a protected backup before changing machines or deleting the profile. Never commit decrypted material.

The repository has separate Actions secrets:

- `MAGICALCRYPTOWALLET_UPDATE_SIGNING_KEY`
- `MAGICALCRYPTOWALLET_NOSTR_ANNOUNCEMENT_KEY`
- `MAGICALCRYPTOWALLET_GPG_PRIVATE_KEY`
- `MAGICALCRYPTOWALLET_GPG_PASSPHRASE`

The public GPG fingerprint is `552DC742598F1BE66485ADF3354986B46FAD0E7B`. [GitHub's secrets guide](https://docs.github.com/en/actions/how-tos/write-workflows/choose-what-workflows-do/use-secrets) describes setting replacement material. Secrets enter signing tools through environment variables, stdin, or native APIs; shell tracing is disabled. No signing key is accepted as a command-line argument.

`package.py` creates unsigned snapshots without credentials or historical tags. Production Windows packages require a genuine project code-signing PFX certificate and password. Production macOS packages require a genuine Developer ID PFX certificate, password, public signing identity and team ID, plus an App Store Connect notary key/ID/issuer. These production certificates and credentials have not been obtained as part of this rebrand.

Platform signing runs separately from packaging. Windows signatures are checked against the supplied certificate; macOS signatures are checked against the configured team, notarized, and stapled. Missing production configuration fails before packaging begins. Public artifacts must never be presented as production signed merely because an unsigned build passed.

After platform packaging, `python Contrib/Signing/sign-release.py` writes `SHA256SUMS`, clear-signs it with GPG, and signs the exact `SHA256SUMS.asc` bytes with the project's secp256k1 update key. The wallet authenticates that signature, requires the plaintext manifest to match the signed content, and checks the installer hash.

Verify downloaded material:

```sh
gpg --import PGP.txt
gpg --verify SHA256SUMS.asc
dotnet run --project Contrib/Releases/Publisher/MagicalCryptoWallet.ReleaseTools.csproj -- verify-manifest SHA256SUMS.asc SHA256SUMS.magicalcryptowalletsig 02a85e26e9e8dd0b6d06d166b1f1c427a06e09e4055f016fc55200cb9ba2dc5ef9
sha256sum --check SHA256SUMS
```

The publisher tool prepares a signed Nostr note locally, using the files that actually exist in the package directory. It does not broadcast notes or publish GitHub releases. Publication remains an explicit, separate operation.

The **Build and audit** workflow can prepare authenticated snapshots from a selected branch: enable its `prepare_release` input and leave `production` disabled. It signs only after all five platform checks, the coordinator container, and Nix tests pass. The **Prepare signed release artifacts** workflow uses the same build and signing jobs. Both upload `signed-release-artifacts` without publishing anything.
