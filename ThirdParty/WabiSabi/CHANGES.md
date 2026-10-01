# Local fork changes

- Import pinned revision `295eb9fd6122b71882cbd7c367eb67387847d6c0`, corresponding to package 1.3.1. Original file hashes are recorded in `UPSTREAM.json`.
- Rename the brand-bearing randomness abstraction and all uses to `WalletRandom`, including wrappers and tests.
- Use independent assembly/package metadata and project references. Resolve NBitcoin.Secp256k1 3.1.6, the version already resolved by the wallet before this fork.
- Adapt test projects to the repository's xUnit v3/Microsoft Testing Platform runner; explicitly retain the JSON parser used for published vectors.
- Register the existing C test executable with CTest. Build native binaries from source, with the existing secp256k1 version and module settings.
- Keep algorithms, domain-separation strings, public protocol namespaces, serialized messages, native ABI, and vector data unchanged.

Original copyright/license notices remain intact. New release and platform signing keys belong to Magical Crypto Wallet and are unrelated to protocol keys.
