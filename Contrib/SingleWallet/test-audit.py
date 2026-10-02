#!/usr/bin/env python3
"""Negative fixtures must be rejected; protocol participants and ordinary transaction selection are legitimate."""
import importlib.util
import json
from pathlib import Path

here = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("single_wallet_audit", here / "audit.py")
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)
fixtures = json.loads((here / "negative-fixtures.json").read_text(encoding="utf-8"))
for token in fixtures:
    assert audit.check_text("synthetic.cs", token, set()), token
for text in ("RoundState[] rounds;", "Input[] inputs;", "Share[] backupShares;", "Transaction.IsSelected", "HwiClient.EnumerateAsync"):
    assert not audit.check_text("synthetic.cs", text, set()), text
print(f"Single-wallet audit rejected all {len(fixtures)} retired fixtures and accepted legitimate protocol, backup, transaction-selection and hardware data.")
