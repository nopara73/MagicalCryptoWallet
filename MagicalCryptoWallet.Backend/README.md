# Historical indexer backend

This directory retains the renamed historical backend source. It was already excluded from the upstream solution: its startup references indexer services and response models that no longer exist in the current core library. It is not a supported executable or packaging target.

The wallet uses current Bitcoin RPC and provider interfaces. The supported server is `MagicalCryptoWallet.Coordinator`; the container recipe builds that service and its native dependency from source. Restoring the historical indexer service is separate work from this rebrand.
