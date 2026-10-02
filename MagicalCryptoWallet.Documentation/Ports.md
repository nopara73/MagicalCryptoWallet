# Ports

A reference of common local ports used by MagicalCryptoWallet and related software.
HiddenWallet's ports (3712x) are chosen within a long range of unassigned IANA ports, based on [this](https://stackoverflow.com/a/28369841/2061103) statistic, but also checked against [Service Name and Transport Protocol Port Number Registry](https://www.iana.org/assignments/service-names-port-numbers/service-names-port-numbers.xhtml).

| Port  | Application                                       |
|-------|---------------------------------------------------|
| 38120 | HiddenWallet API                                  |
| 38121 | Tor socks port used by HiddenWallet               |
| 38122 | Tor control port used by HiddenWallet             |
| 38123 | NTumbleBit server                                 |
| 38124 | Tor socks port used by NTumbleBit                 |
| 38125 | Tor control port used by NTumbleBit               |
| 38126 | Magical Crypto Wallet Coordinator                         |
| 38127 | Magical Crypto Wallet Backend                             |
| 38130 | Magical Crypto Wallet Local Client TCPListener on TestNet |
| 38131 | Magical Crypto Wallet Local Client TCPListener on RegTest |
| 38150 | Tor socks port used by Magical Crypto Wallet              |
| 38151 | Tor control port used by Magical Crypto Wallet            |
| 9050  | Default Tor socks port                            |
| 9051  | Default Tor control port                          |
| 9150  | Tor socks port used by Tor Browser                |
| 9151  | Tor control port used by Tor Browser              |
| 8333  | Bitcoin Mainnet P2P                               |
| 48333 | Bitcoin Testnet4 P2P                              |
| 18444 | Bitcoin Regtest P2P                               |
| 5000  | Stratis: Bitcoin node and Breeze Wallet API       |
| 5105  | Stratis: Stratis node and Stratis Wallet API      |
