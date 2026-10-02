"""Development-only independent QR conformance; no app dependency or camera activation."""
import argparse
from PIL import Image
from pathlib import Path
import hashlib
import subprocess,json
parser=argparse.ArgumentParser();parser.add_argument('--output',type=Path,required=True);args=parser.parse_args()
out=args.output;out.mkdir(parents=True,exist_ok=False)
expected=['tb1ql27ya3gufs5h0ptgjhjd0tm52fq6q0xrav7xza','tb1qfas0k9rn8daqggu7wzp2yne9qdd5fr5wf2u478','tb1qutgpgraaze3hqnvt2xyw5acsmd3urprk3ff27d',"Let's see a Zebra.",'https://twitter.com/SimonHearne']
names=['AddressTest1.png','AddressTest2.png','QrByPhone.jpg','QRwithZebraBackground.png','qr-embed-logos.png']
paths=[];dims=[]
for name in names:
 im=Image.open(Path('MagicalCryptoWallet.Tests/UnitTests/QrDecode/QrResources')/name).convert('L');w,h=im.size
 p=out/(name+'.pgm');p.write_bytes(f'P5\n{w} {h}\n255\n'.encode()+im.tobytes());paths.append(p.resolve());dims.append([w,h])
r=subprocess.run([str(Path('.artifacts/qr-scanning/portable/oracle.exe').resolve())],input=''.join(f'I {p}\n' for p in paths),capture_output=True,text=True,timeout=30)
assert r.returncode==0,r.stderr
lines=r.stdout.splitlines();assert len(lines)==len(names)
actual=[bytes.fromhex(l.split()[-1]).decode() if l.startswith('OK ') else None for l in lines]
res=dict(cases=len(names),passed=sum(a==e for a,e in zip(actual,expected)),results=[dict(name=n,dimensions=d,expected=e,actual=a) for n,d,e,a in zip(names,dims,expected,actual)],input_role='repository test fixtures only, no user camera',binary_sha256=hashlib.sha256(Path('.artifacts/qr-scanning/portable/oracle.exe').read_bytes()).hexdigest(),normalization_oracle='Pillow gray conversion; actual Skia boundary is checked by qr_scanning_managed_verify.ps1')
(out/'result.json').write_text(json.dumps(res,indent=2),encoding='utf-8');print(json.dumps(res,indent=2))

assert all(a==e for a,e in zip(actual,expected,strict=True)),res
