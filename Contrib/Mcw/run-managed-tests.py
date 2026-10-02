#!/usr/bin/env python3
"""Run retained synthetic wallet tests through the shipping application host."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys
from evidence import assemblies, finish, new_run, sha, snapshot

ROOT = Path(__file__).resolve().parents[2]


def main():
    sys.stdout.reconfigure(encoding='utf-8', errors='replace')
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--project', required=True, choices=('MagicalCryptoWallet.Tests', 'MagicalCryptoWallet.IntegrationTests'))
    parser.add_argument('--prebuilt', type=Path, help='Already built output, used by the offline Nix check phase')
    args, runner_args = parser.parse_known_args()
    binary = args.binary.resolve(strict=True)
    artifacts = ROOT / '.artifacts'
    artifacts.mkdir(exist_ok=True)
    output = new_run(artifacts / 'managed-host-verification', args.project)
    work = output / 'build'
    work.mkdir()
    record = snapshot(ROOT, output)
    record.update(project=args.project, arguments=runner_args, native_sha256=sha(binary),
                  production_release=False, real_host_verified=False, prebuilt_output=str(args.prebuilt) if args.prebuilt else None,
                  compilation_provenance='supplied prebuilt output; qualification requires the matching Nix source build' if args.prebuilt else 'built from captured sources in this run')
    try:
        if args.prebuilt:
            shutil.copytree(args.prebuilt.resolve(strict=True), work, dirs_exist_ok=True)
        else:
            with (output / 'build.log').open('wb') as log:
                built = subprocess.run(['dotnet', 'build', str(ROOT / args.project), '-c', 'Release', '-m:1',
                                        '/p:UseSharedCompilation=false', '/p:BuildMcwHost=false', '/p:RestoreLockedMode=true',
                                        '--artifacts-path', str(output / 'artifacts'), '-o', str(work)],
                                       stdout=log, stderr=subprocess.STDOUT, timeout=600)
            if built.returncode:
                print((output / 'build.log').read_text(encoding='utf-8', errors='replace'))
                built.check_returncode()
        suffix = '.exe' if os.name == 'nt' else ''
        host = work / ('mcw' + suffix)
        shutil.copy2(binary, host)
        native_hash = hashlib.sha256(host.read_bytes()).hexdigest()
        child = work / ('magicalcryptowallet' + suffix)
        if os.name == 'nt':
            shutil.copy2(work / (args.project + suffix), child)
        else:
            shell = shutil.which('sh')
            dotnet = shutil.which('dotnet')
            if not shell or not dotnet:
                raise RuntimeError('The test shell and managed runtime must be present')
            child.write_text('#!' + shell + '\nexec ' + shlex.quote(dotnet) + ' ' +
                             shlex.quote(str(work / (args.project + '.dll'))) + ' "$@"\n', encoding='utf-8')
            child.chmod(0o755)
            host.chmod(0o755)
        command = [str(host), 'gui', *runner_args]
        record['assemblies_before'] = assemblies(work)
        result = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                timeout=1800, env=os.environ.copy())
        log = result.stdout.decode('utf-8', errors='replace')
        (output / 'run.log').write_text(log, encoding='utf-8')
        count = re.search(r'(?im)^\s*succeeded:\s*(\d+)', log)
        passed = (result.returncode == 0 and count is not None and int(count[1]) > 0 and
                  'MCW_MANAGED_TEST_HOST_CONNECTED services=production bridge=stdio' in log and
                  'MCW_MANAGED_TEST_HOST_CLOSED' in log)
        record.update(succeeded=int(count[1]) if count else 0, exit_code=result.returncode,
                      real_host_verified=passed, assemblies_after=assemblies(work))
        assert record['assemblies_before'] == record['assemblies_after'], 'Test assembly changed during execution'
        assert sha(binary) == native_hash == record['native_sha256']
        print(json.dumps({key: record[key] for key in ('native_sha256', 'project', 'succeeded', 'exit_code', 'real_host_verified')}))
        print('Immutable managed host evidence: ' + str(output))
        if not passed:
            print(log)
            raise SystemExit(result.returncode or 1)
    except BaseException as error:
        record.update(real_host_verified=False, error=str(error))
        raise
    finally:
        finish(ROOT, output, record)


if __name__ == '__main__':
    main()
