#!/usr/bin/env python3
"""Verify bounded callers with the exact shipping host and current managed sources."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import subprocess
import sys
from evidence import assemblies, finish, snapshot

ROOT = Path(__file__).resolve().parents[2]
sys.stdout.reconfigure(encoding='utf-8', errors='replace')


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--only', choices=('hmac', 'psbt', 'round', 'control', 'markdown', 'scan', 'script', 'nostr', 'block', 'filters'))
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    stamp = datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%S%fZ')
    output = ROOT / '.artifacts/mcw-migrations' / stamp
    output.mkdir(parents=True)
    source_hashes = {}
    for scope in ('mcw/src', 'MagicalCryptoWallet', 'MagicalCryptoWallet.Client', 'MagicalCryptoWallet.Fluent', 'Contrib/Mcw'):
        for path in (ROOT / scope).rglob('*'):
            if path.is_file() and path.suffix in ('.cs', '.rs', '.csproj', '.py') and not set(path.relative_to(ROOT).parts) & {'obj', 'bin', '.artifacts'}:
                source_hashes[path.relative_to(ROOT).as_posix()] = sha(path)
    evidence = snapshot(ROOT, output)
    evidence.update(native_sha256=sha(binary), flows=[], production_release=False)

    def run(name, command):
        print('Verifying current host caller: ' + name, flush=True)
        result = subprocess.run(command, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=600)
        (output / (name + '.log')).write_bytes(result.stdout)
        if result.returncode:
            print(result.stdout.decode('utf-8', errors='replace'))
            raise RuntimeError('Current caller verification failed: ' + name)
        evidence['flows'].append(name)

    def tool(name, project, arguments, properties=()):
        run(name, [sys.executable, str(ROOT / 'Contrib/Mcw/run-managed-tool.py'), '--host', str(binary),
                   '--project', project, '--evidence-dir', str(output / (name + '-host')),
                   *[v for p in properties for v in ('--property', p)], '--', *map(str, arguments)])

    selected = lambda name: args.only is None or args.only == name
    if selected('hmac'):
        fixture = ROOT / 'mcw/tests/wallet_hmac_fixtures/independent.tsv'
        manifest = json.loads(fixture.with_name('manifest.json').read_text())
        assert sha(fixture) == manifest['fixture_sha256']
        report = output / 'hmac.json'
        tool('hmac', 'Contrib/Mcw/HmacProbe', [fixture, report])
        values = json.loads(report.read_text())
        for key, count in {'fixtureCases': 441, 'actualDomainCalls': 462, 'adapterFailureChecks': 6,
                           'callerBoundaryChecks': 3, 'transportFaultChecks': 37}.items():
            assert values[key] == count
        assert values['connectionSurvived'] and values['canceledRequests'] > 0
    if selected('psbt'):
        tool('psbt', 'Contrib/Mcw/PsbtProbe', [])
        assert 'MCW_PSBT_HOST_VERIFIED metadata=2 factory=2 signing=retained' in (output / 'psbt.log').read_text(encoding='utf-8')
    if selected('round'):
        tool('round', 'Contrib/Mcw/RoundHashProbe', ['host', output / 'round.json', ROOT / 'mcw/tests/round_hash_vectors/managed.tsv'], ['RoundHashActivated=true'])
        assert json.loads((output / 'round.json').read_text())['updaterAcceptance']
    if selected('control'):
        tool('control', 'Contrib/McwMigration/PrivacyControlProbe', [output / 'control.json', ROOT / 'mcw/tests/privacy_control_fixtures/replies.tsv'])
        assert json.loads((output / 'control.json').read_text())['productionRustBridge']
    if selected('markdown'):
        inputs = output / 'markdown-inputs'
        inputs.mkdir()
        reference = json.loads((ROOT / 'mcw/tests/native_ui_markdown_legacy.json').read_text())
        for record in reference['records']:
            assert hashlib.sha256(record['markdown'].encode()).hexdigest() == record['markdown_sha256']
            (inputs / record['fixture']).write_bytes(record['markdown'].encode('utf-8'))
        tool('markdown', 'Contrib/McwMigration/NativeUiBridgeProbe', [output / 'markdown.json', inputs])
        run('markdown-legacy', [sys.executable, 'mcw/tests/native_ui_markdown_legacy_compare.py', '--host', '--inputs', str(inputs), '--output', str(output)])
        render_build = output / 'renderer-build'
        run('markdown-render-build', ['dotnet', 'build', 'Contrib/McwMigration/NativeUiVerification', '-c', 'Release', '-m:1',
                                      '/p:UseSharedCompilation=false', '/p:RestoreLockedMode=true', '--artifacts-path',
                                      str(output / 'renderer-artifacts'), '-o', str(render_build)])
        evidence['renderer_assemblies_before'] = assemblies(render_build)
        run('markdown-render', ['dotnet', str(render_build / 'NativeUiVerification.dll'), str(output / 'render'), str(output)])
        evidence['renderer_assemblies_after'] = assemblies(render_build)
        assert evidence['renderer_assemblies_before'] == evidence['renderer_assemblies_after']
        assert len(list((output / 'render').glob('*.png'))) == 16
    if selected('scan'):
        corpus = [{'size': int(size), 'expected': text, 'modules': modules} for size, text, modules in
                  (line.split('\t') for line in (ROOT / 'mcw/tests/qr_scanning_fixtures/golden.tsv').read_text().splitlines())]
        symbols = output / 'scanner-symbols.json'
        symbols.write_text(json.dumps(corpus), encoding='utf-8')
        tool('scan', 'Contrib/Mcw/ScannerProbe', [symbols, output / 'scan.json', ROOT / 'MagicalCryptoWallet.Tests/UnitTests/QrDecode/QrResources'])
        assert json.loads((output / 'scan.json').read_text())['production_caller_leaf_cases'] == len(corpus)
    if selected('script'):
        tool('script', 'mcw/tests/script_text_client', ['--script-text-native-child'])
        assert 'SCRIPT_TEXT_NATIVE_CALLER_CHECKS=' in (output / 'script.log').read_text(encoding='utf-8')
    if selected('nostr'):
        tool('nostr', 'mcw/tests/nostr_host', ['--evidence', output / 'nostr.json'])
        assert json.loads((output / 'nostr.json').read_text())['status'] == 'passed'
    if selected('block'):
        fixture = ROOT / 'mcw/tests/bitcoin_block_fixtures/headers.tsv'
        cache = output / 'synthetic-cache'
        tool('block', 'Contrib/Mcw/BlockHeaderProbe', ['verify', output / 'block.json', fixture, cache])
        values = json.loads((output / 'block.json').read_text())
        assert values['syntheticHeaders'] == values['nBitcoinCoreHashComparisons'] == 256
        assert values['rejectedNativePayloads'] == 82 and values['realCacheCases'] == 9
        assert values['actualHostTeardownPreservesCache']
        tool('block-recover', 'Contrib/Mcw/BlockHeaderProbe', ['recover', output / 'block-recover.json', fixture, cache])
        assert json.loads((output / 'block-recover.json').read_text()) == {'recovered': True, 'bytesUnchanged': True}
    if selected('filters'):
        # Keep retained SQLite fixture paths short on Windows. All state is synthetic.
        state_root = ROOT.parent.parent if ROOT.parent.name == '.artifacts' else ROOT
        state = state_root / '.artifacts' / ('fc-' + stamp[-9:-1])
        assert not state.exists(), 'Synthetic filter state must be a fresh directory'
        tool('filters', 'Contrib/Mcw/FilterCallerProbe', [output / 'filters.json', state])
        values = json.loads((output / 'filters.json').read_text())
        assert len(values['caller_cases']) == 10 and len(values['retained_tests']) == 4
        assert values['current_host_binding_restored'] and not values['legacy_matching_fallback']
        assert not values['production_wallet_or_network_used']
    assert evidence['native_sha256'] == sha(binary), 'Native binary changed during verification'
    assert all(sha(ROOT / name) == value for name, value in source_hashes.items()), 'Caller source changed during verification'
    evidence['current_callers_verified'] = True
    evidence['passed'] = True
    finish(ROOT, output, evidence)
    print(json.dumps({'native_sha256': evidence['native_sha256'], 'flows': evidence['flows'], 'evidence': str(output)}))


if __name__ == '__main__':
    main()
