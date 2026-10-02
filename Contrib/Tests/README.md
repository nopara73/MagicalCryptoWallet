# Magical Crypto Wallet Regtest CoinJoin Testing Script

This bash script automates the setup and testing of Magical Crypto Wallet's CoinJoin functionality in a local Bitcoin regtest environment. It creates a complete testing environment with a Bitcoin node, MagicalCryptoWallet Coordinator, and multiple wallet clients performing simultaneous CoinJoins.

## Overview

The script performs the following operations:

1. Starts a Bitcoin Core node in regtest mode
2. Generates initial blocks to create spendable coins
3. Starts a MagicalCryptoWallet Coordinator
4. Starts five independent Magical Crypto Wallet daemons, with separate data directories and RPC ports
5. Creates and funds one wallet in each daemon
6. Initiates CoinJoin operations across all wallets
7. Monitors the coordinator logs for successful CoinJoin completion

## Prerequisites

## Configuration

The script uses the following default configuration (editable at the top of the script):

- `BITCOIN_DATADIR` - `/tmp/bitcoin-regtest` - Bitcoin regtest data directory
- `MAGICALCRYPTOWALLET_DATADIR` - `/tmp/magicalcryptowallet` - MagicalCryptoWallet data directory
- `BITCOIN_RPC_PORT` - `18443` - Bitcoin RPC port
- `BITCOIN_P2P_PORT` - `18444` - Bitcoin P2P port
- `COORDINATOR_PORT` - `38126` - MagicalCryptoWallet Coordinator port
- `MAGICALCRYPTOWALLET_WALLET_RPC_PORT` - `38128` - First client RPC port; the next clients use consecutive ports
- `NUM_CLIENTS` - `5` - Number of independent single-wallet clients
- `ADDRESSES_PER_WALLET` - `4` - Addresses per wallet to fund
- `TEST_TIMEOUT` - `600` - Timeout in seconds (10 minutes)

## Usage

```bash
./regtest-coinjoin-test.sh
```

## References

- Magical Crypto Wallet GitHub: https://github.com/nopara73/MagicalCryptoWallet
- Bitcoin Core regtest documentation
- MagicalCryptoWallet RPC API documentation
