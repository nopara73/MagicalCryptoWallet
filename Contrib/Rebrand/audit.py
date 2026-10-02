#!/usr/bin/env python3
"""Audit tracked names/content, immutable protocol sources, generated code, assets and extracted packages."""
import argparse, hashlib, json, re, subprocess, sys
from pathlib import Path
ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent

def digest(data): return hashlib.sha256(data).hexdigest()
def audit(artifacts):
    policy = json.loads((HERE / 'policy.json').read_text())
    exceptions = json.loads((HERE / 'exceptions.json').read_text())
    forbidden = re.compile(policy['forbidden_pattern'], re.I)
    removed_controls = re.compile(policy['removed_coin_control_pattern'], re.I)
    port = re.compile(r'(?<![A-Za-z0-9.])371\d\d(?![A-Za-z0-9.])')
    allowed = {(e['path'], e['line_sha256']) for e in exceptions}
    data_files = {'Contrib/Rebrand/policy.json', 'Contrib/Rebrand/exceptions.json'}
    names = subprocess.check_output(['git','ls-files','-z'], cwd=ROOT).decode().split('\0')
    failures=[]; count=0; used=set()
    for entry in filter(None, subprocess.check_output(['git','ls-files','--stage','-z'], cwd=ROOT).decode().split('\0')):
        metadata, name = entry.split('\t', 1)
        if '/BundledApps/Binaries/' in name and '/win-' not in name and name.rsplit('/',1)[-1] in ('tor','bitcoind'):
            if metadata.split()[0] != '100755': failures.append('Unix executable mode: '+name)
    for name in filter(None, names):
        path=ROOT/name
        if not path.is_file(): continue
        count+=1
        if forbidden.search(name) and name not in data_files: failures.append('Path: '+name)
        raw=path.read_bytes()
        if digest(raw) in policy['old_artwork_sha256']: failures.append('Old artwork: '+name)
        if name in data_files: continue
        try: text=raw.decode('utf-8-sig')
        except UnicodeDecodeError: continue
        is_application_source = name.startswith('MagicalCryptoWallet') and not name.split('/')[0].endswith('Tests') and path.suffix in ('.cs', '.axaml')
        if is_application_source and (removed_controls.search(name) or removed_controls.search(text)):
            failures.append('Removed coin selection control: '+name)
        for number,line in enumerate(text.splitlines(),1):
            if forbidden.search(line) or port.search(line):
                key=(name,digest(line.encode()))
                if key in allowed: used.add(key)
                else: failures.append(f'Content: {name}:{number}: {line[:180]}')
    for e in exceptions:
        path=ROOT/e['path']
        if not path.exists() or e['line_sha256'] not in {digest(l.encode()) for l in path.read_text(encoding='utf-8-sig').splitlines()}:
            failures.append('Stale exception: '+e['path']+':'+str(e['line']))
    upstream=json.loads((ROOT/'ThirdParty/WabiSabi/UPSTREAM.json').read_text())
    for name, expected in policy.get('immutable_wallet_fixtures', {}).items():
        if digest((ROOT/name).read_bytes().replace(b'\r\n', b'\n')) != expected:
            failures.append('Wallet fixture drift: '+name)
    # Algorithms and published vectors are immutable; only the randomness identifier
    # and attribution comments differ from source. Build and test adapters are listed.
    immutable=0
    for name,expected in upstream['source_sha256'].items():
        if not (name.startswith(('c/src/','c/include/','c/tests/','csharp/WabiSabi/Crypto/')) or name.endswith('Vectors.json')): continue
        target=ROOT/'ThirdParty/WabiSabi'/name.replace('WasabiRandom','WalletRandom')
        raw=target.read_bytes().replace(b'\r\n', b'\n')
        if digest(raw)!=expected:
            normalized=raw.decode().replace('WalletRandom','WasabiRandom')
            if name.startswith('c/'):
                normalized=normalized.replace('Magical Crypto Wallet','WalletWasabi').replace('MagicalCryptoWallet','WalletWasabi')
            if digest(normalized.encode())!=expected: failures.append('Protocol source/vector drift: '+name)
        immutable+=1
    # Generated application source is audited after its normal build.
    generated=0
    for folder in ROOT.glob('MagicalCryptoWallet*/obj'):
        for path in folder.rglob('*.cs'):
            if 'GeneratedFiles' in path.parts:
                generator=path.parts[path.parts.index('GeneratedFiles')+1]
                if not generator.startswith('MagicalCryptoWallet.'):
                    continue # Inactive caches from an earlier checkout are not compiler inputs.
            generated+=1
            text = path.read_text(encoding='utf-8-sig')
            if forbidden.search(text): failures.append('Generated identity: '+str(path.relative_to(ROOT)))
            if not folder.parent.name.endswith('Tests') and removed_controls.search(text): failures.append('Removed generated coin selection control: '+str(path.relative_to(ROOT)))
    for artifact in artifacts:
        folder=Path(artifact).resolve()
        if not folder.is_dir(): failures.append('Missing extracted package: '+str(folder)); continue
        for path in folder.rglob('*'):
            if not path.is_file(): continue
            if forbidden.search(path.relative_to(folder).as_posix()): failures.append('Package path: '+str(path))
            raw=path.read_bytes()
            if digest(raw) in policy['old_artwork_sha256']: failures.append('Package artwork: '+str(path))
            if path.name.startswith('libwabisabi') and path.suffix in ('.so','.dll','.dylib'):
                if forbidden.search(raw.decode('latin1')): failures.append('Native symbol/identity: '+str(path))
            if path.name in ('LICENSE.md','NOTICE.md'):
                if raw!=(ROOT/path.name).read_bytes(): failures.append('Altered original notice: '+str(path))
                continue
            if path.suffix.lower() in ('.json','.xml','.config','.plist','.desktop','.wxs','.wxl','.txt','.md'):
                try: text=raw.decode('utf-8-sig')
                except UnicodeDecodeError: continue
                if forbidden.search(text) or port.search(text): failures.append('Package text: '+str(path))
        subprocess.run(['dotnet','run','--project',str(HERE/'AssemblyAudit'),'-c','Release','--',str(HERE/'policy.json'),str(folder)],check=True,cwd=ROOT)
    for problem in failures: print(problem,file=sys.stderr)
    print(json.dumps({'tracked_files':count,'recorded_exceptions':len(exceptions),'immutable_protocol_files':immutable,'immutable_wallet_fixtures':len(policy.get('immutable_wallet_fixtures', {})),'generated_sources':generated,'artifact_folders':len(artifacts),'failures':len(failures)}))
    return 1 if failures else 0

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--artifacts',nargs='*',default=[])
    args=parser.parse_args()
    sys.exit(audit(args.artifacts))
