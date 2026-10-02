"""Synchronized bounded content component proof using unpatched current Inbox.

Complements the supplied-shipping-binary proof. Does not replace it or build a
native host. Python/rustc are verification tools; runtime dependencies stay zero.
"""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess

from compression_content_host_portable import Slot, sha


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-root', required=True)
    parser.add_argument('--out', required=True, help='Fresh owned evidence directory')
    parser.add_argument('--rustc', default='rustc')
    parser.add_argument('--coordination-root')
    parser.add_argument('--wait-slot-seconds', type=int, default=180)
    args = parser.parse_args()
    root = Path(args.source_root).resolve()
    out = Path(args.out).resolve()
    tests = Path(__file__).resolve().parent
    assert not out.exists(), 'Preserve earlier runs; fresh evidence required'
    assert 'pub fn is_interrupted(' in (root / 'mcw/src/app/inbox.rs').read_text()
    # Exact dependency closure of the current bridge; no stubs or shared hooks.
    names = ('qr.rs', 'qr/tables.rs', 'bridge.rs', 'app/inbox.rs',
             'bitcoin_encoding.rs', 'bitcoin_script.rs', 'script_text.rs',
             'compact_filters.rs', 'compression.rs')
    native = [root / 'mcw/src' / name for name in names]
    native += sorted((root / 'mcw/src/content_service').rglob('*'))
    native = [path for path in native if path.is_file()]
    pinned = {str(path): sha(path) for path in native}
    fixtures = (tests / 'compression_fixtures/content_inbox_tests.rs',
                Path(__file__).resolve(), tests / 'compression_content_host_portable.py')
    fixture_hashes = {str(path): sha(path) for path in fixtures}
    out.mkdir(parents=True)
    def snapshot(path):
        relative = path.relative_to(root / 'mcw/src')
        # Rust #[path] gives qr.rs a different implicit child-module directory.
        # Using mod.rs retains the original adjacent qr/tables.rs lookup.
        if relative.as_posix() == 'qr.rs':
            relative = Path('qr/mod.rs')
        return out / 'native' / relative
    for path in native:
        target = snapshot(path)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, target)
        assert sha(target) == pinned[str(path)]
    fixture = out / 'content_inbox_tests.rs'
    shutil.copy2(fixtures[0], fixture)
    modules = ('qr', 'bridge', 'bitcoin_encoding', 'bitcoin_script',
               'script_text', 'compact_filters', 'compression', 'content_service')
    wrapper = ['#![forbid(unsafe_code)]']
    for name in modules:
        relative = name + ('/mod.rs' if name in ('qr', 'content_service') else '.rs')
        wrapper.append(f'#[path="native/{relative}"] pub mod {name};')
    # Public host dispatch methods are intentionally not all used by this bounded
    # component wrapper; their exact source remains compiled and fingerprinted.
    wrapper += ['#[allow(dead_code)] #[path="native/app/inbox.rs"] mod inbox;',
                '#[path="content_inbox_tests.rs"] mod content_inbox_tests;']
    source = out / 'lib.rs'
    source.write_text('\n'.join(wrapper) + '\n', encoding='utf-8')
    environment = os.environ.copy()
    if os.name == 'nt':
        linkers = sorted(Path('C:/Program Files/Microsoft Visual Studio').glob(
            '*/*/VC/Tools/MSVC/*/bin/Hostx64/x64/link.exe'))
        assert linkers, 'Existing MSVC linker required'
        linker = linkers[-1]
        msvc = linker.parents[3]
        sdks = sorted(Path('C:/Program Files (x86)/Windows Kits/10/Lib').iterdir())
        sdk = sdks[-1]
        environment['PATH'] = str(linker.parent) + os.pathsep + str(Path(args.rustc).resolve().parent) + os.pathsep + environment['PATH']
        environment['LIB'] = os.pathsep.join(str(path) for path in
            (msvc / 'lib/onecore/x64', sdk / 'ucrt/x64', sdk / 'um/x64'))
    coordination = Path(args.coordination_root).resolve() if args.coordination_root else None
    if coordination is None and os.name == 'nt':
        common = Path(subprocess.check_output(['git', '-C', str(root), 'rev-parse',
            '--path-format=absolute', '--git-common-dir'], text=True).strip())
        coordination = common.parent / '.artifacts/mcw-coordination'
        assert coordination.is_dir(), 'Existing Windows build-slot directory required'
    record = dict(source_root=str(root), source_hashes=pinned, fixture_hashes=fixture_hashes,
                  current_source=True, test_local_shared_hook=False, actual_application_host=False,
                  production_release=False, synthetic_only=True, loopback_only=True,
                  synchronized_inner_work_checkpoint=20, partial_output_required=True,
                  private_failure_bytes=22, tests={})
    slot = Slot(coordination, args.wait_slot_seconds)
    try:
        record['compiler'] = subprocess.check_output([args.rustc, '--version'], text=True, env=environment).strip()
        record['target'] = subprocess.check_output([args.rustc, '-vV'], text=True, env=environment).strip()
        for profile, optimization in (('debug', '0'), ('optimized', '2')):
            executable = out / (profile + ('.exe' if os.name == 'nt' else ''))
            command = [args.rustc, '--edition=2024', '--test', '-D', 'warnings',
                       '-C', 'codegen-units=1', '-C', 'overflow-checks=yes',
                       '-C', 'opt-level=' + optimization]
            if os.name == 'nt':
                command += ['-C', 'target-feature=+crt-static']
            with (out / (profile + '-build.log')).open('wb') as log:
                result = subprocess.run([*command, str(source), '-o', str(executable)],
                    stdout=log, stderr=subprocess.STDOUT, env=environment, timeout=180)
            assert result.returncode == 0, 'Exact current-source component compilation failed: ' + profile
            with (out / (profile + '.log')).open('wb') as log:
                result = subprocess.run([str(executable), 'content_inbox_tests::', '--test-threads=1'],
                    stdout=log, stderr=subprocess.STDOUT, timeout=90)
            text = (out / (profile + '.log')).read_text(encoding='utf-8-sig')
            count = len(re.findall(r'^test content_inbox_tests::.* \.\.\. ok$', text, re.M))
            assert result.returncode == 0 and count == 7, 'Seven exact synchronized component cases required: ' + profile
            record['tests'][profile] = dict(passed=count, exit=result.returncode)
            print(f'CURRENT CONTENT INBOX: {profile} {count} synchronized cases passed', flush=True)
        for path, expected in pinned.items():
            assert sha(Path(path)) == expected, 'Current native source changed during proof: ' + path
            assert sha(snapshot(Path(path))) == expected
        for path, expected in fixture_hashes.items():
            assert sha(Path(path)) == expected, 'Fixture changed during proof: ' + path
        record['source_stable'] = True
        record['passed'] = True
    except Exception as error:
        record['passed'] = False
        record['error'] = str(error)
        raise
    finally:
        (out / 'verification.json').write_text(json.dumps(record, indent=2) + '\n', encoding='utf-8')
        slot.close()
        print('CONTENT INBOX EVIDENCE ' + str(out / 'verification.json'), flush=True)


if __name__ == '__main__':
    main()
