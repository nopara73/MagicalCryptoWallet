"""Immutable source and assembly evidence for synthetic application checks."""
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import subprocess
import xml.etree.ElementTree as ET
import zipfile


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def new_run(parent, label):
    stamp = datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%S%fZ')
    path = Path(parent) / (stamp + '-' + label)
    path.mkdir(parents=True, exist_ok=False)
    return path


def git(root, *arguments):
    try:
        return subprocess.run(['git', *arguments], cwd=root,
                              stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    except FileNotFoundError:
        # Git is a development tool, not an input required by offline Nix checks.
        return None


def source_files(root):
    result = git(root, 'ls-files', '-z', '--cached', '--others', '--exclude-standard')
    if result is not None and result.returncode == 0:
        names = [name.decode('utf-8') for name in result.stdout.split(b'\0') if name]
    else:
        # Offline Nix sources deliberately omit .git. Inspect the actual source
        # tree, excluding only generated/build output; provenance stays explicit.
        names = []
        for directory, subdirectories, files in os.walk(root):
            subdirectories[:] = [name for name in subdirectories if name not in
                                 {'.git', '.artifacts', 'bin', 'obj', 'target', '__pycache__'}]
            names.extend((Path(directory) / name).relative_to(root).as_posix() for name in files)
    suffixes = {'.cs', '.rs', '.csproj', '.props', '.targets', '.slnx', '.json', '.py',
                '.toml', '.lock', '.yml', '.yaml', '.axaml', '.nix', '.tsv', '.bin', '.txt', '.ps1', '.sh', '.inc',
                '.png', '.jpg', '.jpeg', '.gif', '.webp', '.bmp', '.svg', '.ico', '.ttf', '.otf'}
    selected = sorted({name for name in names if Path(name).suffix in suffixes
                       or name in ('.editorconfig', 'NuGet.Config', 'BannedSymbols.txt',
                                   'MagicalCryptoWallet/Announcements/ReleaseHighlights.md')})
    return [name for name in selected if (root / name).is_file()]


def snapshot(root, output):
    """Include fixtures and build inputs, preserving the bytes actually tested."""
    values = {}
    archive = output / 'source-snapshot.zip'
    with zipfile.ZipFile(archive, 'x', compression=zipfile.ZIP_DEFLATED) as zipped:
        for name in source_files(root):
            captured = (root / name).read_bytes()
            values[name] = hashlib.sha256(captured).hexdigest()
            zipped.writestr(name, captured)
    head = git(root, 'rev-parse', 'HEAD')
    diff = git(root, 'diff', '--binary', 'HEAD')
    return {'source_hashes': values, 'source_archive_sha256': sha(archive),
            'git_head': head.stdout.decode('utf-8').strip() if head is not None and head.returncode == 0 else None,
            'git_diff_sha256': hashlib.sha256(diff.stdout).hexdigest() if diff is not None and diff.returncode == 0 else None,
            'build_environment': {name: os.environ.get(name) for name in
                                  ('CARGO_BUILD_JOBS', 'RUSTFLAGS', 'MAGICALCRYPTOWALLET_VERSION')}}


def assemblies(directory):
    suffixes = {'.dll', '.exe', '.pdb', '.so', '.dylib'}
    return {path.relative_to(directory).as_posix(): sha(path) for path in sorted(directory.rglob('*'))
            if path.is_file() and (path.suffix in suffixes or path.name in ('mcw', 'magicalcryptowallet')
                                   or path.name.endswith(('.deps.json', '.runtimeconfig.json')))}


def restore_lock_inputs(root, output, names=None):
    """Isolate no-RID verifier locks without changing any resolved framework graph."""
    project = ET.Element('Project')
    records = {}
    for name in names if names is not None else source_files(root):
        if not name.endswith('.csproj'):
            continue
        source = root / Path(name).parent / 'packages.lock.json'
        if not source.is_file():
            continue
        original = json.loads(source.read_text(encoding='utf-8-sig'))
        portable = dict(original)
        portable['dependencies'] = {framework: value for framework, value in original['dependencies'].items()
                                    if '/' not in framework}
        assert portable['dependencies'], 'A framework lock graph is required'
        relative = source.relative_to(root).as_posix()
        destination = output / 'restore-locks' / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(json.dumps(portable, indent=2) + '\n', encoding='utf-8')
        group = ET.SubElement(project, 'PropertyGroup', {
            'Condition': "'$(MSBuildProjectFullPath)' == '" + str((root / name).resolve()) + "'"})
        ET.SubElement(group, 'NuGetLockFilePath').text = str(destination.resolve())
        # Verifiers use framework-dependent apphosts for the current machine.
        # The coordinator's declared release RID list must not select packaging
        # graphs while it is referenced by this portable verification build.
        ET.SubElement(group, 'RuntimeIdentifiers').text = ''
        ET.SubElement(group, 'RuntimeIdentifier').text = ''
        records[relative] = {'source_sha256': sha(source), 'isolated_sha256': sha(destination),
                             'isolated_path': destination.relative_to(output).as_posix(),
                             'framework_graph_unchanged': True,
                             'runtime_sections_omitted': [framework for framework in original['dependencies'] if '/' in framework]}
    target = output / 'restore-locks.targets'
    ET.indent(project)
    ET.ElementTree(project).write(target, encoding='unicode')
    return {'runtime_profile': 'portable verification only', 'locks': records,
            'target': target.relative_to(output).as_posix(), 'target_sha256': sha(target),
            'arguments': ['/p:CustomAfterMicrosoftCommonTargets=' + str(target.resolve())]}


def restore_inputs_stable(output, inputs):
    return (sha(output / inputs['target']) == inputs['target_sha256'] and
            all(sha(output / value['isolated_path']) == value['isolated_sha256'] for value in inputs['locks'].values()))


def finish(root, output, record):
    for key in ('restore_inputs', 'renderer_restore_inputs'):
        if inputs := record.get(key):
            record[key + '_stable'] = restore_inputs_stable(output, inputs)
            if not record[key + '_stable']:
                record.update(passed=False, real_host_verified=False, error='Isolated restore inputs changed')
    record['source_stable'] = (set(source_files(root)) == set(record['source_hashes']) and
                              all((root / name).is_file() and sha(root / name) == value
                                  for name, value in record['source_hashes'].items())
                              )
    if not record['source_stable']:
        record.update(passed=False, real_host_verified=False)
    record['runner_succeeded'] = bool(record['source_stable'] and not record.get('error') and
                                      (record.get('passed') or record.get('real_host_verified')))
    (output / 'verification.json').write_text(json.dumps(record, indent=2) + '\n', encoding='utf-8')
    if not record['source_stable']:
        raise RuntimeError('Source or build input changed during verification; see ' + str(output))
    if any(record.get(key) is False for key in ('restore_inputs_stable', 'renderer_restore_inputs_stable')):
        raise RuntimeError('Isolated restore input changed during verification; see ' + str(output))
