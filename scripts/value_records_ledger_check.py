#!/usr/bin/env python3
"""Value records (DESIGN-p4l4 §10.4): each parity fixture's expected
admissions, read from its `// vr-expect: <birth keys> <verdict prefix>`
lines, against the facts dump's `records` ledger with
PERRY_FACTS_VALUE_RECORDS=1.

usage: value_records_ledger_check.py --perry PERRY --out DIR FIXTURE...
Prints one line per expectation and exits 1 if any does not hold.
"""
import argparse
import os
import re
import subprocess
import sys
from pathlib import Path


def ledger(dump):
    """(keys, verdict) per `# record` line: keys from `fields=` or `keys=`."""
    out = []
    for line in dump.splitlines():
        if not line.startswith("# record "):
            continue
        m = re.search(r" ADMITTED fields=(\S+)", line)
        if m:
            keys = ",".join(f.rsplit(":", 1)[0] for f in m.group(1).split(","))
            out.append((keys, "ADMITTED"))
            continue
        m = re.search(r" REJECTED (.*?)(?: at \S+)? keys=(\S*)$", line)
        if m:
            out.append((m.group(2), "REJECTED " + m.group(1)))
    return out


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--perry", required=True)
    p.add_argument("--out", required=True, type=Path)
    p.add_argument("fixtures", nargs="+", type=Path)
    a = p.parse_args()
    a.out.mkdir(parents=True, exist_ok=True)
    bad = 0
    for f in a.fixtures:
        expect = re.findall(r"^// vr-expect: (\S+) (.+)$", f.read_text(), re.M)
        if not expect:
            continue
        dump = a.out / (f.stem + ".dump")
        env = dict(os.environ, PERRY_FACTS_VALUE_RECORDS="1", PERRY_FACTS_ONLY="1",
                   PERRY_FACTS_DUMP=str(dump), PERRY_NO_CACHE="1")
        subprocess.run([a.perry, "compile", str(f), "-o", str(a.out / f.stem)], env=env,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, cwd=f.parent)
        rows = ledger(dump.read_text()) if dump.exists() else []
        for keys, verdict in expect:
            got = [v for k, v in rows if k == keys]
            ok = len(got) == 1 and got[0].startswith(verdict.strip())
            bad += not ok
            print(f"{'ok  ' if ok else 'FAIL'} {f.name} {keys}: expected {verdict.strip()!r}, got {got}")
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
