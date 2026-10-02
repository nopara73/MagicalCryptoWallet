#!/usr/bin/env python3
"""Source-build native libraries and produce independent, unsigned snapshots.

Production mode invokes platform signing separately and refuses missing identities.
This tool creates artifacts only; it never publishes a release or announcement.
"""
from __future__ import annotations
import argparse, hashlib, importlib.util, json, os, platform, plistlib
from pathlib import Path
import re, shutil, subprocess, sys, tarfile, zipfile

ROOT = Path(__file__).resolve().parents[2]
APP_ID = "io.github.nopara73.magicalcryptowallet"
NAME = "Magical Crypto Wallet"
RIDS = ("win-x64", "linux-x64", "linux-arm64", "osx-x64", "osx-arm64")

def run(*args, **kwargs):
    subprocess.run([str(a) for a in args], check=True, **kwargs)

def required(*names):
    missing = [name for name in names if not os.environ.get(name)]
    if missing: raise RuntimeError("Missing production configuration: " + ", ".join(missing))

def clean(path: Path):
    path = path.resolve()
    allowed = (ROOT / ".artifacts").resolve()
    if allowed not in path.parents: raise RuntimeError("Cleanup outside package workspace")
    if path.exists(): shutil.rmtree(path)
    path.mkdir(parents=True)

def zip_directory(folder: Path, destination: Path, prefix: str):
    with zipfile.ZipFile(destination, "w", zipfile.ZIP_DEFLATED, compresslevel=6) as archive:
        for path in sorted(folder.rglob("*")):
            if path.is_file(): archive.write(path, prefix + "/" + path.relative_to(folder).as_posix())

def desktop_file(executable: str):
    return f"[Desktop Entry]\nName={NAME}\nExec={executable}\nIcon={APP_ID}\nTerminal=false\nType=Application\nCategories=Finance;Network;\nStartupWMClass={APP_ID}\n"

def native_library(rid: str) -> Path:
    folder = ROOT / "ThirdParty/WabiSabi/c"
    build = folder / ("build-win" if rid.startswith("win") else "build")
    command = ["cmake", "-S", str(folder), "-B", str(build), "-DCMAKE_BUILD_TYPE=Release"]
    if rid.startswith("win"):
        command += ["-G", "MinGW Makefiles", "-DCMAKE_C_COMPILER=gcc"]
    if rid.startswith("osx"):
        command += ["-DCMAKE_OSX_ARCHITECTURES=" + ("arm64" if rid.endswith("arm64") else "x86_64")]
    run(*command)
    run("cmake", "--build", build, "--parallel", "4")
    run("ctest", "--test-dir", build, "--output-on-failure")
    extension = ".dll" if rid.startswith("win") else ".dylib" if rid.startswith("osx") else ".so"
    library = build / ("libwabisabi" + extension)
    if not library.is_file(): raise RuntimeError("Source-built native library is missing")
    return library

def windows_installer(dist: Path, work: Path, output: Path, version: str, production: bool):
    spec = importlib.util.spec_from_file_location("components", ROOT / "Contrib/Releases/installer-components.py")
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    components = work / "PublishedComponents.wxs"
    module.generate(dist, components)
    wix = ROOT / "MagicalCryptoWallet.WindowsInstaller"
    binaries = Path(os.environ.get("WIX", "C:/Program Files (x86)/WiX Toolset v3.14")) / "bin"
    candle = shutil.which("candle.exe") or str(binaries / "candle.exe")
    light = shutil.which("light.exe") or str(binaries / "light.exe")
    files = [wix / f for f in ("Product.wxs", "Components.wxs", "Directories.wxs")] + [components]
    objects = work / "wix"; objects.mkdir(exist_ok=True)
    for source in files:
        run(candle, "-nologo", "-arch", "x64", "-ext", "WixUtilExtension", "-ext", "WixUIExtension",
            "-dBuildVersion=" + version, "-dBasePath=" + str(dist),
            "-dDesktopProjectDir=" + str(ROOT / "MagicalCryptoWallet.Fluent.Desktop"),
            "-out", str(objects) + os.sep, source)
    package = output / f"MagicalCryptoWallet-{version}.msi"
    run(light, "-nologo", "-ext", "WixUtilExtension", "-ext", "WixUIExtension", "-sice:ICE40",
        "-loc", wix / "Common.wxl", "-pdbout", work / "Installer.wixpdb", "-out", package, *sorted(objects.glob("*.wixobj")))
    if production: run("pwsh", "-NoProfile", "-File", ROOT / "Contrib/Signing/sign-windows.ps1", package)

def linux_packages(dist: Path, work: Path, output: Path, version: str, rid: str, appimage: bool):
    architecture = "arm64" if rid.endswith("arm64") else "amd64"
    deb = work / "deb"; clean(deb)
    shutil.copytree(dist, deb / "opt/magicalcryptowallet", dirs_exist_ok=True)
    (deb / "usr/bin").mkdir(parents=True)
    for executable in ("magicalcryptowallet", "magicalcryptowalletd", "magicalcryptowallet-coordinator"):
        (deb / "usr/bin" / executable).symlink_to("/opt/magicalcryptowallet/" + executable)
    applications = deb / "usr/share/applications"; applications.mkdir(parents=True)
    (applications / f"{APP_ID}.desktop").write_text(desktop_file("magicalcryptowallet"))
    for size in (16, 24, 32, 48, 64, 128, 256, 512):
        icons = deb / f"usr/share/icons/hicolor/{size}x{size}/apps"; icons.mkdir(parents=True)
        shutil.copyfile(ROOT / f"Contrib/Assets/MagicalCryptoWalletLogo{size}.png", icons / f"{APP_ID}.png")
    control = deb / "DEBIAN"; control.mkdir()
    (control / "control").write_text(f"Package: magicalcryptowallet\nVersion: {version}\nSection: net\nPriority: optional\nArchitecture: {architecture}\nMaintainer: Magical Crypto Wallet Contributors <nopara73@users.noreply.github.com>\nHomepage: https://github.com/nopara73/MagicalCryptoWallet\nDepends: libx11-6, libxrandr2, libfontconfig1, libice6, libsm6, libglib2.0-0, libstdc++6\nDescription: Privacy-focused Bitcoin wallet\n")
    suffix = "-arm64" if architecture == "arm64" else ""
    run("dpkg-deb", "--root-owner-group", "--build", deb, output / f"MagicalCryptoWallet-{version}{suffix}.deb")
    if appimage:
        appdir = work / "AppDir"; clean(appdir)
        shutil.copytree(dist, appdir / "usr/lib/magicalcryptowallet")
        (appdir / "AppRun").write_text('#!/bin/sh\nHERE=$(dirname "$(readlink -f "$0")")\nexec "$HERE/usr/lib/magicalcryptowallet/magicalcryptowallet" "$@"\n')
        (appdir / "AppRun").chmod(0o755)
        (appdir / f"{APP_ID}.desktop").write_text(desktop_file("magicalcryptowallet"))
        shutil.copyfile(ROOT / "Contrib/Assets/MagicalCryptoWalletLogo256.png", appdir / f"{APP_ID}.png")
        tool = os.environ.get("APPIMAGETOOL", "appimagetool")
        environment = os.environ.copy(); environment["ARCH"] = "aarch64" if architecture == "arm64" else "x86_64"
        run(tool, "--no-appstream", appdir, output / f"MagicalCryptoWallet-{version}{suffix}.AppImage", env=environment)

def macos_packages(dist: Path, work: Path, output: Path, version: str, rid: str, production: bool):
    staging = work / "dmg"; clean(staging)
    app = staging / f"{NAME}.app"; contents = app / "Contents"
    resources = contents / "Resources"; resources.mkdir(parents=True)
    shutil.copytree(dist, contents / "MacOS")
    shutil.copyfile(ROOT / "Contrib/Assets/MagicalCryptoWalletLogo.icns", resources / "MagicalCryptoWalletLogo.icns")
    with (contents / "Info.plist").open("wb") as stream:
        plistlib.dump({"CFBundleIdentifier": APP_ID, "CFBundleName": NAME, "CFBundleDisplayName": NAME,
          "CFBundleExecutable": "magicalcryptowallet", "CFBundleIconFile": "MagicalCryptoWalletLogo.icns",
          "CFBundleVersion": version, "CFBundleShortVersionString": version,
          "CFBundlePackageType": "APPL", "NSHighResolutionCapable": True,
          "LSMinimumSystemVersion": "12.0"}, stream)
    suffix = "-arm64" if rid.endswith("arm64") else ""
    if production:
        run("python3", ROOT / "Contrib/Signing/sign-macos.py", app)
    zipped = output / f"MagicalCryptoWallet-{version}-macOS-{rid.split('-')[1]}.zip"
    # ditto preserves framework symlinks and executable permissions.
    run("ditto", "-c", "-k", "--sequesterRsrc", "--keepParent", app, zipped)
    if production:
        run("python3", ROOT / "Contrib/Signing/sign-macos.py", "--notarize", zipped, app)
        # Include the stapled app in the final archive.
        zipped.unlink()
        run("ditto", "-c", "-k", "--sequesterRsrc", "--keepParent", app, zipped)
    (staging / "Applications").symlink_to("/Applications")
    dmg = output / f"MagicalCryptoWallet-{version}{suffix}.dmg"
    run("hdiutil", "create", "-volname", NAME, "-srcfolder", staging, "-format", "UDZO", "-ov", dmg)
    if production:
        run("python3", ROOT / "Contrib/Signing/sign-macos.py", "--notarize", dmg, dmg)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rid", choices=RIDS, required=True)
    parser.add_argument("--version", default=os.environ.get("MAGICALCRYPTOWALLET_VERSION", "99.99.99"))
    parser.add_argument("--production", action="store_true")
    parser.add_argument("--appimage", action="store_true")
    parser.add_argument("--skip-native", action="store_true", help="Use an already source-built library for this RID")
    args = parser.parse_args()
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:\.\d+)?", args.version): raise RuntimeError("Expected a numeric 3- or 4-part version")
    if args.production:
        if args.rid.startswith("win"): required("MAGICALCRYPTOWALLET_WINDOWS_CERTIFICATE", "MAGICALCRYPTOWALLET_WINDOWS_CERTIFICATE_PASSWORD")
        if args.rid.startswith("osx"): required("MAGICALCRYPTOWALLET_MACOS_CERTIFICATE", "MAGICALCRYPTOWALLET_MACOS_CERTIFICATE_PASSWORD", "MAGICALCRYPTOWALLET_MACOS_SIGNING_IDENTITY", "MAGICALCRYPTOWALLET_MACOS_TEAM_ID", "MAGICALCRYPTOWALLET_NOTARY_KEY", "MAGICALCRYPTOWALLET_NOTARY_KEY_ID", "MAGICALCRYPTOWALLET_NOTARY_ISSUER")
    if not args.skip_native: library = native_library(args.rid)
    else:
        extension = ".dll" if args.rid.startswith("win") else ".dylib" if args.rid.startswith("osx") else ".so"
        library = ROOT / "ThirdParty/WabiSabi/c" / ("build-win" if args.rid.startswith("win") else "build") / ("libwabisabi" + extension)
        if not library.is_file(): raise RuntimeError("Source-built native library is missing")
    work = ROOT / ".artifacts/packages" / args.rid; clean(work)
    dist = work / "MagicalCryptoWallet"; dist.mkdir()
    output = ROOT / "packages"; output.mkdir(exist_ok=True)
    for project, executable in (("MagicalCryptoWallet.Fluent.Desktop", "magicalcryptowallet"),
                                ("MagicalCryptoWallet.Daemon", "magicalcryptowalletd"),
                                ("MagicalCryptoWallet.Coordinator", "magicalcryptowallet-coordinator")):
        publish = work / project
        run("dotnet", "publish", ROOT / project / (project + ".csproj"), "-c", "Release", "-r", args.rid,
            "--self-contained", "true", "-p:ClientVersion=" + args.version,
            "-p:NativeLibraryPath=" + str(library), "-p:DebugType=embedded", "-o", publish)
        extension = ".exe" if args.rid.startswith("win") else ""
        (publish / (project + extension)).rename(publish / (executable + extension))
        shutil.copytree(publish, dist, dirs_exist_ok=True)
    for source in ("LICENSE.md", "NOTICE.md", "PGP.txt"):
        shutil.copyfile(ROOT / source, dist / source)
    if not args.rid.startswith("win"):
        for path in dist.rglob("*"):
            if path.is_file() and (path.name in ("magicalcryptowallet", "magicalcryptowalletd", "magicalcryptowallet-coordinator", "tor") or path.suffix in (".so", ".dylib")): path.chmod(0o755)
    if args.production and args.rid.startswith("win"):
        run("pwsh", "-NoProfile", "-File", ROOT / "Contrib/Signing/sign-windows.ps1", dist)
    if args.rid.startswith("osx"):
        macos_packages(dist, work, output, args.version, args.rid, args.production)
    else:
        zip_directory(dist, output / f"MagicalCryptoWallet-{args.version}-{args.rid}.zip", "MagicalCryptoWallet")
        if args.rid.startswith("win"): windows_installer(dist, work, output, args.version, args.production)
        else:
            with tarfile.open(output / f"MagicalCryptoWallet-{args.version}-{args.rid}.tar.gz", "w:gz") as archive:
                archive.add(dist, arcname="MagicalCryptoWallet")
            linux_packages(dist, work, output, args.version, args.rid, args.appimage)
    print(json.dumps({"rid": args.rid, "version": args.version, "signed_platform": args.production,
                      "package_directory": str(output)}))

if __name__ == "__main__": main()
