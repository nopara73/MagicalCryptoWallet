#!/usr/bin/env python3
"""Exercise the actual mcw host/managed pipe with synthetic children and payloads."""
import argparse, contextlib, json, os, shutil, signal, subprocess, time
from pathlib import Path
from evidence import assemblies, finish, new_run, restore_lock_inputs, sha, snapshot

ROOT = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--binary", type=Path, required=True)
args = parser.parse_args()
binary = args.binary.resolve()
evidence = new_run(ROOT / '.artifacts/mcw-evidence', 'host')
record = snapshot(ROOT, evidence)
record.update(native_sha256=sha(binary), production_release=False)
source = evidence / 'build'
record['restore_inputs'] = restore_lock_inputs(ROOT, evidence, record['source_hashes'])
with (evidence / 'build.log').open('wb') as log:
    built = subprocess.run(["dotnet","build",str(ROOT / "Contrib/Mcw/BridgeProbe"),"-c","Release","-m:1",
                            '/p:UseSharedCompilation=false','/p:BuildMcwHost=false','/p:RestoreLockedMode=true',
                            '--artifacts-path',str(evidence / 'artifacts'),'-o',str(source),
                            *record['restore_inputs']['arguments']],
                           stdout=log,stderr=subprocess.STDOUT,timeout=300)
if built.returncode:
    print((evidence / 'build.log').read_text(encoding='utf-8', errors='replace'))
    built.check_returncode()
with contextlib.nullcontext(evidence / 'mcw spaces 你好') as work:
    shutil.copytree(source,work)
    suffix=".exe" if os.name=="nt" else ""
    shutil.copy2(binary,work / ("mcw"+suffix))
    for child in ("magicalcryptowallet",):
        shutil.copy2(work / ("BridgeProbe"+suffix),work / (child+suffix))
    host=work / ("mcw"+suffix)
    record['assemblies_before'] = assemblies(work)
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
    run("queue-eof",1)
    run("queue-overload",1)
    run("sessions-cancel")
    run("sessions-eof",1)
    run("sessions-overload",1)
    for action in ('sessions-cancel', 'sessions-eof', 'sessions-overload'):
        assert json.loads(results[action]['report'])['cleanup']
    run("early-exit",1)
    run("unexpected-exit",1)
    run("no-handshake",1,timeout=150)
    # Parent death closes private pipes. The real managed adapter requests orderly
    # shutdown; on Windows the private job also prevents orphaned descendants.
    report=work / "parent-exit.txt"
    parent=subprocess.Popen([str(host),"gui","sessions-wait",str(report)],stdout=subprocess.PIPE,stderr=subprocess.PIPE)
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
        assert Path(str(report) + '.wallet').read_text() == 'original synthetic bytes'
        assert Path(str(report) + '.wallet.new').exists() and not Path(str(report) + '.wallet.old').exists()
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
record.update(results=results, passed=True, assemblies_after=assemblies(work))
assert record['assemblies_before'] == record['assemblies_after']
assert sha(binary) == record['native_sha256'] == sha(host)
finish(ROOT, evidence, record)
print(json.dumps(results))
