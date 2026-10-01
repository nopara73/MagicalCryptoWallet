#!/usr/bin/env python3
"""Exercise the audit gate with controlled retired-identity and trust-pin leaks."""
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
AUDIT = ROOT/'Contrib/Rebrand/audit.py'
README = ROOT/'README.md'
policy = json.loads((AUDIT.parent/'policy.json').read_text())
tests = {
    'product identity': 'Wasabi Wallet',
    'legacy default port': 'http://localhost:37128/',
    'retired update signing key': policy['forbidden_pattern'].split('|')[-2],
}
original = README.read_bytes()
try:
    subprocess.run([sys.executable, str(AUDIT)], cwd=ROOT, check=True)
    for label, token in tests.items():
        README.write_bytes(original+b'\n'+token.encode()+b'\n')
        result = subprocess.run([sys.executable, str(AUDIT)], cwd=ROOT, capture_output=True, text=True)
        if result.returncode != 1 or 'Content: README.md:' not in result.stderr:
            raise RuntimeError('Audit did not reject '+label+': '+result.stdout+result.stderr)
        print('Rejected '+label)
finally:
    README.write_bytes(original)
subprocess.run([sys.executable, str(AUDIT)], cwd=ROOT, check=True)
