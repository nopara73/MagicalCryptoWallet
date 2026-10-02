"""Verify the bounded Markdown retirement in restored graphs and built artifacts.

This does not establish native-host execution or release readiness. Use
--proposed for an unpublished cutover snapshot; the report keeps that distinction.
"""
import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import xml.etree.ElementTree as ET

RETIRED = {
    "Avalonia.AvaloniaEdit", "Avalonia.Svg", "ColorDocument.Avalonia",
    "ColorTextBlock.Avalonia", "ExCSS", "Fizzler", "HtmlAgilityPack",
    "Markdown.Avalonia", "Markdown.Avalonia.Html", "Markdown.Avalonia.Svg",
    "Markdown.Avalonia.SyntaxHigh", "Markdown.Avalonia.Tight", "ShimSkiaSharp",
    "Svg.Custom", "Svg.Model",
}
RETIRED_LOWER = {name.lower() for name in RETIRED}
CONSUMERS = [
    "MagicalCryptoWallet.Fluent", "MagicalCryptoWallet.Fluent.Desktop",
    "MagicalCryptoWallet.Tests", "Contrib/Mcw/BridgeProbe", "Contrib/VisualPreview",
]
ASSEMBLIES = RETIRED_LOWER | {"avaloniaedit", "markdown.avalonia.full"}


def retired(name):
    name = name.lower()
    return name in RETIRED_LOWER or name.startswith("markdown.avalonia.")


def verify(root, artifact_directory, proposed):
    violations, graphs, artifacts = [], {}, []
    for path in [root / "Directory.Packages.props", *[
        project for consumer in CONSUMERS
        for project in (root / consumer).glob("*.csproj")
    ]]:
        if not path.is_file():
            violations.append(f"Missing manifest: {path}")
            continue
        for item in ET.parse(path).iter():
            if item.tag.rsplit("}", 1)[-1] in {"PackageReference", "PackageVersion"}:
                name = item.attrib.get("Include", item.attrib.get("Update", ""))
                if retired(name):
                    violations.append(f"Retained package reference: {path}: {name}")

    for consumer in CONSUMERS:
        path = root / consumer / "packages.lock.json"
        if not path.is_file():
            violations.append(f"Missing restored lock: {path}")
            continue
        graph = json.loads(path.read_text(encoding="utf-8-sig"))["dependencies"]
        graphs[consumer] = {}
        for framework, packages in graph.items():
            found = sorted(name for name in packages if retired(name))
            graphs[consumer][framework] = {"packages": len(packages), "retired": found}
            if found:
                violations.append(f"Retained graph: {consumer} {framework}: {found}")

    leaf = root / "MagicalCryptoWallet.Fluent/Views/Dialogs/ReleaseHighlights"
    view = (leaf / "ReleaseHighlightsDialogView.axaml").read_text(encoding="utf-8-sig")
    code = (leaf / "ReleaseHighlightsDialogView.cs").read_text(encoding="utf-8-sig")
    if "controls:ReleaseHighlightsText" not in view or "MarkdownScrollViewer" in view:
        violations.append("Retained dialog has not switched to ReleaseHighlightsText")
    if "OpenBrowserAsync(target)" not in code:
        violations.append("Retained dialog does not use its existing link confirmation")
    if (root / "MagicalCryptoWallet.Fluent/Styles/Markdown.axaml").exists():
        violations.append("Legacy Markdown styles remain compiled")
    if "pub mod markdown;" not in (root / "mcw/src/lib.rs").read_text(encoding="utf-8-sig"):
        violations.append("Markdown module export is absent")
    if "crate::markdown::dispatch" not in (root / "mcw/src/app.rs").read_text(encoding="utf-8-sig"):
        violations.append("Markdown host dispatch is absent")

    manifests = sorted(artifact_directory.rglob("*.deps.json"))
    if not any(artifact_directory.rglob("MagicalCryptoWallet.Fluent.dll")):
        violations.append(f"No compiled Fluent caller assembly: {artifact_directory}")
    if not manifests:
        violations.append(f"No built dependency manifests: {artifact_directory}")
    for path in manifests:
        data = json.loads(path.read_text(encoding="utf-8-sig"))
        names = set(data.get("libraries", {}))
        for target in data.get("targets", {}).values():
            names.update(target)
        found = sorted(name for name in names if retired(name.split("/", 1)[0]))
        artifacts.append({"manifest": str(path), "retired": found})
        if found:
            violations.append(f"Retained built dependencies: {path}: {found}")
    for path in artifact_directory.rglob("*.dll"):
        if path.stem.lower() in ASSEMBLIES or path.stem.lower().startswith("markdown.avalonia."):
            violations.append(f"Retained implementation assembly: {path}")

    return {
        "root": str(root), "proposed_cutover": proposed,
        "production_integrated": False if proposed else None,
        "published_host_verified": False, "native_release_audited": False,
        "utc": datetime.now(timezone.utc).isoformat(), "retired_candidates": sorted(RETIRED),
        "consumer_graphs": graphs, "artifact_manifests": artifacts,
        "artifact_directory": str(artifact_directory),
        "passed": not violations, "violations": violations,
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--artifact-directory", type=Path)
    parser.add_argument("--proposed", action="store_true")
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve()
    artifacts = args.artifact_directory or root / "MagicalCryptoWallet.Fluent/bin/Release/net10.0"
    result = verify(root, artifacts, args.proposed)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"passed": result["passed"], "proposed": args.proposed,
                      "consumer_graphs": len(result["consumer_graphs"]),
                      "artifact_manifests": len(result["artifact_manifests"]),
                      "violations": result["violations"]}, indent=2))
    raise SystemExit(0 if result["passed"] else 1)
