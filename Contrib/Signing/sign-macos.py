#!/usr/bin/env python3
"""Import a configured certificate through Security.framework, without secret argv."""
import base64, ctypes as C, os, secrets, subprocess, sys, tempfile
from pathlib import Path

def required(name):
    value = os.environ.get("MAGICALCRYPTOWALLET_" + name)
    if not value: raise RuntimeError("Missing production setting: " + name)
    return value

def run(*args): subprocess.run([str(a) for a in args], check=True)

def notarize(archive, staple):
    with tempfile.TemporaryDirectory(prefix="magicalcryptowallet-notary-") as temporary:
        key = Path(temporary) / "AuthKey.p8"
        key.write_text(required("NOTARY_KEY")); key.chmod(0o600)
        run("xcrun", "notarytool", "submit", archive, "--key", key,
            "--key-id", required("NOTARY_KEY_ID"), "--issuer", required("NOTARY_ISSUER"), "--wait")
        run("xcrun", "stapler", "staple", staple)
        run("xcrun", "stapler", "validate", staple)

def sign(app):
    # Passwords go directly into native API memory. codesign receives only
    # a public identity and keychain path; no private material is an argument.
    cf = C.CDLL("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation")
    security = C.CDLL("/System/Library/Frameworks/Security.framework/Security")
    pointer = C.c_void_p
    cf.CFStringCreateWithCString.argtypes = [pointer, C.c_char_p, C.c_uint32]
    cf.CFStringCreateWithCString.restype = pointer
    cf.CFDataCreate.argtypes = [pointer, C.c_char_p, C.c_long]; cf.CFDataCreate.restype = pointer
    cf.CFArrayCreate.argtypes = [pointer, C.POINTER(pointer), C.c_long, pointer]; cf.CFArrayCreate.restype = pointer
    cf.CFRelease.argtypes = [pointer]
    security.SecKeychainCreate.argtypes = [C.c_char_p, C.c_uint32, pointer, C.c_bool, pointer, C.POINTER(pointer)]
    security.SecKeychainDelete.argtypes = [pointer]
    security.SecTrustedApplicationCreateFromPath.argtypes = [C.c_char_p, C.POINTER(pointer)]
    security.SecAccessCreate.argtypes = [pointer, pointer, C.POINTER(pointer)]
    class Parameters(C.Structure):
        _fields_ = [("version", C.c_uint32), ("flags", C.c_uint32), ("passphrase", pointer),
                    ("alertTitle", pointer), ("alertPrompt", pointer), ("accessRef", pointer),
                    ("keyUsage", pointer), ("keyAttributes", pointer)]
    security.SecItemImport.argtypes = [pointer, pointer, C.POINTER(C.c_uint32), C.POINTER(C.c_uint32), C.c_uint32, C.POINTER(Parameters), pointer, C.POINTER(pointer)]
    def check(status):
        if status != 0: raise RuntimeError("Security.framework signing setup failed: " + str(status))
    certificate = base64.b64decode(required("MACOS_CERTIFICATE"), validate=True)
    passphrase = cf.CFStringCreateWithCString(None, required("MACOS_CERTIFICATE_PASSWORD").encode(), 0x08000100)
    data = cf.CFDataCreate(None, certificate, len(certificate))
    title = cf.CFStringCreateWithCString(None, b"Magical Crypto Wallet Release", 0x08000100)
    trusted = pointer(); check(security.SecTrustedApplicationCreateFromPath(b"/usr/bin/codesign", C.byref(trusted)))
    apps = cf.CFArrayCreate(None, (pointer * 1)(trusted), 1, None)
    access = pointer(); check(security.SecAccessCreate(title, apps, C.byref(access)))
    keychain = pointer()
    with tempfile.TemporaryDirectory(prefix="magicalcryptowallet-signing-") as temporary:
        path = Path(temporary) / "release.keychain"
        password = secrets.token_bytes(32)
        check(security.SecKeychainCreate(os.fsencode(path), len(password), C.c_char_p(password), False, None, C.byref(keychain)))
        try:
            parameters = Parameters(0, 0, passphrase, None, None, access, None, None)
            file_format, item_type, imported = C.c_uint32(0), C.c_uint32(0), pointer()
            check(security.SecItemImport(data, None, C.byref(file_format), C.byref(item_type), 0, C.byref(parameters), keychain, C.byref(imported)))
            if imported: cf.CFRelease(imported)
            identity = required("MACOS_SIGNING_IDENTITY")
            run("codesign", "--force", "--deep", "--options", "runtime", "--timestamp", "--keychain", path, "--sign", identity, app)
            run("codesign", "--verify", "--deep", "--strict", "--verbose=2", app)
            details = subprocess.run(["codesign", "-dv", "--verbose=4", str(app)], capture_output=True, text=True, check=True)
            if "TeamIdentifier=" + required("MACOS_TEAM_ID") not in details.stderr:
                raise RuntimeError("Unexpected production signing team")
        finally:
            check(security.SecKeychainDelete(keychain)); cf.CFRelease(keychain)
            for reference in (data, passphrase, title, apps, trusted, access): cf.CFRelease(reference)

if __name__ == "__main__":
    if sys.platform != "darwin": raise RuntimeError("macOS signing requires macOS")
    if len(sys.argv) == 4 and sys.argv[1] == "--notarize": notarize(sys.argv[2], sys.argv[3])
    elif len(sys.argv) == 2: sign(sys.argv[1])
    else: raise RuntimeError("Usage: sign-macos.py <app> | --notarize <archive> <app-or-dmg>")
