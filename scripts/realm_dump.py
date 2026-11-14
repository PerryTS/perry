#!/usr/bin/env python3
"""Compile/run the realm oracle or compare full/reachable descriptor graphs.

PERRY_BIN and PERRY_RUNTIME_DIR select a matched compiler/runtime arm.
Dump output is canonical JSONL. A reach file contains JSON {"names": [...]}.
Comparison follows descriptor and prototype edges and re-canonicalizes cycles.
A compiler projection restricts every image cell to its kept slots;
reach identities held through a chain need not be Global properties.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile


def graph(path):
    rows = [json.loads(line) for line in Path(path).read_text().splitlines()]
    if not rows or rows[0]["path"] != "$global":
        raise ValueError("missing global descriptor witness")
    return {row["path"]: row for row in rows}


def canonical(rows, names=None, projection=None, whole=()):
    labels = {"$global": "$global"}
    pending = ["$global"]
    identities = {"$global": "%Global%"}

    def bind(value, identity):
        if identity and isinstance(value, dict) and set(value) == {"ref"}:
            identities.setdefault(value["ref"], identity)

    def selected(row, identity):
        fact = (projection or {}).get(identity)
        if fact is None:
            return row
        def declared_key(prop):
            key = prop["key"]
            if not prop.get("symbol"):
                return False, key
            if key.startswith("@@"):
                return True, "Symbol(Symbol." + key[2:] + ")"
            if key.startswith(("@for:", "@runtime:")):
                return True, "Symbol(" + key.split(":", 1)[1] + ")"
            raise ValueError(f"unknown declared symbol identity: {key}")

        properties = fact.get("properties", [])
        if fact.get("native_namespace"):
            # Native namespace reflection follows its source-declared public
            # order; physical dispatch storage is not an own descriptor.
            public = {(False, k) for k in fact.get("own_key_order", [])}
            public.update((True, k) for k in fact.get("own_symbol_order", []))
            properties = [p for p in properties if (p.get("symbol", False), p["key"]) in public]
        declarations = {declared_key(p): p for p in properties}
        # Selection is supplied by the compiler's actual RealmPlan. Never
        # infer membership from a name's presence anywhere in R.
        def key(prop):
            return (True, prop["symbol"]) if "symbol" in prop else (False, prop["key"])
        present = {key(p) for p in row["properties"]}
        missing = set(declarations) - present
        if missing:
            raise ValueError(f"missing kept slots on {identity}: {sorted(missing)}")
        row["properties"] = [p for p in row["properties"] if key(p) in declarations]
        row["ownKeyOrder"] = [k for k in row["ownKeyOrder"] if (False, k) in declarations]
        if "ownSymbolIdentityOrder" in row:
            row["ownSymbolIdentityOrder"] = [identity for key, identity in
                zip(row.get("ownSymbolOrder", []), row["ownSymbolIdentityOrder"])
                if (True, key) in declarations]
        row["ownSymbolOrder"] = [k for k in row.get("ownSymbolOrder", []) if (True, k) in declarations]
        for field in ("name", "length"):
            if (False, field) not in declarations:
                row.pop(field, None)
        for prop in row["properties"]:
            lookup = (True, prop["symbol"]) if "symbol" in prop else (False, prop["key"])
            declaration = declarations.get(lookup)
            if not declaration:
                continue
            descriptor = prop["descriptor"]
            for part in ("get", "set"):
                bind(descriptor.get(part), declaration.get(part))
            value = declaration.get("value") or {}
            if value.get("kind") in ("reference", "raw_reference"):
                bind(descriptor.get("value"), value["value"])
        prototype = row.get("prototype")
        if fact.get("prototype") and not (isinstance(prototype, dict) and set(prototype) == {"ref"}):
            raise ValueError(f"missing kept prototype on {identity}")
        bind(prototype, fact.get("prototype"))
        for intrinsic in row.get("intrinsics", []):
            bind(intrinsic.get("value"), intrinsic["name"])
        return row

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
            row["intrinsics"] = [i for i in row.get("intrinsics", []) if i["name"] in names]
        row = selected(row, identities.get(old))
        if old == "$global" and names is not None and projection is None:
            row["properties"] = [p for p in row["properties"] if p.get("key") in names]
            row["ownKeyOrder"] = [n for n in row["ownKeyOrder"] if n in names]
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
        facts = {} if args.reach is None else json.loads(args.reach.read_text())
        names = None if args.reach is None else set(facts["names"])
        base = canonical(graph(args.base), names, facts.get("projection"), facts.get("whole", ()))
        candidate = canonical(graph(args.candidate), names, facts.get("projection"), facts.get("whole", ()))
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
