"""Run existing bounded content proof with real sources and a supplied mcw.

No native build, candidate host patch, common source edit, package/CI edit or
shipping substitution. Python/.NET and OS APIs are verification tools only.
"""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time
import xml.etree.ElementTree as ET
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'Contrib/Mcw'))
from evidence import restore_inputs_stable, restore_lock_inputs


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def sources(root):
    result = set()
    for folder in ('MagicalCryptoWallet', 'ThirdParty/WabiSabi/csharp', 'mcw/src'):
        for path in (root / folder).rglob('*'):
            relative = path.relative_to(root)
            if path.is_file() and not set(relative.parts) & {'bin', 'obj', '.artifacts'}:
                if path.suffix in {'.cs', '.csproj', '.props', '.targets', '.rs', '.json', '.bin'}:
                    result.add(relative.as_posix())
    result.update(('MagicalCryptoWallet.Client/Application/ManagedApplicationHost.cs',
                   'ThirdParty/WabiSabi/Directory.Build.props', 'Directory.Build.props',
                   'Directory.Build.targets', 'Directory.Packages.props', 'global.json',
                   'NuGet.Config', 'BannedSymbols.txt', '.editorconfig'))
    result.update(path.relative_to(root).as_posix() for path in (
        root / 'mcw/Cargo.toml', root / 'mcw/Cargo.lock', root / 'mcw/rust-toolchain.toml',
        root / 'mcw/build.rs', root / 'mcw/.cargo/config.toml', root / 'Contrib/Mcw/build.py',
        root / 'Contrib/Mcw/build-windows.ps1', root / 'Contrib/Mcw/link-linux.sh',
        root / 'Contrib/Mcw/evidence.py') if path.is_file())
    return {name: sha(root / name) for name in sorted(result)}


class Slot:
    """Windows guard matches shared FileShare.None; CI lanes are external elsewhere."""
    def __init__(self, root, wait):
        self.handle = None
        if os.name != 'nt' or not root:
            return
        kernel = ctypes.WinDLL('kernel32', use_last_error=True)
        class Memory(ctypes.Structure):
            _fields_ = [('length', ctypes.c_uint32), ('load', ctypes.c_uint32),
                        ('total', ctypes.c_uint64), ('available', ctypes.c_uint64),
                        ('total_page', ctypes.c_uint64), ('available_page', ctypes.c_uint64),
                        ('total_virtual', ctypes.c_uint64), ('available_virtual', ctypes.c_uint64),
                        ('extended', ctypes.c_uint64)]
        memory = Memory()
        memory.length = ctypes.sizeof(memory)
        assert kernel.GlobalMemoryStatusEx(ctypes.byref(memory)) and memory.available >= 2 * 1024 ** 3, 'Build deferred: RAM below 2 GiB'
        kernel.CreateFileW.argtypes = [ctypes.c_wchar_p, ctypes.c_uint32, ctypes.c_uint32,
                                      ctypes.c_void_p, ctypes.c_uint32, ctypes.c_uint32, ctypes.c_void_p]
        kernel.CreateFileW.restype = ctypes.c_void_p
        self.kernel = kernel
        deadline = time.monotonic() + wait
        while True:
            for number in (1, 2):
                handle = kernel.CreateFileW(str(root / f'build-slot-{number}.lock'),
                                            0xC0000000, 0, None, 4, 0x80, None)
                if handle != ctypes.c_void_p(-1).value:
                    self.handle = handle
                    print(f'BUILD SLOT {number}; verifier PID {os.getpid()}; existing portable content host proof', flush=True)
                    return
                if ctypes.get_last_error() not in (32, 33):
                    raise ctypes.WinError(ctypes.get_last_error())
            if time.monotonic() >= deadline:
                raise RuntimeError('Build deferred: slots occupied')
            print('Existing content proof waiting; no slot held', flush=True)
            time.sleep(10)

    def close(self):
        if self.handle is not None:
            self.kernel.CloseHandle.argtypes = [ctypes.c_void_p]
            self.kernel.CloseHandle(self.handle)
            self.handle = None
            print(f'BUILD SLOT released; verifier PID {os.getpid()}', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-root', required=True, help='Real current Core/host/factory source checkout')
    parser.add_argument('--native', required=True, help='Caller-supplied shipping mcw binary for this platform')
    parser.add_argument('--out', required=True, help='Fresh owned evidence directory')
    parser.add_argument('--coordination-root', help='Existing Windows build-slot directory')
    parser.add_argument('--wait-slot-seconds', type=int, default=180)
    parser.add_argument('--dotnet', default='dotnet')
    parser.add_argument('--build-only', action='store_true')
    args = parser.parse_args()
    root = Path(args.source_root).resolve()
    native = Path(args.native).resolve()
    out = Path(args.out).resolve()
    fixture_root = Path(__file__).resolve().parent
    assert not out.exists(), 'Fresh evidence directory required; preserve earlier runs'
    assert native.is_file(), 'Supplied shipping binary required'
    factory = root / 'MagicalCryptoWallet/WebClients/MagicalCryptoWallet/MagicalCryptoWalletHttpClientFactory.cs'
    app = (root / 'mcw/src/app.rs').read_text(encoding='utf-8')
    assert 'content_service::adapter::execute(' in app or 'content_adapter::execute(' in app
    assert 'inbox.is_interrupted(frame.id, frame.operation)' in app
    assert 'pub mod content_service;' in (root / 'mcw/src/lib.rs').read_text()
    assert 'McwContentDecodingHandler' in factory.read_text() and 'DecompressionMethods.None' in factory.read_text()
    pinned = sources(root)
    tests = (fixture_root / 'compression_content_host_fixture.cs',
             fixture_root / 'compression_fixtures/content_raw_host.cs', Path(__file__).resolve())
    test_hashes = {str(path): sha(path) for path in tests}
    binary_hash = sha(native)
    out.mkdir(parents=True)
    with zipfile.ZipFile(out / 'source-snapshot.zip', 'x', compression=zipfile.ZIP_DEFLATED) as archive:
        for name in pinned:
            archive.write(root / name, name)
        for path in tests:
            archive.write(path, path.relative_to(root).as_posix())
    project = ET.Element('Project', {'Sdk': 'Microsoft.NET.Sdk'})
    properties = ET.SubElement(project, 'PropertyGroup')
    for name, value in {
        'OutputType': 'Exe', 'TargetFramework': 'net10.0', 'LangVersion': '14',
        'AssemblyName': 'McwContentActualHostFixture', 'PackageId': 'McwContentActualHostFixture',
        'EnableDefaultCompileItems': 'false', 'UseAppHost': 'true', 'BuildMcwHost': 'false',
        'Nullable': 'enable',
        'UseSharedCompilation': 'false', 'TreatWarningsAsErrors': 'true',
        'RestorePackagesWithLockFile': 'false', 'IsPackable': 'false',
    }.items():
        ET.SubElement(properties, name).text = value
    items = ET.SubElement(project, 'ItemGroup')
    ET.SubElement(items, 'ProjectReference', {'Include': str(root / 'MagicalCryptoWallet/MagicalCryptoWallet.csproj')})
    for path in (root / 'MagicalCryptoWallet.Client/Application/ManagedApplicationHost.cs', *tests[:2]):
        ET.SubElement(items, 'Compile', {'Include': str(path)})
    ET.indent(project)
    project_path = out / 'ContentActualHost.csproj'
    ET.ElementTree(project).write(project_path, encoding='unicode')
    # Isolate every referenced project's bin/obj output. These files suppress
    # unrelated project defaults without editing any actual source or manifest.
    (out / 'Directory.Build.props').write_text('<Project />\n')
    (out / 'Directory.Build.targets').write_text('<Project />\n')
    coordination = Path(args.coordination_root).resolve() if args.coordination_root else None
    if coordination is None and os.name == 'nt':
        common = Path(subprocess.check_output(['git', '-C', str(root), 'rev-parse',
                     '--path-format=absolute', '--git-common-dir'], text=True).strip())
        coordination = common.parent / '.artifacts/mcw-coordination'
        assert coordination.is_dir(), 'Existing Windows build-slot directory required'
    slot = Slot(coordination, args.wait_slot_seconds)
    record = dict(source_root=str(root), source_hashes=pinned, fixture_hashes=test_hashes,
                  supplied_binary=str(native), native_sha256=binary_hash,
                  native_source_pairing='Caller supplies current-source shipping binary; runner pins both independently',
                  current_source_fixture=True, candidate_host_patches=False, native_rebuilt=False,
                  production_release=False, synthetic_only=True, external_network=False,
                  in_flight_native_verified=False, barrier_synchronized_native_checkpoint=False,
                  injection_schedule_is_not_proof=True, cases={})
    try:
        record['restore_inputs'] = restore_lock_inputs(root, out, pinned)
        build_log = out / 'managed-build.log'
        with build_log.open('wb') as log:
            result = subprocess.run([args.dotnet, 'build', str(project_path), '-c', 'Release', '-m:1',
                '--artifacts-path', str(out / 'build'), '-p:BuildMcwHost=false',
                '-p:UseSharedCompilation=false', '-p:RestoreLockedMode=true',
                '-p:NuGetAudit=false', '--nologo', *record['restore_inputs']['arguments']],
                stdout=log, stderr=subprocess.STDOUT, timeout=300)
        assert result.returncode == 0, 'Real Core/ManagedApplicationHost fixture build failed: ' + str(build_log)
        record['managed_build_passed'] = True
        outputs = [path for path in (out / 'build/bin/ContentActualHost').rglob('McwContentActualHostFixture.dll')]
        if not outputs:
            outputs = [path for path in (out / 'build/bin').rglob('McwContentActualHostFixture.dll')]
        assert len(outputs) == 1, 'Unique actual fixture output required'
        executable_suffix = '.exe' if os.name == 'nt' else ''
        stage = outputs[0].parent
        child = stage / ('McwContentActualHostFixture' + executable_suffix)
        assert child.is_file(), 'Portable SDK apphost missing'
        shutil.copy2(child, stage / ('magicalcryptowallet' + executable_suffix))
        staged_native = stage / ('mcw' + executable_suffix)
        shutil.copy2(native, staged_native)
        assert sha(staged_native) == binary_hash
        if os.name != 'nt':
            staged_native.chmod(staged_native.stat().st_mode | 0o111)
        record['staged_binary'] = str(staged_native)
        def assemblies():
            return {path.relative_to(stage).as_posix(): sha(path) for path in sorted(stage.rglob('*'))
                    if path.is_file() and (path.suffix in ('.dll', '.exe', '.pdb', '.so', '.dylib')
                                           or path.name in ('mcw', 'magicalcryptowallet'))}
        record['assemblies_before'] = assemblies()
        record['source_archive_sha256'] = sha(out / 'source-snapshot.zip')
        if not args.build_only:
            flags = subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0
            def run(label, arguments):
                log_path = out / (label + '.log')
                with log_path.open('wb') as log:
                    completed = subprocess.run([str(staged_native), 'gui', *arguments], cwd=stage,
                        stdout=log, stderr=subprocess.STDOUT, timeout=90, creationflags=flags)
                text = log_path.read_text(encoding='utf-8-sig')
                return completed.returncode, text, str(log_path)
            code, text, log = run('retained', [])
            assert code == 0 and text.count('ACTUAL CONTENT HOST RESULT: 26 passed;') == 1, 'Retained actual caller/bounds failed: ' + log
            record['cases']['retained'] = dict(passed=26, native_exit=code, log=log)
            print('ACTUAL CONTENT HOST: 26 retained caller/codec/bounds cases passed', flush=True)
            for mode in ('cancel', 'eof', 'saturation'):
                attempts = []
                verified = None
                # Bounded schedule search accommodates OS/CPU scheduling. Success
                # depends only on exact native partial-work counters and closure.
                for index, spins in enumerate((0, 256, 4096, 16384, 65536, 262144), 1):
                    code, text, log = run(f'{mode}-{index}', ['raw-' + mode, str(spins)])
                    attempts.append(dict(spins=spins, native_exit=code, log=log))
                    match = re.search(r'CONTENT RAW VERIFIED mode=' + mode + r' input=(\d+) output=(\d+) layer=(\d+) full=(\d+) queue_limit=(true|false) shutdown=true monotonically_increasing_requests=true', text)
                    if match:
                        consumed, produced, layer, size = map(int, match.groups()[:4])
                        assert consumed > 0 and 0 < produced < size and layer < 3
                        assert code == (0 if mode == 'cancel' else 1), 'Unexpected shipping host exit'
                        assert mode != 'saturation' or match.group(5) == 'true'
                        verified = dict(input=consumed, output=produced, layer=layer, full=size,
                                        native_exit=code, log=log, private_failure_bytes=22)
                        break
                    # Terminal native ingress may exit 1 even when the child
                    # reports a clean nonpartial trial (75). Neither can pass
                    # the gate; only the exact VERIFIED counter oracle can.
                    miss_codes = (75,) if mode == 'cancel' else (1, 75)
                    assert code in miss_codes and 'CONTENT RAW NONPARTIAL mode=' + mode in text, 'Protocol trial failed: ' + log
                record['cases'][mode] = dict(verified=verified, attempts=attempts)
                assert verified is not None, 'Unsatisfied in-flight native ' + mode + ' gate; no timing-only acceptance'
                print('ACTUAL CONTENT HOST: partial-work ' + mode + ' verified', flush=True)
            record['in_flight_native_verified'] = True
        for name, expected in pinned.items():
            assert sha(root / name) == expected, 'Current source changed during proof: ' + name
        for path, expected in test_hashes.items():
            assert sha(Path(path)) == expected, 'Fixture changed during proof: ' + path
        assert sha(native) == binary_hash and sha(staged_native) == binary_hash
        record['assemblies_after'] = assemblies()
        assert record['assemblies_before'] == record['assemblies_after'], 'Managed fixture assembly changed during proof'
        record['restore_inputs_stable'] = restore_inputs_stable(out, record['restore_inputs'])
        assert record['restore_inputs_stable'], 'Isolated restore inputs changed during proof'
        record['source_stable'] = True
        record['passed'] = not args.build_only
    except Exception as error:
        record['passed'] = False
        record['error'] = str(error)
        raise
    finally:
        (out / 'verification.json').write_text(json.dumps(record, indent=2) + '\n', encoding='utf-8')
        slot.close()
        print('CONTENT HOST EVIDENCE ' + str(out / 'verification.json'), flush=True)


if __name__ == '__main__':
    main()
