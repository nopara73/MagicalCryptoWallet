MagicalCryptoWallet Daemon
=============

MagicalCryptoWallet daemon is a _headless_ MagicalCryptoWallet wallet designed to minimize the usage of resources (CPU, GPU, Memory, Bandwidth) with the goal of
making it more suitable for running all the time in the background.

## Configuration

All configuration options available via `Config.json` file are also available as command line arguments and environment variables:

### Command Line and Environment variables

* Command line switches have the form `--switch_name=value` where _switch_name_ is the same name that is used in the config file (case insensitive).
* Environment variables have the form `MAGICALCRYPTOWALLET_SWITCHNAME` where _SWITCHNAME_ is the same name that is used in the config file.

A few examples:

| Config file                | Command line                | Environment variable             |
|----------------------------|-----------------------------|----------------------------------|
| Network: "TestNet"         | --network=testnet           | MAGICALCRYPTOWALLET_NETWORK=testnet           |
| JsonRpcServerEnabled: true | --jsonrpcserverenabled=true | MAGICALCRYPTOWALLET_JSONRPCSERVERENABLED=true |
| UseTor: true               | --usetor=true               | MAGICALCRYPTOWALLET_USETOR=true               |
| DustThreshold: "0.00005"   | --dustthreshold=0.00005     | MAGICALCRYPTOWALLET_DUSTTHRESHOLD=0.00005     |

### Values precedence

* **Values passed by command line arguments** have the highest precedence and override values in environment variables and those specified in config files.
* **Values stored in environment variables** have higher precedence than those in config file and lower precedence than the ones pass by command line.
* **Values stored in config file** have the lower precedence.

### Special values

There are a few special switches that are not present in the `Config.json` file and are only available using command line and/or variable environment:

* **LogLevel** to specify the level of detail used during logging
* **DataDir** to specify the path to the directory used during runtime.
* **BlockOnly** to instruct magicalcryptowallet to ignore p2p transactions

### Examples

Run MagicalCryptoWallet and connect to the testnet Bitcoin network with Tor disabled and accept JSON RPC calls. Store everything in `$HOME/temp/magicalcryptowallet-1`.

```bash
$ magicalcryptowalletd --usetor=false --datadir="$HOME/temp/magicalcryptowallet-1" --network=testnet --jsonrpcserverenabled=true --blockonly=true
```

Run MagicalCryptoWallet Daemon and connect to the testnet Bitcoin network.

```bash
$ MAGICALCRYPTOWALLET_NETWORK=testnet magicalcryptowalletd
```

The daemon automatically starts the configured wallet. Create or recover the initial wallet through the GUI or the root RPC endpoint. Setup starts it immediately; no load command or wallet-selection argument exists. Status and cached public data are available during startup, and protected operations authorize separately. See the [single-wallet API migration](../MagicalCryptoWallet.Documentation/SingleWallet.md).

### Version

```bash
$ magicalcryptowalletd --version
MagicalCryptoWallet Daemon 2.0.3.0
```

### Usage

To interact with the daemon, use the [RPC server](https://github.com/nopara73/MagicalCryptoWallet/blob/master/MagicalCryptoWallet.Documentation/README.md) or the [wcli script](https://github.com/nopara73/MagicalCryptoWallet/tree/master/Contrib/CLI).
