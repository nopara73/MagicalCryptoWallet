#!/usr/bin/env python3
"""Run an inert managed verification tool through the production mcw host."""
import argparse, os, shutil, subprocess, sys, xml.etree.ElementTree as ET
from pathlib import Path
from evidence import assemblies, finish, new_run, restore_lock_inputs, sha, snapshot
ROOT=Path(__file__).resolve().parents[2]
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument("--host",required=True,type=Path)
parser.add_argument("--project",required=True,type=Path)
parser.add_argument("--property", action="append", default=[])
parser.add_argument("--evidence-dir", type=Path)
parser.add_argument("--output-artifacts", type=Path, help="Fresh synthetic output directory whose files are retained and hashed")
parser.add_argument("arguments",nargs=argparse.REMAINDER)
args=parser.parse_args()
project=args.project.resolve()
suffix=".exe" if os.name=="nt" else ""
tools=list(project.glob("*.csproj"))
if len(tools)!=1:raise RuntimeError("Expected one verification tool project")
name=tools[0].stem
assembly=ET.parse(tools[0]).findtext('./PropertyGroup/AssemblyName') or name
arguments=args.arguments[1:] if args.arguments[:1]==["--"] else args.arguments
output = args.evidence_dir.resolve() if args.evidence_dir else new_run(ROOT / '.artifacts/mcw-managed-tools', name)
output.mkdir(parents=True, exist_ok=True)
work = output / 'build'
work.mkdir(exist_ok=False)
record = snapshot(ROOT, output)
record.update(project=str(tools[0].relative_to(ROOT)), arguments=arguments, properties=args.property,
              native_sha256=sha(args.host.resolve()), production_release=False, passed=False)
try:
    if args.output_artifacts and args.output_artifacts.exists() and any(args.output_artifacts.iterdir()):
        raise RuntimeError('Verification output directory must be fresh')
    record['restore_inputs'] = restore_lock_inputs(ROOT, output, record['source_hashes'])
    command = ["dotnet","build",str(tools[0]),"-c","Release","-m:1",
                    "/p:UseSharedCompilation=false", "/p:BuildMcwHost=false", "/p:RestoreLockedMode=true",
                    '--artifacts-path', str(output / 'artifacts'), "-o", str(work),
                    *["/p:" + value for value in args.property], *record['restore_inputs']['arguments']]
    with (output / 'build.log').open('wb') as log:
        result = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, timeout=300)
    if result.returncode:
        print((output / 'build.log').read_text(encoding='utf-8', errors='replace'))
        result.check_returncode()
    shutil.copy2(args.host.resolve(),work/("mcw"+suffix))
    shutil.copy2(work/(assembly+suffix),work/("magicalcryptowallet"+suffix))
    record['assemblies_before'] = assemblies(work)
    # Explicit handles also capture diagnostics from Windows GUI-subsystem apps.
    result = subprocess.run([str(work/("mcw"+suffix)),"gui",*arguments],stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True,encoding='utf-8',errors='replace',timeout=300)
    print(result.stdout, end="")
    (output / 'run.log').write_text(result.stdout, encoding='utf-8')
    record['exit_code'] = result.returncode
    record['assemblies_after'] = assemblies(work)
    assert record['assemblies_before'] == record['assemblies_after'], 'Verified assembly changed during execution'
    assert sha(args.host.resolve()) == record['native_sha256'] == sha(work / ('mcw' + suffix))
    result.check_returncode()
    if args.output_artifacts:
        artifact_root = args.output_artifacts.resolve(strict=True)
        retained = output / 'outputs'
        record['output_hashes'] = {}
        for path in sorted(artifact_root.rglob('*')):
            if path.is_file():
                name = path.relative_to(artifact_root).as_posix()
                destination = retained / name
                destination.parent.mkdir(parents=True, exist_ok=True)
                captured = path.read_bytes()
                destination.write_bytes(captured)
                record['output_hashes'][name] = sha(destination)
                assert sha(path) == record['output_hashes'][name], 'Verification output changed while capturing'
        assert record['output_hashes'], 'Verification tool produced no output artifacts'
    record['passed'] = True
except BaseException as error:
    record.update(passed=False, error=str(error))
    raise
finally:
    finish(ROOT, output, record)
