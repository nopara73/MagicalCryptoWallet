"""Development-only independent QR conformance; no app dependency or camera activation."""
import argparse
from pathlib import Path
from PIL import Image
import json,subprocess,hashlib
parser=argparse.ArgumentParser();parser.add_argument('--output',type=Path,required=True);args=parser.parse_args()
root=Path('.artifacts/qr-scanning');out=args.output;out.mkdir(parents=True,exist_ok=False)
source=json.loads((root/'matrix-1/synthetic-corpus.json').read_text());cases=[]
def solve(eq):
 for i in range(8):
  best=max(range(i,8),key=lambda j:abs(eq[j][i]));eq[i],eq[best]=eq[best],eq[i]
  d=eq[i][i];eq[i]=[v/d for v in eq[i]]
  for j in range(8):
   if j!=i:
    f=eq[j][i];eq[j]=[a-f*b for a,b in zip(eq[j],eq[i])]
 return [r[8] for r in eq]
for version in [1,3,5,10,25,40]:
 c=next(c for c in source if c.get('kind')=='byte-all-versions-levels-masks' and c['version']==version and c['level']==2)
 n=c['size'];scale=5;e=(n+8)*scale
 base=Image.new('L',(e,e),245);p=base.load()
 for y in range(n):
  for x in range(n):
   if c['modules'][y*n+x]=='1':
    for dy in range(scale):
     for dx in range(scale):p[(x+4)*scale+dx,(y+4)*scale+dy]=15
 for skew in [0.05,0.12]:
  margin=30;delta=e*skew
  dst=[(margin+delta,margin),(e+margin,margin+delta),(e+margin-delta,e+margin),(margin,e+margin-delta)]
  src=[(0,0),(e,0),(e,e),(0,e)]
  eq=[]
  for (x,y),(u,v) in zip(dst,src):
   eq.extend([[x,y,1,0,0,0,-u*x,-u*y,u],[0,0,0,x,y,1,-v*x,-v*y,v]])
  im=base.transform((e+2*margin,e+2*margin),Image.Transform.PERSPECTIVE,solve(eq),resample=Image.Resampling.NEAREST,fillcolor=245)
  path=out/f'persp-v{version}-{skew}.pgm';path.write_bytes(f'P5\n{im.width} {im.height}\n255\n'.encode()+im.tobytes());cases.append(dict(path=str(path.resolve()),version=version,skew=skew,expected=c['expected']))
r=subprocess.run([str((root/'portable/oracle.exe').resolve())],input=''.join(f'I {c["path"]}\n' for c in cases),capture_output=True,text=True,timeout=60)
assert r.returncode==0,r.stderr
fail=[]
for c,line in zip(cases,r.stdout.splitlines(),strict=True):
 a=bytes.fromhex(line.split()[-1]).decode() if line.startswith('OK ') else None
 if a!=c['expected']:fail.append(dict(version=c['version'],skew=c['skew'],actual=a))
result=dict(cases=len(cases),passed=len(cases)-len(fail),failures=fail,binary_sha256=hashlib.sha256((root/'portable/oracle.exe').read_bytes()).hexdigest());(out/'result.json').write_text(json.dumps(result,indent=2));print(json.dumps(result,indent=2))

assert not fail
