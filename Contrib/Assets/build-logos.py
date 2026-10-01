#!/usr/bin/env python3
"""Trace the supplied artwork into vector masters and export platform icons.

Build-only dependencies: Pillow, Potrace 1.16, and ImageMagick 7.
The original artwork is never modified. Raster exports are rendered from SVG.
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET

from PIL import Image

ROOT = Path(__file__).resolve().parents[2]

ASSETS = Path(__file__).resolve().parent
SVG_NS = "http://www.w3.org/2000/svg"
SIZES = (16, 24, 32, 48, 64, 128, 256, 512, 1024)


def trace(variant: str) -> dict:
    source = ASSETS / "Source" / f"MagicalCryptoWallet-{variant}-original.png"
    with Image.open(source) as image:
        image = image.convert("RGB")
        pixels = image.load()
        points = []
        for y in range(image.height):
            for x in range(image.width):
                red, green, blue = pixels[x, y]
                inside = max(red, green, blue) < 50 if variant == "compact" else green > 70
                if inside:
                    points.append((x, y))
        left = min(x for x, _ in points)
        top = min(y for _, y in points)
        right = max(x for x, _ in points) + 1
        bottom = max(y for _, y in points) + 1
        width, height = right - left, bottom - top
        mask = Image.new("1", (width, height), 1)
        output = mask.load()
        for y in range(top, bottom):
            for x in range(left, right):
                red, green, blue = pixels[x, y]
                inside = max(red, green, blue) < 50 if variant == "compact" else green > 70
                output[x - left, y - top] = 0 if inside else 1
    with tempfile.TemporaryDirectory(dir=ROOT / ".artifacts/rebrand") as temporary:
        bitmap = Path(temporary) / "mark.pbm"
        vector = Path(temporary) / "mark.svg"
        mask.save(bitmap)
        if shutil.which("potrace"):
            command = ["potrace", str(bitmap), "--svg", "--opttolerance", "0.2", "--output", str(vector)]
        elif sys.platform == "win32" and shutil.which("wsl"):
            def linux_path(path: Path) -> str:
                return "/mnt/" + path.drive[0].lower() + path.as_posix()[2:]
            command = ["wsl", "-d", "Ubuntu", "--", "potrace", linux_path(bitmap),
                       "--svg", "--opttolerance", "0.2", "--output", linux_path(vector)]
        else:
            raise RuntimeError("Install Potrace to rebuild the vector masters")
        subprocess.run(command, check=True)
        traced = ET.parse(vector).getroot()
    group = traced.find(f"{{{SVG_NS}}}g")
    if group is None:
        raise RuntimeError(f"No traced geometry for {variant}")
    paths = []
    for element in traced.iter(f"{{{SVG_NS}}}path"):
        path = {key: value for key, value in element.attrib.items() if key in ("d", "transform")}
        path["transform"] = group.get("transform", "")
        paths.append(path)
    if not paths:
        raise RuntimeError(f"No traced geometry for {variant}")
    geometry = "\n".join("  <path " + " ".join(
        f'{key}="{value}"' for key, value in path.items()) + "/>" for path in paths)
    master = ASSETS / f"MagicalCryptoWallet-{variant}.svg"
    master.write_text(
        f'<svg xmlns="{SVG_NS}" viewBox="0 0 {width} {height}" '
        f'width="{width}" height="{height}" fill="currentColor" '
        f'role="img" aria-label="Magical Crypto Wallet">\n'
        f"{geometry}\n</svg>\n", encoding="utf-8")
    return {"variant": variant, "width": width, "height": height,
            "source_bounds": [left, top, right, bottom], "paths": paths,
            "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest()}


def export_icons(compact: dict) -> None:
    # Keep consistent icon padding while giving the monogram sufficient room.
    width, height = compact["width"], compact["height"]
    scale = 840 / max(width, height)
    dx, dy = (1024 - width * scale) / 2, (1024 - height * scale) / 2
    paths = "\n".join("<path " + " ".join(
        f'{key}="{value}"' for key, value in path.items()) + "/>"
                      for path in compact["paths"])
    icon_svg = ASSETS / "MagicalCryptoWallet-icon.svg"
    icon_svg.write_text(
        f'<svg xmlns="{SVG_NS}" width="1024" height="1024" '
        'viewBox="0 0 1024 1024" role="img" aria-label="Magical Crypto Wallet">\n'
        '<rect width="1024" height="1024" fill="#70C307"/>\n'
        f'<g transform="translate({dx:.3f} {dy:.3f}) scale({scale:.6f})" '
        f'fill="#000000">{paths}</g>\n</svg>\n', encoding="utf-8")
    destinations = [ASSETS, ROOT / "MagicalCryptoWallet.Fluent.Desktop/Assets"]
    for size in SIZES:
        png = ASSETS / f"MagicalCryptoWalletLogo{size}.png"
        subprocess.run(["magick", "-background", "none", str(icon_svg),
                        "-resize", f"{size}x{size}", "-strip", str(png)], check=True)
        shutil.copyfile(png, destinations[1] / png.name)
    ico = ASSETS / "MagicalCryptoWalletLogo.ico"
    subprocess.run(["magick", str(ASSETS / "MagicalCryptoWalletLogo1024.png"),
                    "-define", "icon:auto-resize=256,128,64,48,32,24,16", str(ico)], check=True)
    shutil.copyfile(ico, destinations[1] / ico.name)
    fluent = ROOT / "MagicalCryptoWallet.Fluent/Assets"
    shutil.copyfile(ico, fluent / ico.name)
    # ICNS stores the unmodified PNG exports in Apple's standard size chunks.
    chunks = []
    for size, chunk in ((16, b"icp4"), (32, b"icp5"), (64, b"icp6"),
                        (128, b"ic07"), (256, b"ic08"), (512, b"ic09"), (1024, b"ic10")):
        data = (ASSETS / f"MagicalCryptoWalletLogo{size}.png").read_bytes()
        chunks.append(chunk + (len(data) + 8).to_bytes(4, "big") + data)
    body = b"".join(chunks)
    (ASSETS / "MagicalCryptoWalletLogo.icns").write_bytes(
        b"icns" + (len(body) + 8).to_bytes(4, "big") + body)


def main() -> None:
    variants = [trace("compact"), trace("horizontal")]
    export_icons(variants[0])
    (ASSETS / "logo-geometry.json").write_text(
        json.dumps({item["variant"]: item for item in variants}, indent=2) + "\n",
        encoding="utf-8")
    print(json.dumps({item["variant"]: {key: item[key] for key in
                     ("width", "height", "source_bounds")} for item in variants}))


if __name__ == "__main__":
    main()
