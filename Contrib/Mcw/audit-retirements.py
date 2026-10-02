#!/usr/bin/env python3
"""Prove retired dependency references and packaged client assets are absent."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import zipfile
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[2]
record = json.loads((ROOT / 'Contrib/McwMigration/incorporated-flows.json').read_text())
retired = {name.casefold(): name for name in record['retirement_candidates']}
source_only = {name.casefold(): name for name in record['source_only_package_retirements']}
source_forbidden = retired.keys() | source_only.keys()
client_forbidden = source_forbidden | {name.casefold() for name in record['verification_only_packages']}
assembly_forbidden = retired.keys() | {name.casefold() for name in record['verification_only_packages']}


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def framework_assets(artifacts, package_roots=None):
    """Qualify retained coordinator framework bytes, never a retired NuGet library."""
    proofs = []
    for policy in record['retained_framework_assets']:
        name = policy['assembly']
        asset = artifacts / name
        assert asset.is_file(), 'Missing retained framework asset: ' + name
        assert asset.resolve().is_relative_to(artifacts.resolve()), 'Framework asset escapes payload'
        provider_manifest = artifacts / policy['manifest']
        assert provider_manifest.is_file(), 'Missing framework provider manifest'
        supplied = []
        for manifest in artifacts.rglob('*.deps.json'):
            data = json.loads(manifest.read_text())
            for target, libraries in data['targets'].items():
                for library, sections in libraries.items():
                    for section, assets in sections.items():
                        if not isinstance(assets, dict):
                            continue
                        for path, metadata in assets.items():
                            if Path(path).name.casefold() == name.casefold():
                                assert manifest == provider_manifest and section == 'runtime' and path == name, 'Client or unexpected framework asset provider'
                                assert data['libraries'][library]['type'] == 'runtimepack', 'Retired NuGet asset presented as framework'
                                supplied.append((target, library, metadata))
        assert len(supplied) == 1, 'Ambiguous or missing retained framework provider'
        target, library, metadata = supplied[0]
        rid = target.rsplit('/', 1)[-1]
        assert rid in {'win-x64', 'linux-x64', 'linux-arm64', 'osx-x64', 'osx-arm64'}, 'Unknown framework target'
        package_name, version = library.removeprefix('runtimepack.').rsplit('/', 1)
        assert package_name == policy['runtime_package_prefix'] + rid, 'Unexpected runtime pack'
        assert re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', version), 'Invalid runtime version'
        config = json.loads((artifacts / policy['runtime_config']).read_text())['runtimeOptions']
        assert any(value == {'name': 'Microsoft.AspNetCore.App', 'version': version}
                   for value in config['includedFrameworks']), 'Coordinator framework version differs'
        assert metadata['assemblyVersion'] == version.split('.')[0] + '.0.0.0', 'Framework assembly identity differs'
        assert re.fullmatch(r'net[0-9]+\.0', config['tfm']), 'Unexpected framework TFM'
        if package_roots is None:
            environment = os.environ.copy()
            environment['DOTNET_CLI_UI_LANGUAGE'] = 'en-US'
            output = subprocess.check_output(['dotnet', 'nuget', 'locals', 'global-packages', '--list'],
                                             text=True, env=environment, timeout=30)
            assert output.startswith('global-packages: '), 'Cannot identify SDK runtime cache'
            package_roots = [Path(output.partition(': ')[2].strip())]
        candidates = [root / package_name.lower() / version / 'runtimes' / rid / 'lib' / config['tfm'] / name
                      for root in package_roots]
        candidates = [path for path in candidates if path.is_file()]
        assert len(candidates) == 1, 'Missing or ambiguous restored SDK runtime asset'
        restored = candidates[0]
        assert sha(asset) == sha(restored), 'Packaged framework bytes differ from restored SDK runtime pack'
        package = restored.parents[4] / (package_name.lower() + '.' + version + '.nupkg')
        checksum = package.with_suffix('.nupkg.sha512')
        assert package.is_file() and checksum.is_file(), 'Missing runtime package integrity input'
        package_sha512 = hashlib.sha512(package.read_bytes()).digest()
        assert base64.b64decode(checksum.read_text().strip(), validate=True) == package_sha512, 'Runtime package checksum differs'
        member = 'runtimes/' + rid + '/lib/' + config['tfm'] + '/' + name
        with zipfile.ZipFile(package) as archive:
            assert archive.getinfo(member).file_size <= 16 * 1024 * 1024, 'Framework asset is unexpectedly large'
            member_sha256 = hashlib.sha256(archive.read(member)).hexdigest()
        assert member_sha256 == sha(restored) == sha(asset), 'Framework asset differs from its integrity-checked runtime package member'
        proofs.append({'assembly': name, 'role': policy['role'], 'provider_manifest': policy['manifest'],
                       'runtime_package': package_name, 'runtime_version': version, 'rid': rid,
                       'sha256': sha(asset), 'restored_asset_sha256': sha(restored),
                       'runtime_member': member, 'runtime_member_sha256': member_sha256,
                       'runtime_package_sha512': package_sha512.hex(),
                       'package_integrity_scope': 'restored package matches its local SHA512 sidecar; not independent feed authentication',
                       'scope': 'retained framework asset; source NuGet reference removed'})
    return proofs


def audit(artifacts=None):
    checked = 0
    for path in ROOT.rglob('*'):
        if not path.is_file() or set(path.relative_to(ROOT).parts) & {'.artifacts', 'bin', 'obj', 'target', '.git'}:
            continue
        if path.name == 'packages.lock.json':
            for entries in json.loads(path.read_text(encoding='utf-8-sig'))['dependencies'].values():
                assert not (set(name.casefold() for name in entries) & source_forbidden), 'Retired lock node: ' + str(path)
            checked += 1
        elif path.suffix in ('.csproj', '.props', '.targets'):
            tree = ET.parse(path)
            names = {element.get('Include', '').casefold() for element in tree.iter()
                     if element.tag in ('PackageReference', 'PackageVersion')}
            assert not (names & source_forbidden), 'Retired package reference: ' + str(path)
    assert not ({value['pname'].casefold() for value in json.loads((ROOT / 'deps.json').read_text())} & source_forbidden), 'Retired Nix cache node'
    packages = 0
    retained = []
    if artifacts:
        assert artifacts.is_dir(), 'Extracted client package required'
        for path in artifacts.rglob('*'):
            if path.suffix.casefold() in ('.dll', '.pdb'):
                assert path.stem.casefold() not in assembly_forbidden, 'Retired or test-only client assembly: ' + str(path)
                if path.stem.casefold() in source_only:
                    assert path.suffix.casefold() == '.dll' and path.parent == artifacts, 'Unqualified source-only retired asset'
            elif path.name.endswith('.deps.json'):
                libraries = json.loads(path.read_text())['libraries']
                assert not ({name.split('/')[0].casefold() for name in libraries} & client_forbidden), 'Retired or test-only packaged dependency: ' + str(path)
                packages += 1
        assert packages > 0, 'No compiled package dependency manifest found'
        retained = framework_assets(artifacts)
    result = {'retired_packages': sorted(retired.values()), 'source_lock_files': checked,
              'source_only_package_retirements': sorted(source_only.values()), 'retained_framework_assets': retained,
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
