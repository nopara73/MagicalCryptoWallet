Wallet diagnostic
----------

Tools for observing the configured wallet.

## MagicalCryptoWallet keys state generation

Create a visual representation from a MagicalCryptoWallet keys dump.

### How to run it

Run MCW and wait for its configured wallet to synchronize.

Next open a terminal and enter:

```bash
dotnet fsi keygraph.fsx
```

### The result

![walletkeys.png](img/walletkeys.png)

### Dependencies

None

## MagicalCryptoWallet CoinGraph generation

Create a visual representation from a MagicalCryptoWallet coins dump.

### How to run it

Run MCW and wait for its configured wallet to synchronize.

Next open a terminal and enter:

```bash
dotnet fsi txgraph.fsx <initial-txid> | dot -Tpng | feh  -
```

## The result

![txgraph.png](img/txgraph.png)

## Dependencies

- graphviz, required for dot
- feh
