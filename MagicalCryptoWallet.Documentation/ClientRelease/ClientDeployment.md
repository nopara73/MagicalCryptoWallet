# Release preparation

Update `MagicalCryptoWallet.Fluent/Assets/ReleaseHighlights.md`, run the build workflow, and inspect every target's artifacts and audit results. Dispatch the release workflow with an explicit version for signed manifest preparation. Snapshot packaging uses development version `99.99.99` when no version is supplied, without Git tags.

The workflow creates downloadable Actions artifacts and a signed announcement JSON file. It does not create a public release or send an announcement. Publishing either requires a separate explicit decision. Production mode refuses missing platform certificates. See [signing and key recovery](../../Contrib/Signing/README.md).
