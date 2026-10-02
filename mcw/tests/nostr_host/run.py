"""Run the actual mcw process with its synthetic managed child; no shell/relay/wallet."""
import argparse
import json
import pathlib
import subprocess

parser = argparse.ArgumentParser()
parser.add_argument("--host", required=True)
parser.add_argument("--evidence", required=True)
parser.add_argument("--log", required=True)
args = parser.parse_args()
result = subprocess.run([args.host, "gui", "--evidence", args.evidence], stdout=subprocess.PIPE,
                        stderr=subprocess.STDOUT, timeout=180, creationflags=subprocess.CREATE_NO_WINDOW)
pathlib.Path(args.log).write_bytes(result.stdout)
if result.returncode:
    print(result.stdout.decode("utf-8", errors="replace"))
    raise SystemExit(result.returncode)
record = json.loads(pathlib.Path(args.evidence).read_text(encoding="utf-8"))
if record["status"] != "passed":
    raise AssertionError(record)
print(json.dumps(record))
