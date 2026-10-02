# Privacy conformance sources

These are dev-only independent references. No reference implementation is linked,
loaded, spawned or packaged by `mcw`.

- [FIPS 202](https://csrc.nist.gov/pubs/fips/202/final): Keccak-f1600, SHA3-256 and SHAKE256.
- [Keccak team's permutation examples](https://keccak.team/files/KeccakF-1600-IntermediateValues.txt): all 25 lanes after permuting the all-zero state.
- `privacy_reference.py`: Python standard-library `hashlib` generates 24
  deterministic SHA3/SHAKE vectors, including every rate/block boundary and a
  300-byte XOF output. Its native crypto provider is an independent dev oracle,
  and remains outside every shipping source/package.
- [Tor cells](https://spec.torproject.org/tor-spec/cell-packet-format.html),
  [link negotiation](https://spec.torproject.org/tor-spec/negotiating-channels.html),
  [binary certificate formats](https://spec.torproject.org/cert-spec.html),
  [relay messages](https://spec.torproject.org/tor-spec/relay-cells.html),
  [flow control](https://spec.torproject.org/tor-spec/flow-control.html): literal
  wire fixtures, malformed/duplicate/critical-extension rejection and authenticated
  SENDME checks. Parsing certificate structures does not prove their signatures.
- [Onion-address encoding](https://spec.torproject.org/rend-spec/encoding-onion-addresses.html): the three published names plus each single-symbol corruption.

`inventory.json` is a read-only exact Git/source/hash/PE/ELF/Mach-O snapshot, not a
five-target runtime test. Static Tor dependencies are not eliminated by an import
table with only OS libraries.
