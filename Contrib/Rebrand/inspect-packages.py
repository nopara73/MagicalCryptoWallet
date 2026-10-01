#!/usr/bin/env python3
"""Extract the produced installers and audit their payloads and independent identities."""
import argparse, hashlib, json, os, plistlib, shutil, subprocess, tarfile, zipfile
from pathlib import Path
import xml.etree.ElementTree as ET
ROOT=Path(__file__).resolve().parents[2]
APP_ID='io.github.nopara73.magicalcryptowallet'
NAME='Magical Crypto Wallet'
def run(*args,**kwargs): return subprocess.run([str(x) for x in args],check=True,**kwargs)
def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def extract_zip(path, destination):
    with zipfile.ZipFile(path) as archive:
        for item in archive.infolist():
            target=(destination/item.filename).resolve()
            if destination.resolve() not in target.parents and target!=destination.resolve():raise RuntimeError('Archive path escapes inspection directory')
        archive.extractall(destination)

parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--rid',required=True);args=parser.parse_args()
work=ROOT/'.artifacts/package-inspection'/args.rid
work.mkdir(parents=True,exist_ok=True)
dist=ROOT/'.artifacts/packages'/args.rid/'MagicalCryptoWallet'
packages=ROOT/'packages';payloads=[];identities={}
for path in sorted(packages.glob('*')):
    matches_rid = args.rid in path.name.lower() or (args.rid.startswith('osx') and ('macos-'+args.rid.split('-')[1]) in path.name.lower())
    if path.suffix=='.zip' and matches_rid:
        destination=work/(path.stem+'-zip');destination.mkdir(exist_ok=True)
        extract_zip(path,destination);payloads.append(destination)
    elif path.name.endswith('.tar.gz') and matches_rid:
        destination=work/(path.name[:-7]+'-tar');destination.mkdir(exist_ok=True)
        with tarfile.open(path) as archive: archive.extractall(destination,filter='data')
        payloads.append(destination)
if args.rid.startswith('win'):
    msi=next(packages.glob('*.msi'));destination=work/'msi';destination.mkdir(exist_ok=True)
    manifest=destination/'package.wxs'
    run('dark.exe','-nologo','-x',destination/'files','-o',manifest,msi)
    document=ET.parse(manifest);ns={'w':'http://schemas.microsoft.com/wix/2006/wi'}
    product=document.find('w:Product',ns);ids=json.loads((ROOT/'MagicalCryptoWallet.WindowsInstaller/identities.json').read_text())
    assert product is not None and product.attrib['Name']==NAME
    assert product.attrib['UpgradeCode'].strip('{}').upper()==ids['upgrade_code']
    properties=document.findall('.//w:ShortcutProperty',ns)
    assert len(properties)==2 and all(p.attrib['Key']=='System.AppUserModel.ID' and p.attrib['Value']==APP_ID for p in properties)
    expected={p.name+':'+sha(p):p for p in dist.rglob('*') if p.is_file()}
    canonical=destination/'payload';canonical.mkdir(exist_ok=True);files=0
    for file in document.findall('.//w:File',ns):
        source=Path(file.attrib['Source']);name=file.attrib.get('Name') or file.attrib.get('LongName')
        if not source.is_absolute():source=ROOT/source
        assert source.is_file(), 'Missing extracted MSI file: '+str(source)
        original=expected.get(str(name)+':'+sha(source))
        assert original is not None, 'MSI payload differs from the published application: '+str(name)
        target=canonical/original.relative_to(dist);target.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(source,target);files+=1
    assert files==len(list(dist.rglob('*')))-len([p for p in dist.rglob('*') if p.is_dir()])
    payloads.append(canonical);identities={'upgrade_code':ids['upgrade_code'],'product_name':product.attrib['Name'],'payload_files':files}
elif args.rid.startswith('linux'):
    arm=args.rid.endswith('arm64');deb=next(p for p in packages.glob('*.deb') if ('-arm64' in p.name)==arm)
    destination=work/'deb';destination.mkdir(exist_ok=True);run('dpkg-deb','-x',deb,destination)
    package=subprocess.check_output(['dpkg-deb','-f',str(deb),'Package']).decode().strip()
    architecture=subprocess.check_output(['dpkg-deb','-f',str(deb),'Architecture']).decode().strip()
    assert package=='magicalcryptowallet' and architecture==('arm64' if arm else 'amd64')
    desktop=destination/'usr/share/applications'/f'{APP_ID}.desktop'
    assert f'Name={NAME}' in desktop.read_text() and f'Icon={APP_ID}' in desktop.read_text()
    payloads.append(destination);identities={'package':package,'architecture':architecture,'desktop_id':APP_ID}
    appimage=next((p for p in packages.glob('*.AppImage') if ('-arm64' in p.name)==arm),None)
    if appimage:
        destination=work/'appimage';destination.mkdir(exist_ok=True);appimage.chmod(0o755)
        run(appimage,'--appimage-extract',cwd=destination,stdout=subprocess.DEVNULL)
        assert (destination/'squashfs-root'/f'{APP_ID}.desktop').exists()
        payloads.append(destination/'squashfs-root')
else:
    arm=args.rid.endswith('arm64');dmg=next(p for p in packages.glob('*.dmg') if ('-arm64' in p.name)==arm)
    mount=work/'mounted';mount.mkdir(exist_ok=True)
    run('hdiutil','attach','-readonly','-nobrowse','-mountpoint',mount,dmg,stdout=subprocess.DEVNULL)
    try:
        app=mount/(NAME+'.app');plist=plistlib.loads((app/'Contents/Info.plist').read_bytes())
        assert plist['CFBundleIdentifier']==APP_ID and plist['CFBundleDisplayName']==NAME and plist['CFBundleExecutable']=='magicalcryptowallet'
        assert sha(app/'Contents/Resources/MagicalCryptoWalletLogo.icns')==sha(ROOT/'Contrib/Assets/MagicalCryptoWalletLogo.icns')
        extracted=work/'app';shutil.copytree(app,extracted,symlinks=True,dirs_exist_ok=True);payloads.append(extracted)
        identities={k:plist[k] for k in ('CFBundleIdentifier','CFBundleDisplayName','CFBundleExecutable','CFBundleVersion')}
    finally:run('hdiutil','detach',mount,stdout=subprocess.DEVNULL)
for executable in ('magicalcryptowallet','magicalcryptowalletd','magicalcryptowallet-coordinator'):
    assert (dist/(executable+('.exe' if args.rid.startswith('win') else ''))).is_file()
run(sys_executable:=__import__('sys').executable,ROOT/'Contrib/Rebrand/audit.py','--artifacts',*payloads)
report={'rid':args.rid,'identities':identities,'extracted_payloads':[str(p.relative_to(ROOT)) for p in payloads],
        'packages':{p.name:sha(p) for p in packages.iterdir() if p.is_file() and p.suffix!='.wixpdb'}}
(ROOT/'.artifacts/package-inspection'/f'{args.rid}.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report))
