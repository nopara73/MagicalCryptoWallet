#!/usr/bin/env python3
"""Generate deterministic components in this product's independent GUID namespace."""
from pathlib import Path
import argparse, hashlib, json, uuid, xml.etree.ElementTree as ET

NS = "http://schemas.microsoft.com/wix/2006/wi"
ET.register_namespace("", NS)

def generate(folder: Path, destination: Path) -> None:
    root = Path(__file__).resolve().parents[2]
    identities = json.loads((root / "MagicalCryptoWallet.WindowsInstaller/identities.json").read_text())
    namespace = uuid.UUID(identities["component_namespace"])
    wix = ET.Element(f"{{{NS}}}Wix")
    fragment = ET.SubElement(wix, f"{{{NS}}}Fragment")
    directory = ET.SubElement(fragment, f"{{{NS}}}DirectoryRef", Id="INSTALLFOLDER")
    group = ET.SubElement(fragment, f"{{{NS}}}ComponentGroup", Id="PublishedComponents")
    directories = {"": directory}
    for path in sorted(folder.rglob("*")):
        if not path.is_file(): continue
        relative = path.relative_to(folder).as_posix()
        parent = path.parent.relative_to(folder).as_posix().replace(".", "", 1) if path.parent == folder else path.parent.relative_to(folder).as_posix()
        current = ""
        for part in Path(parent).parts:
            previous = current
            current = f"{current}/{part}".strip("/")
            if current not in directories:
                identifier = "d" + hashlib.sha256(current.encode()).hexdigest()[:24]
                directories[current] = ET.SubElement(directories[previous], f"{{{NS}}}Directory", Id=identifier, Name=part)
        identifier = "c" + hashlib.sha256(relative.encode()).hexdigest()[:24]
        component = ET.SubElement(directories[parent], f"{{{NS}}}Component", Id=identifier,
                                  Guid=str(uuid.uuid5(namespace, relative)).upper(), Win64="yes")
        ET.SubElement(component, f"{{{NS}}}File", Id="f" + identifier[1:], KeyPath="yes",
                      Source="$(var.BasePath)\\" + relative.replace("/", "\\"))
        ET.SubElement(group, f"{{{NS}}}ComponentRef", Id=identifier)
    destination.parent.mkdir(parents=True, exist_ok=True)
    ET.indent(wix)
    ET.ElementTree(wix).write(destination, encoding="utf-8", xml_declaration=True)

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("folder", type=Path)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    generate(args.folder.resolve(), args.destination.resolve())
