#!/usr/bin/env python3
"""Exercise the actual mcw host/managed pipe with synthetic children and payloads."""
import argparse, json, os, shutil, signal, subprocess, tempfile, time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--binary", type=Path, required=True)
args = parser.parse_args()
binary = args.binary.resolve()
subprocess.run(["dotnet","build",str(ROOT / "Contrib/Mcw/BridgeProbe"),"-c","Release"],check=True)
source = ROOT / "Contrib/Mcw/BridgeProbe/bin/Release/net10.0"
evidence = ROOT / ".artifacts/mcw-evidence";evidence.mkdir(parents=True,exist_ok=True)
with tempfile.TemporaryDirectory(prefix="mcw spaces 你好 ", dir=ROOT / ".artifacts") as temporary:
    work=Path(temporary);shutil.copytree(source,work,dirs_exist_ok=True)
    suffix=".exe" if os.name=="nt" else ""
    shutil.copy2(binary,work / ("mcw"+suffix))
    for child in ("magicalcryptowallet",):
        shutil.copy2(work / ("BridgeProbe"+suffix),work / (child+suffix))
    host=work / ("mcw"+suffix)
    results={}
    def run(action, expected=0, mode="gui", timeout=180):
        report=work / (action+".json")
        result=subprocess.run([str(host),mode,action,str(report)],stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=timeout)
        assert result.returncode==expected, (action,result.returncode,result.stderr.decode(errors="replace")[-2000:])
        results[action]={"exit":result.returncode,"report":report.read_text() if report.exists() else None}
    run("qr")
    run("exit-7",7)
    run("restart",7)
    run("crash",7)
    crash=json.loads(results["crash"]["report"])
    assert "private synthetic exception" in crash["arguments"]
    assert "private synthetic exception" not in " ".join(crash["processArguments"]), "Crash payload leaked into argv"
    assert "private synthetic exception" not in subprocess.check_output([str(host),"--help"]).decode()
    run("update",1)
    run("bad-version",1)
    run("bad-length",1)
    run("truncated",1)
    run("unknown-operation")
    run("early-exit",1)
    run("unexpected-exit",1)
    run("no-handshake",1,timeout=150)
    # Parent death closes private pipes. The real managed adapter requests orderly
    # shutdown; on Windows the private job also prevents orphaned descendants.
    report=work / "parent-exit.txt"
    parent=subprocess.Popen([str(host),"gui","wait",str(report)],stdout=subprocess.PIPE,stderr=subprocess.PIPE)
    try:
        deadline=time.monotonic()+15
        while not report.exists() and time.monotonic()<deadline: time.sleep(0.05)
        assert report.exists(),"Managed child did not start"
        pid=int(report.read_text().splitlines()[0])
        if os.name=="nt":
            script = f"$p=Get-Process -Id {parent.pid}; @($p.Modules | ForEach-Object {{ $_.FileName }}) | ConvertTo-Json -Compress"
            modules=json.loads(subprocess.check_output(["pwsh","-NoProfile","-Command",script]).decode("utf-8-sig"))
            system=Path(os.environ["SystemRoot"]).resolve()
            assert all(Path(path).resolve()==host or system in Path(path).resolve().parents for path in modules), "mcw loaded a non-OS runtime"
            results["loaded-modules"]=modules
        if os.name!="nt":
            parent.send_signal(signal.SIGTERM)
            assert parent.wait(timeout=15)==0
            assert "shutdown" in report.read_text(),"Child did not shut down gracefully"
        else:
            parent.kill();parent.wait(timeout=15)
            script=f"if (Get-Process -Id {pid} -ErrorAction SilentlyContinue) {{ exit 1 }}"
            subprocess.run(["pwsh","-NoProfile","-Command",script],check=True)
        results["parent-exit"]={"child_pid":pid,"cleaned":True}
    finally:
        if parent.poll() is None: parent.kill();parent.wait()
    # CLI input remains on stdin and is never printed to stderr.
    for raw in (b"",b"\xff",b"a"*2954):
        result=subprocess.run([str(host),"qr","encode","--ecc","L"],input=raw,capture_output=True)
        assert result.returncode!=0 and not result.stdout
    text="  Unicode 🦀 exact case\n".encode()
    result=subprocess.run([str(host),"qr","encode"],input=text,capture_output=True,check=True)
    rows=result.stdout.decode().splitlines();width=int(rows[0])
    assert len(rows)==width+1 and all(len(row)==width and set(row)<={"0","1"} for row in rows[1:])
    assert not result.stderr
    results["cli"]={"width":width,"malformed_rejected":3}
(evidence / "host-tests.json").write_text(json.dumps(results,indent=2)+"\n")
print(json.dumps(results))
