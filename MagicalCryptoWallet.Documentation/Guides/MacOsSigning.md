# macOS packages

`python3 Contrib/Releases/package.py --rid osx-arm64` creates an unsigned `.app`, ZIP, and DMG. Use `osx-x64` on an Intel build host. Both share application ID `io.github.nopara73.magicalcryptowallet` and use the compact ICNS mark.

Production packaging adds `--production` and requires independently provisioned Developer ID and notarization credentials. Certificate acquisition is outside this repository's rebrand. [Signing configuration](../../Contrib/Signing/README.md) documents the required repository secrets. The signing helper imports the certificate through macOS Security APIs, verifies the configured team, notarizes, and staples. Private certificate passwords are never passed in command arguments.
