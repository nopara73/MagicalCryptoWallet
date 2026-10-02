# JSON reference fixtures

`JSONTestSuite/test_parsing` is the complete 318-file parsing corpus from
[nst/JSONTestSuite](https://github.com/nst/JSONTestSuite/tree/1ef36fa01286573e846ac449e8683f8833c5b26a/test_parsing),
commit `1ef36fa01286573e846ac449e8683f8833c5b26a`. The upstream MIT license is
retained in `JSONTestSuite/LICENSE`. Downloaded archive SHA-256:
`5b205e1f9533123411e794f1f052cf50df8e723a855f629a07f777f472af53d8`.
The corpus files total 354,024 bytes and are retained byte for byte, including
invalid UTF-8 and line endings. `json_vectors.sha256` pins all fixture bytes.

The 95 `y_` vectors must parse, the 188 `n_` vectors must fail, and the 35 `i_`
vectors describe implementation choices. The RFC corpus test explicitly enables
duplicate preservation: RFC 8259 recommends unique names but its grammar allows
duplicates. The application default rejects decoded duplicate keys. The corpus
test also verifies source replay, lexical number retention, ordered members,
decoded strings and deterministic reserialization for every accepted vector.

`application/*.json` contains twelve **synthetic syntax payloads**, derived from
the encoders/callers at application source baseline
`82127991068522210cdcf77080dc9b819502e486`:

- `MagicalCryptoWallet.Client/Configuration/Serialization.cs` and
  `MagicalCryptoWallet.Fluent/UiConfig/Serialization.cs`: mixed UseTor bool/string,
  string-valued Bitcoin amounts, decimal fee limits and UI geometry.
- `MagicalCryptoWallet/Blockchain/Keys/KeyManager.cs`,
  `MagicalCryptoWallet/Serialization/Client.cs` and `Bitcoin.cs`: wallet-shaped
  fields, base64/hex/key paths, labels, string heights and integer satoshi amounts.
- Frozen single/batch JSON protocol payloads cover the syntax also used by
  retained Bitcoin Core RPC: null/absent/string/numeric IDs, ordered response fields and uint32 outpoint index.
- `MagicalCryptoWallet/Serialization/Coordination.cs`: round state/credential
  fields, date strings, signed integer deltas and discriminated objects.
- `MagicalCryptoWallet/Wallets/Exchange/ExchangeRateProvider.cs` and
  `Serialization/Client.cs`: numeric/string exchange quotes and banned-coin dates.

All identities, keys, dates and endpoints are synthetic. Wallet keys and group
elements are deliberately invalid; these are JSON syntax/compatibility fixtures,
not valid wallets, signing material, credentials, a payment test or execution of
the application DTO/domain validators. Managed differential verification uses
cached Newtonsoft 13.0.4 and native .NET System.Text.Json in a temporary test
project only. Neither is a dependency of the Rust implementation.

The persistent configuration vector retains obsolete version-4 fields solely as
frozen legacy input for the generic JSON parser. It does not enable those fields
or restore the removed wallet automation runtime.
