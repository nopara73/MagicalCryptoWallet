"""Brotli/content verification against installed .NET BCL, synthetic data only.
No oracle, Python, C# or external library is shipped with the Rust service.
"""
import argparse
import hashlib
import json
import pathlib
import random
import subprocess
import zlib


class Bits:
    def __init__(self):
        self.value = 0
        self.n = 0
        self.data = bytearray()

    def raw(self, value, n):
        self.value |= value << self.n
        self.n += n
        while self.n >= 8:
            self.data.append(self.value & 255)
            self.value >>= 8
            self.n -= 8

    def simple(self, symbols, width):
        self.raw(1, 2)
        self.raw(len(symbols)-1, 2)
        for symbol in symbols:
            self.raw(symbol, width)

    def finish(self):
        if self.n:
            self.data.append(self.value)
        return bytes(self.data)


def ferment(word, at):
    if word[at] < 192:
        if 97 <= word[at] <= 122:
            word[at] ^= 32
        return 1
    if word[at] < 224:
        if at+1 < len(word):
            word[at+1] ^= 32
        return 2
    if at+2 < len(word):
        word[at+2] ^= 5
    return 3


def transformed(word, prefix, kind, suffix):
    word = bytearray(word)
    if 3 <= kind <= 11:
        word = word[kind-2:]
    if 12 <= kind <= 20:
        word = word[:max(0, len(word)-(kind-11))]
    if kind == 1 and word:
        ferment(word, 0)
    if kind == 2:
        at = 0
        while at < len(word):
            at += ferment(word, at)
    return prefix + bytes(word) + suffix


def dictionary_stream(length, word_id, result):
    # An independently assembled single final compressed meta-block. No literal
    # insertion, one command and one explicit dictionary distance. Empty
    # transformed words are followed by a literal-only command to finish MLEN.
    bases = [2,3,4,5,6,7,8,9,10,12,14,18,22,30,38,54,70,102,134,198,326,582,1094,2118]
    extra = [0,0,0,0,0,0,0,0,1,1,2,2,3,3,4,4,5,5,6,7,8,9,10,24]
    cc = max(i for i, base in enumerate(bases) if base <= length)
    command = (128 if cc < 8 else 192) + (cc & 7)
    distance = word_id+1
    for ds in range(16, 64):
        nd = 1+((ds-16) >> 1)
        offset = ((2+((ds-16) & 1)) << nd)-4
        if offset+1 <= distance <= offset+(1 << nd):
            dx = distance-offset-1
            break
    else:
        raise AssertionError("distance representation")
    b = Bits()
    b.raw(0, 1)  # WBITS 16
    b.raw(1, 1)  # ISLAST
    b.raw(0, 1)  # not empty
    b.raw(0, 2)  # MLEN uses four nibbles
    b.raw(max(1, len(result))-1, 16)
    b.raw(0, 3)  # three single-block categories
    b.raw(0, 6)  # postfix/direct
    b.raw(0, 2)  # LSB6 context
    b.raw(0, 2)  # single literal/distance trees
    b.simple([65], 8)
    b.simple([command] if result else [command, 8], 10)
    b.simple([ds], 6)
    if not result:
        b.raw(1, 1)  # sorted command 8 gets code0, dictionary command code1
    b.raw(length-bases[cc], extra[cc])
    b.raw(dx, nd)
    if not result:
        b.raw(0, 1)
    return b.finish(), result or b"A"


def mapped_literals(mode, switching, rle, mtf, luts):
    """RFC-assembled literal/context/block-switch vectors, .NET-verified below."""
    def context(p1, p2):
        if mode == 0:
            return p1 & 63
        if mode == 1:
            return p1 >> 2
        if mode == 2:
            return luts[0][p1] | luts[1][p2]
        return (luts[2][p1] << 3) | luts[2][p2]
    if switching:
        a, c = 65, 66
        mapping = [0]*64 + [1]*64
    else:
        a = 65
        for c in (66, 68, 32, 0, 128, 240):
            choices = [(context(0, 0), 0), (context(a, 0), 1),
                       (context(c, a), 0), (context(a, c), 1)]
            assignment = {}
            valid = True
            for k, v in choices:
                if k in assignment and assignment[k] != v:
                    valid = False
                    break
                assignment[k] = v
            if not valid:
                continue
            mapping = [assignment.get(i, 0) for i in range(64)]
            break
        else:
            raise AssertionError("context vector construction")
    encoded = mapping.copy()
    if mtf:
        order = list(range(256))
        for i, value in enumerate(mapping):
            index = order.index(value)
            encoded[i] = index
            order.pop(index)
            order.insert(0, value)
    b = Bits()
    b.raw(0, 1); b.raw(1, 1); b.raw(0, 1); b.raw(0, 2); b.raw(3, 16)
    if switching:
        b.raw(1, 1); b.raw(0, 3)  # two literal block types
        b.simple([1], 2)         # next block type, modulo 2
        b.simple([0], 5)         # count base 1 with 2 extra bits
        b.raw(0, 2)
    else:
        b.raw(0, 1)
    b.raw(0, 1); b.raw(0, 1)  # single command and distance blocks
    b.raw(0, 2); b.raw(0, 4)
    for _ in range(2 if switching else 1):
        b.raw(mode, 2)
    b.raw(1, 1); b.raw(0, 3)  # two literal trees
    if rle:
        b.raw(1, 1); b.raw(3, 4)  # RLE prefix cap 4; use run symbol 1
        b.simple([5, 1, 0], 3)   # 5=>0; 0=>10; 1=>11
        i = 0
        while i < len(encoded):
            if encoded[i]:
                b.raw(0, 1); i += 1
            elif i+1 < len(encoded) and encoded[i+1] == 0:
                count = 3 if i+2 < len(encoded) and encoded[i+2] == 0 else 2
                b.raw(3, 2); b.raw(count-2, 1); i += count
            else:
                b.raw(1, 2); i += 1
    else:
        b.raw(0, 1); b.simple([0, 1], 1)
        for value in encoded:
            b.raw(value, 1)
    b.raw(int(mtf), 1)
    b.raw(0, 1)  # one distance tree
    b.simple([a], 8); b.simple([c], 8)
    b.simple([32], 10); b.simple([0], 6)  # insert 4 literals, no copy
    if switching:
        for _ in range(3):
            b.raw(0, 2)  # literal block switches after each count of 1
    return b.finish(), bytes((a, c, a, c))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--driver", required=True)
    parser.add_argument("--oracle", required=True)
    parser.add_argument("--evidence", required=True)
    args = parser.parse_args()
    root = pathlib.Path(args.evidence)
    data_root = pathlib.Path(__file__).resolve().parents[1]/"src/content_service/data"
    rng = random.Random(0x7932)
    cases = []

    def oracle(requests):
        results = []
        for start in range(0, len(requests), 24):
            result = subprocess.run(["dotnet", args.oracle], input="\n".join(requests[start:start+24])+"\n",
                                    text=True, capture_output=True, timeout=90)
            assert result.returncode == 0, result.stderr
            lines = result.stdout.splitlines()
            assert len(lines) == len(requests[start:start+24]), result.stdout[:300]
            results.extend(lines)
        return results

    def add(name, data, expected, mode="B", encoding="-", error=None, abort=None):
        request="\t".join((mode, encoding, data.hex()))
        if abort is not None:
            request += "\t"+str(abort)
        cases.append(dict(name=name, request=request, expected=expected, consumed=len(data), error=error))

    payloads = [b"", b"hello", bytes(range(256)), b"a"*20000, b"xy"*15000,
                b"<html><body>the information and the compression application</body></html>"*40,
                ('{"synthetic":true,"price":42000,"text":"\u00e9\u4e2d\u6587'+chr(0x1f680)+'"}').encode()*70]
    for i in range(36):
        size = rng.randrange(1, 16000)
        data = rng.randbytes(size) if i%3 == 0 else bytes(rng.randrange(16)+32 for _ in range(size))
        payloads.append(data)
    requests=[]
    descriptions=[]
    for i,data in enumerate(payloads):
        for quality in (0,1,3,6,9,11):
            window=(10,12,16,18,22,24)[quality%6]
            requests.append(f"C\t{quality}\t{window}\t{data.hex()}")
            descriptions.append((i,data,quality,window))
    for line,(i,data,quality,window) in zip(oracle(requests),descriptions):
        parts=line.split("\t");assert parts[0]=="OK",(i,quality,line)
        packed=bytes.fromhex(parts[1])
        add(f"encoder-{i}-{quality}-{window}",packed,data)
        add(f"http-br-{i}-{quality}",packed,data,mode="H",encoding="br")
        if i<5 and quality in (0,6):
            for n in range(len(packed)):
                add(f"truncated-{i}-{quality}-{n}",packed[:n],None,error="Truncated")
        if i==5 and quality==11:
            add("brotli-cancel",packed,None,error="Aborted",abort=1)
    # Every RFC window (10..24) and every .NET encoder quality (0..11).
    requests=[]
    descriptions=[]
    for i,data in enumerate((payloads[1], payloads[5], payloads[6])):
        for window in range(10,25):
            for quality in range(12):
                requests.append(f"C\t{quality}\t{window}\t{data.hex()}")
                descriptions.append((i,data,quality,window))
    for line,(i,data,quality,window) in zip(oracle(requests),descriptions):
        parts=line.split("\t"); assert parts[0]=="OK",line
        add(f"window-quality-{i}-{window}-{quality}",bytes.fromhex(parts[1]),data)
    # Exhaustive normative dictionary lengths/transforms, including multilingual
    # Ferment behavior. Both the Rust decoder and .NET oracle decode the same
    # independently assembled bitstream, not only self-roundtrips.
    dictionary=(data_root/"dictionary.bin").read_bytes()
    import re
    tables=(data_root/"tables.rs").read_text()
    transforms=[]
    for p,k,s in re.findall(r'prefix: b"([^"]*)", kind: (\d+), suffix: b"([^"]*)"',tables):
        transforms.append((bytes.fromhex(p.replace("\\x","")),int(k),bytes.fromhex(s.replace("\\x",""))))
    assert len(transforms)==121
    depths=[0,0,0,0,10,10,11,11,10,10,10,10,10,9,9,8,7,7,8,7,7,6,6,5,5]
    offset=0
    dictionary_vectors=[]
    for length in range(4,25):
        count=1<<depths[length]
        for transform,(prefix,kind,suffix) in enumerate(transforms):
            index=(transform*37)%count
            word=dictionary[offset+index*length:offset+(index+1)*length]
            expected=transformed(word,prefix,kind,suffix)
            packed,expected=dictionary_stream(length,transform*count+index,expected)
            dictionary_vectors.append((f"dictionary-{length}-{transform}",packed,expected))
        offset+=count*length
    assert offset==len(dictionary)
    luts=[]
    for n in range(3):
        raw=re.search(rf'const LUT{n}: \[u8; 256\] = \[(.*?)\];',tables,re.S).group(1)
        luts.append([int(v) for v in re.findall(r'\d+',raw)])
        assert len(luts[-1])==256
    mapped=[]
    for mode in range(4):
        for switching in (False,True):
            for rle in (False,True):
                for mtf in (False,True):
                    packed,expected=mapped_literals(mode,switching,rle,mtf,luts)
                    mapped.append((f"mapped-{mode}-{switching}-{rle}-{mtf}",packed,expected))
    for line,(name,packed,expected) in zip(oracle(["D\t"+p.hex() for _,p,_ in mapped]),mapped):
        parts=line.split("\t")
        assert parts[0]=="OK" and int(parts[1])==len(packed) and bytes.fromhex(parts[2])==expected,(name,line,packed.hex())
        add(name,packed,expected)
    requests=["D\t"+packed.hex() for _,packed,_ in dictionary_vectors]
    for line,(name,packed,expected) in zip(oracle(requests),dictionary_vectors):
        parts=line.split("\t")
        assert parts[0]=="OK",(name,line,packed.hex())
        assert int(parts[1])==len(packed) and bytes.fromhex(parts[2])==expected,(name,line,expected)
        add(name,packed,expected)

    # Layered HTTP response bodies, explicit gzip/zlib mapping and typed rejection.
    requests=[]
    nested=[]
    for i,data in enumerate(payloads[:12]):
        obj=zlib.compressobj(wbits=31)
        packed=obj.compress(data)+obj.flush()
        requests.append(f"C\t9\t22\t{packed.hex()}")
        nested.append((i,data))
        add(f"http-gzip-{i}",packed,data,mode="H",encoding="gzip")
        z=zlib.compress(data)
        add(f"http-deflate-{i}",z,data,mode="H",encoding="deflate")
        add(f"http-deflate-trailing-{i}",z+b"tail",None,mode="H",encoding="deflate",error="Compression(TrailingData)")
    for line,(i,data) in zip(oracle(requests),nested):
        packed=bytes.fromhex(line.split("\t")[1])
        add(f"http-gzip-br-{i}",packed,data,mode="H",encoding=" GZip,\tbr ".replace("\t"," "))
    add("invalid-window",b"\x11",None,error="InvalidWindow")
    add("unsupported",b"encoded",None,mode="H",encoding="unknown",error="UnsupportedEncoding")
    add("invalid-empty-coding",b"encoded",None,mode="H",encoding="gzip,",error="InvalidEncoding")
    add("cancel-before",b"",None,mode="H",encoding="br",error="Aborted",abort=0)
    add("identity",b"unencoded synthetic data",b"unencoded synthetic data",mode="H")

    fixture_path=root/"reference-fixtures.jsonl"
    with fixture_path.open("w",encoding="utf-8",newline="\n") as f:
        for case in cases:
            record={k:v for k,v in case.items() if k!="expected"}
            if case["expected"] is not None:
                record["expected_sha256"]=hashlib.sha256(case["expected"]).hexdigest()
                record["expected_length"]=len(case["expected"])
            f.write(json.dumps(record,sort_keys=True,separators=(",",":"))+"\n")
    for start in range(0,len(cases),24):
        batch=cases[start:start+24]
        result=subprocess.run([args.driver],input="\n".join(c["request"] for c in batch)+"\n",
                              text=True,capture_output=True,timeout=90)
        assert result.returncode==0,result.stderr
        lines=result.stdout.splitlines()
        assert len(lines)==len(batch),(len(lines),len(batch))
        for line,case in zip(lines,batch):
            parts=line.split("\t")
            if case["error"] is not None:
                assert parts[0]=="ERR" and parts[1].startswith(case["error"]),(case["name"],line)
            else:
                assert parts[0]=="OK",(case["name"],line)
                assert int(parts[1])==case["consumed"],(case["name"],parts[:4])
                assert bytes.fromhex(parts[4])==case["expected"],case["name"]
    report=dict(total=len(cases),dictionary_transform_vectors=len(dictionary_vectors),context_block_vectors=len(mapped),
                window_quality_vectors=len(descriptions),payloads=len(payloads),
                reference="Installed .NET 10 System.IO.Compression.BrotliEncoder/BrotliDecoder; independent verification only",
                fixture_sha256=hashlib.sha256(fixture_path.read_bytes()).hexdigest(),fixture_path=str(fixture_path),
                production_integrated=False)
    (root/"differential.json").write_text(json.dumps(report,indent=2)+"\n",encoding="utf-8")
    print(json.dumps(report,indent=2))


if __name__=="__main__":
    main()
