# HMAC evidence qualification

This supplement preserves the published historical proof in commit
`1e0be04c94fffcd7be644e617f6b1e355a60999c` and GUI proof in
`5b7651c2a283e8f06aed11c03eb17ffc3effb069`. Their manifests, binaries and
previously published documents are unchanged.

The ignored `caller-probe/caller-verification.json` path is mutable. Its historical
referenced SHA256 was
`1d663ae512d1d29105112a601809635d0e4b5c3d5686e9e47a579b8c541d7bcc`.
The GUI run superseded it with SHA256
`6930b5f0a22b5730d7fee2cfcbeb2fd99cbda3cbf81e5fdaad837a5a6de95601`.
A raw-hash search of existing JSON/BAK files up to 2 MB in the owned
`wallet-hmac-evidence` artifacts found no matching original backup. That referenced
historical raw output is unavailable in the audited artifacts. No old execution
log was reconstructed from a manifest or other data.

The GUI report matches `wallet-hmac-gui-evidence.json` exactly. It is now preserved
with the tested source, binaries, generated project/lock, report, managed build
output and captured stdout/stderr in a separate directory:

`.artifacts/wallet-hmac-evidence/source-pinned-gui-76b5c878cc-5b7651c2a2`

The directory belongs to the isolated `.artifacts/mcw-wallet-hash-callers`
checkout. Its 94 files were copied byte-for-byte and checked against the published
GUI source/binary hashes where available. `wallet-hmac-evidence-qualification.json`
records each saved file's checksum, the source basis
`76b5c878cc27f6ae3182f2a77730fb15c163b1c2`, immutable manifest hashes, backup-search
scope and superseded-output status. Later mutable output at `caller-probe` must
not be described as the historical raw report or this pinned GUI report without
checking its checksum.

| Checks | What was executed |
|---|---|
| 441 independent fixtures / 462 successful domain calls | Actual OwnershipIdentifier/Slip21Node classes through the real Rust HMAC host |
| Two malformed requests / one canceled request | Actual Rust dispatcher and connection, followed by usable valid calls |
| 37 transport checks | Actual managed transport with scripted dummy replies: five ownership/fault modes and 32 cancellation/delivery races |
| Six adapter checks | Two direct pre-cancel/unbound checks and four fault-service modes |
| Three caller boundary checks | Actual domain classes invoking a fault service to prove host delegation |

The scripted transport and fault services do not compute HMAC. Their counts are
separate from the successful Rust-backed MAC calls. This qualification ran no
additional builds. Production activation, regular-suite host binding and
five-target acceptance remain pending coupled integration.
