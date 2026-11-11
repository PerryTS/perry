#!/usr/bin/env python3
"""An actual SIGABRT must preserve completed env/argv observations mid-init."""
import csv,sys
from pathlib import Path
p=Path(sys.argv[1])
assert p.is_file(), "ordered record missing after SIGABRT"
assert p.read_bytes().endswith(b"\n"), "truncated event row"
rows=list(csv.DictReader(p.open(),delimiter="\t"))
assert rows and all(int(r["ir"])>0 for r in rows)
assert all(int(a["seq"])<int(b["seq"]) for a,b in zip(rows,rows[1:]))
values={int(r["seq"]):bytes.fromhex(r["value_hex"]).decode() for r in rows}
expected="SNAP22_ABORT_SENTINEL".encode().hex()+"="+"before_abort".encode().hex()
assert any(r["op"]=="getenv" and values[int(r["seq"])]==expected for r in rows), "actual env observation missing"
argv=[r for r in rows if r["op"]=="argv"]
assert argv, "argv never observed"
assert any(r["op"]=="result" and values[int(r["seq"])].startswith(a["seq"]+":[string:") for a in argv for r in rows), "complete argv value missing"
assert any(r["op"]=="init_enter" for r in rows), "fixture module never entered"
assert not any(r["op"]=="boundary" for r in rows), "test reached a normal census flush"
print("GREEN: complete ordered env/argv prefix survives mid-init SIGABRT; no boundary reached")
