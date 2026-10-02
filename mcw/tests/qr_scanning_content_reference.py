"""Development-only independent QR conformance; no app dependency or camera activation."""
import argparse
from pathlib import Path
import importlib.util,sys,subprocess,json,hashlib
parser=argparse.ArgumentParser();parser.add_argument('--output',type=Path,required=True);args=parser.parse_args()
root=Path('.artifacts/qr-scanning')
assert hashlib.sha256((root/'oracles/qrcodegen-1.8.0.py').read_bytes()).hexdigest()=='b089855caf16185c61421ea4927c1b213cf9468940d71fa8ab11ef83662dcc84'
spec=importlib.util.spec_from_file_location('q',root/'oracles/qrcodegen-1.8.0.py');q=importlib.util.module_from_spec(spec);spec.loader.exec_module(q)
sys.path.insert(0,str(root/'oracles/segno'));import segno
Q,S=q.QrCode,q.QrSegment
cases=[]
def add(qr,text,name):
 if hasattr(qr,'get_size'):
  n=qr.get_size();data=''.join('1' if qr.get_module(x,y) else '0' for y in range(n) for x in range(n))
 else:
  n=len(qr.matrix);data=''.join(str(v) for row in qr.matrix for v in row)
 cases.append(dict(size=n,modules=data,expected=text,kind=name))
for text,mode in [('0'*7089,'numeric'),('A'*4296,'alphanumeric'),('a'*2953,'byte'),('', 'byte'),('A\0B','byte'),('Grüße, 🦀','byte')]:
 if mode=='numeric':segment=S.make_numeric(text)
 elif mode=='alphanumeric':segment=S.make_alphanumeric(text)
 else:segment=S.make_bytes(text.encode())
 segs=([S.make_eci(26)] if any(ord(c)>127 for c in text) else [])+[segment]
 add(Q.encode_segments(segs,Q.Ecc.LOW,minversion=40,maxversion=40,boostecl=False),text,'maximum-and-exact-'+mode)
for text,encoding,mode in [('漢字日本語','shift_jis','kanji'),('汉字中文','gb2312','hanzi'),('Grüße','iso8859-1','byte'),('Καλημέρα','iso8859-7','byte'),('مرحبا','cp1256','byte'),('中文','big5','byte'),('한글','euc_kr','byte'),('🦀','gb18030','byte'),('abc\0€','utf-16-be','byte')]:
 for version in [4,10,27,40]:
  qr=segno.make(text,version=version,error='M',mode=mode,encoding=encoding,eci=mode=='byte',micro=False,boost_error=False)
  add(qr,text,'segno-'+encoding+'-'+mode)
inputs=''.join(f'M {c["size"]} {c["modules"]}\n' for c in cases)
r=subprocess.run([str((root/'portable/oracle.exe').resolve())],input=inputs,capture_output=True,text=True,timeout=120)
assert r.returncode==0,r.stderr
fail=[]
for c,line in zip(cases,r.stdout.splitlines(),strict=True):
 fields=line.split(' ');actual=bytes.fromhex(fields[-1]).decode() if fields[0]=='OK' else None
 if actual!=c['expected']:fail.append(dict(kind=c['kind'],expected_length=len(c['expected']),actual=actual))
out=args.output;out.mkdir(parents=True,exist_ok=False)
(out/'corpus.json').write_text(json.dumps(cases,ensure_ascii=False),encoding='utf-8')
(out/'result.json').write_text(json.dumps(dict(cases=len(cases),passed=len(cases)-len(fail),failures=fail,segno=segno.__version__,binary_sha256=hashlib.sha256((root/'portable/oracle.exe').read_bytes()).hexdigest()),indent=2),encoding='utf-8')
print((out/'result.json').read_text())
assert not fail
