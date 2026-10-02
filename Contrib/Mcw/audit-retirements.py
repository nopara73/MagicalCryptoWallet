#!/usr/bin/env python3
"""Prove retired dependency references and packaged client assets are absent."""
import argparse
import json
from pathlib import Path
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[2]
record = json.loads((ROOT / 'Contrib/McwMigration/incorporated-flows.json').read_text())
retired = {name.casefold(): name for name in record['retirement_candidates']}
client_forbidden = retired.keys() | {name.casefold() for name in record['verification_only_packages']}


def audit(artifacts=None):
    checked = 0
    for path in ROOT.rglob('*'):
        if not path.is_file() or set(path.relative_to(ROOT).parts) & {'.artifacts', 'bin', 'obj', 'target', '.git'}:
            continue
        if path.name == 'packages.lock.json':
            for entries in json.loads(path.read_text(encoding='utf-8-sig'))['dependencies'].values():
                assert not (set(name.casefold() for name in entries) & retired.keys()), 'Retired lock node: ' + str(path)
            checked += 1
        elif path.suffix in ('.csproj', '.props', '.targets'):
            tree = ET.parse(path)
            names = {element.get('Include', '').casefold() for element in tree.iter()
                     if element.tag in ('PackageReference', 'PackageVersion')}
            assert not (names & retired.keys()), 'Retired package reference: ' + str(path)
    assert not ({value['pname'].casefold() for value in json.loads((ROOT / 'deps.json').read_text())} & retired.keys()), 'Retired Nix cache node'
    packages = 0
    if artifacts:
        assert artifacts.is_dir(), 'Extracted client package required'
        for path in artifacts.rglob('*'):
            if path.suffix.casefold() in ('.dll', '.pdb'):
                assert path.stem.casefold() not in client_forbidden, 'Retired or test-only client assembly: ' + str(path)
            elif path.name.endswith('.deps.json'):
                libraries = json.loads(path.read_text())['libraries']
                assert not ({name.split('/')[0].casefold() for name in libraries} & client_forbidden), 'Retired or test-only packaged dependency: ' + str(path)
                packages += 1
        assert packages > 0, 'No compiled package dependency manifest found'
    result = {'retired_packages': sorted(retired.values()), 'source_lock_files': checked,
              'compiled_package_manifests': packages, 'production_release': False}
    if artifacts:
        evidence = ROOT / '.artifacts/mcw-evidence'
        evidence.mkdir(parents=True, exist_ok=True)
        (evidence / 'retired-package-audit.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--artifacts', type=Path)
    args = parser.parse_args()
    audit(args.artifacts)
