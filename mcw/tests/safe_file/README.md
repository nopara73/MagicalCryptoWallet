# SafeFile development verification

This directory contains synthetic test fixtures, an independent snapshot of the original first-party SafeFile helper, and development-only drivers. None of its executables or reference implementations ship with the wallet. The shipping service uses Rust's standard library and native OS APIs only.

From the isolated checkout, run:

```powershell
./mcw/tests/safe_file/safe_file_verify.ps1
```

The script requires the coordinator's Rust 1.99.0 toolchain, Visual Studio's Windows linker, .NET 10 and development Python at `C:/Python314/python.exe`. It acquires a shared build slot, refuses to build below 2 GiB free RAM, uses one build job, and saves source hashes and results under ignored `.artifacts/safe-file-evidence/`. It generates the exact proposed SafeFile caller change in an ignored copy, preserving the source reader and failing if the expected writer has changed. The compiled candidate must call the adapter; testing the legacy helper against itself cannot pass this verification. Production caller files stay untouched until integration.

The tests compare exact main/.new/.old bytes, exception type, parameter name and HRESULT for six standard encodings, empty and large contents, strict encoding failures, surrogate chunk boundaries, all eight initial artifact combinations, missing directories, directory collisions and Windows read-only attributes. Native tests reject incomplete, canceled and malformed sessions. Nine owned process exits verify the retained read selection. Two tests exercise the proposed actual application's CANCEL and SHUTDOWN dispatch. The same differential corpus runs through the published managed application host and an ignored source copy carrying the exact proposed shared registration.

`make_registration_patch.py` writes a reviewable patch without editing shared files. For example:

```powershell
C:/Python314/python.exe ./mcw/tests/safe_file/make_registration_patch.py --repo . --patch ./mcw/src/safe_file_service/integration.patch
```

The proposed registration, development source copy and independently framed driver are verification tools. Their success does not establish that master or a release package invokes the service. Linux/macOS runtime compatibility, the shipping runtime-import audit and physical power-loss durability require separate evidence. Existing KeyManager tests run without an application service connection; the integrator must provide a real Rust test-service binding before enabling the unconditional caller cutover.
