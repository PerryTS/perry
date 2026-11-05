#!/usr/bin/env python3
"""Compile/run the realm oracle or compare full/reachable descriptor graphs.

PERRY_BIN and PERRY_RUNTIME_DIR select a matched compiler/runtime arm.
Dump output is canonical JSONL. A reach file contains JSON {"names": [...]}.
Comparison follows all descriptor and prototype edges from those global keys,
re-canonicalizing cycle paths in each arm before comparison.
"""
import argparse
import json
import os
import re
from pathlib import Path
import subprocess
import tempfile


def graph(path):
    rows = [json.loads(line) for line in Path(path).read_text().splitlines()]
    if not rows or rows[0]["path"] != "$global":
        raise ValueError("missing global descriptor witness")
    return {row["path"]: row for row in rows}


def canonical(rows, names=None):
    labels = {"$global": "$global"}
    pending = ["$global"]

    def visit(value, path):
        if isinstance(value, dict):
            if set(value) == {"ref"}:
                target = value["ref"]
                if target not in rows:
                    raise ValueError(f"dangling reference: {target}")
                if target not in labels:
                    labels[target] = path
                    pending.append(target)
                return {"ref": labels[target]}
            return {k: visit(v, path + "." + k) for k, v in sorted(value.items())}
        if isinstance(value, list):
            return [visit(v, path + f"[{i}]") for i, v in enumerate(value)]
        return value

    out = []
    for old in pending:
        row = dict(rows[old])
        row.pop("path")
        if old == "$global" and names is not None:
            globals_ = {n for n in names if re.fullmatch(r"[A-Za-z_$][A-Za-z0-9_$]*", n)}
            row["properties"] = [p for p in row["properties"] if p.get("key") in names]
            found = {p.get("key") for p in row["properties"]}
            missing = globals_ - found
            if missing:
                raise ValueError(f"reach names absent from global: {sorted(missing)}")
            row["ownKeyOrder"] = [n for n in row["ownKeyOrder"] if n in names]
            row["intrinsics"] = [i for i in row.get("intrinsics", []) if i["name"] in names]
        row = visit(row, labels[old])
        row["path"] = labels[old]
        out.append(row)
    return "".join(json.dumps(row, sort_keys=True, separators=(",", ":")) + "\n" for row in out)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="mode", required=True)
    dump = sub.add_parser("dump")
    dump.add_argument("output", type=Path)
    dump.add_argument("--work-dir", type=Path)
    compare = sub.add_parser("compare")
    compare.add_argument("base", type=Path)
    compare.add_argument("candidate", type=Path)
    compare.add_argument("--reach", type=Path, help="omit for a full/top comparison")
    args = parser.parse_args()
    if args.mode == "dump":
        binary = os.environ["PERRY_BIN"]
        source = Path(__file__).with_suffix(".ts").resolve()
        with tempfile.TemporaryDirectory(prefix="perry-realm-dump-") as temporary:
            work = args.work_dir or Path(temporary)
            work.mkdir(parents=True, exist_ok=True)
            app = work / "realm-dump"
            subprocess.run([binary, "compile", str(source), "-o", str(app)], check=True)
            # Compile-arm selectors, SSH connection ids and the launching
            # shell's pid are not realm facts. Run both arms in one fixed env.
            run_env = {"PATH": "/usr/bin:/bin", "HOME": "/root", "LANG": "C"}
            result = subprocess.run([str(app)], check=True, text=True, capture_output=True, env=run_env)
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(canonical(graph_from_text(result.stdout)))
    else:
        names = None if args.reach is None else set(json.loads(args.reach.read_text())["names"])
        base = canonical(graph(args.base), names)
        candidate = canonical(graph(args.candidate), names)
        if base != candidate:
            import difflib
            print("".join(difflib.unified_diff(base.splitlines(True), candidate.splitlines(True),
                                              fromfile=str(args.base), tofile=str(args.candidate))))
            raise SystemExit(1)
        print(f"realm descriptors equal ({len(base.splitlines())} cells)")


def graph_from_text(text):
    rows = [json.loads(line[7:]) for line in text.splitlines() if line.startswith("@realm ")]
    if not rows or rows[0]["path"] != "$global":
        raise ValueError("missing global descriptor witness")
    return {row["path"]: row for row in rows}


if __name__ == "__main__":
    main()
