# Identity audit

Run `python Contrib/Rebrand/audit.py` after staging added or renamed files. The audit enumerates tracked paths and content, checks exact exception hashes, rejects retired trust pins, installer GUIDs and artwork, verifies immutable cryptographic source hashes, and inspects the current generated C# sources.

The same gate rejects removed manual coin-selection controls in application sources, generated code, assembly metadata and source symbol paths. `test-audit.py` verifies this rejection with a controlled source injection alongside the identity and trust-pin checks.

Use `--artifacts <directory> ...` to inspect published or extracted payloads. The managed metadata inspector examines application assemblies, embedded resources, strings, public and private symbol names, reflection references, and source document names in embedded/portable PDBs. Native credential library contents are checked for retired identifiers.

Run `python Contrib/Rebrand/inspect-packages.py --rid <rid>` on the target operating system after packaging. It extracts archives and installers, checks the platform application identity, compares every Windows MSI payload file with published output, and applies the same audit to the extracted application. Inspection reports are retained under `.artifacts/package-inspection` and uploaded by CI.

The Windows coexistence script is intended for an ephemeral CI runner. It downloads the checksum-pinned baseline recorded in `coexist-baseline.json`, installs both products, and checks independent product and upgrade identities plus execution of the desktop. Do not run this installation test on a development machine with an existing installation.

Each permitted occurrence in `exceptions.json` has a category, exact path, original text and line hash. Editing an allowed line requires reviewing and updating its exception. Generic compatibility aliases are not permitted.
