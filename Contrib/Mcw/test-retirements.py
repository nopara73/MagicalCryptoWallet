#!/usr/bin/env python3
"""Synthetic negative gates for retained framework asset attribution."""
import base64
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import zipfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--output', type=Path)
args = parser.parse_args()
source_paths = [Path(__file__), Path(__file__).with_name('audit-retirements.py'),
                Path(__file__).parents[1] / 'McwMigration/incorporated-flows.json']
source_root = Path(__file__).resolve().parents[2]
source_hashes = {path.relative_to(source_root).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest() for path in source_paths}

spec = importlib.util.spec_from_file_location('retirements', Path(__file__).with_name('audit-retirements.py'))
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)
passed = 0
with tempfile.TemporaryDirectory(prefix='mcw-runtime-attribution-') as directory:
    root = Path(directory)
    payload = root / 'payload'
    payload.mkdir()
    cache = root / 'cache'
    name = 'Microsoft.Extensions.Http.dll'
    package_name = 'Microsoft.AspNetCore.App.Runtime.linux-arm64'
    version = '10.0.12'
    package_root = cache / package_name.lower() / version
    asset = package_root / 'runtimes/linux-arm64/lib/net10.0' / name
    asset.parent.mkdir(parents=True)
    synthetic_bytes = b'synthetic framework asset; no executable code'
    asset.write_bytes(synthetic_bytes)
    (payload / name).write_bytes(synthetic_bytes)
    package = package_root / (package_name.lower() + '.' + version + '.nupkg')
    with zipfile.ZipFile(package, 'w') as archive:
        archive.writestr('runtimes/linux-arm64/lib/net10.0/' + name, synthetic_bytes)
    package.with_suffix('.nupkg.sha512').write_text(base64.b64encode(hashlib.sha512(package.read_bytes()).digest()).decode())
    manifest = payload / 'MagicalCryptoWallet.Coordinator.deps.json'
    library = 'runtimepack.' + package_name + '/' + version
    baseline = {'targets': {'.NETCoreApp,Version=v10.0/linux-arm64': {
        library: {'runtime': {name: {'assemblyVersion': '10.0.0.0', 'fileVersion': '10.0.1226.42308'}}}}},
        'libraries': {library: {'type': 'runtimepack'}}}
    config = payload / 'MagicalCryptoWallet.Coordinator.runtimeconfig.json'
    config_value = {'runtimeOptions': {'tfm': 'net10.0', 'includedFrameworks': [
        {'name': 'Microsoft.AspNetCore.App', 'version': version}]}}
    def reset():
        manifest.write_text(json.dumps(baseline))
        config.write_text(json.dumps(config_value))
        (payload / name).write_bytes(synthetic_bytes)
        asset.write_bytes(synthetic_bytes)
        package.with_suffix('.nupkg.sha512').write_text(base64.b64encode(hashlib.sha512(package.read_bytes()).digest()).decode())
        for path in payload.glob('unexpected*'):
            path.unlink()
    def reject(mutate):
        global passed
        reset()
        mutate()
        try:
            audit.framework_assets(payload, [cache])
        except (AssertionError, FileNotFoundError):
            passed += 1
        else:
            raise AssertionError('Invalid framework attribution accepted')
    reset()
    proof, = audit.framework_assets(payload, [cache])
    assert proof['sha256'] == proof['restored_asset_sha256'] == hashlib.sha256(synthetic_bytes).hexdigest()
    reject(lambda: (payload / name).write_bytes(b'different package bytes'))
    reject(lambda: manifest.unlink())
    reject(lambda: config.write_text(json.dumps({'runtimeOptions': {'tfm': 'net10.0', 'includedFrameworks': []}})))
    reject(lambda: (payload / 'unexpected-client.deps.json').write_text(json.dumps(baseline)))
    def bad_type():
        data = json.loads(manifest.read_text())
        data['libraries'][library]['type'] = 'package'
        manifest.write_text(json.dumps(data))
    reject(bad_type)
    def missing_provider():
        data = json.loads(manifest.read_text())
        data['targets'] = {}
        manifest.write_text(json.dumps(data))
    reject(missing_provider)
    reject(lambda: package.with_suffix('.nupkg.sha512').write_text(base64.b64encode(b'incorrect package digest').decode()))
    reject(lambda: asset.unlink())
    def alter_both_dlls():
        (payload / name).write_bytes(b'changed cache and payload; unchanged package ZIP')
        asset.write_bytes((payload / name).read_bytes())
    reject(alter_both_dlls)
    def client_runtime_target():
        client = json.loads(json.dumps(baseline))
        client['targets']['.NETCoreApp,Version=v10.0/linux-arm64'][library] = {'runtimeTargets': {
            'runtimes/linux-arm64/lib/net10.0/' + name: {'rid': 'linux-arm64', 'assetType': 'runtime'}}}
        (payload / 'unexpected-client.deps.json').write_text(json.dumps(client))
    reject(client_runtime_target)
assert passed == 10
assert all(hashlib.sha256(path.read_bytes()).hexdigest() == source_hashes[path.relative_to(source_root).as_posix()] for path in source_paths)
report = {'synthetic_only': True, 'positive_provider': 1, 'positive_package_is_zip': True,
          'rejected_attribution_cases': passed, 'source_hashes': source_hashes, 'source_stable': True, 'passed': True}
if args.output:
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(report))
