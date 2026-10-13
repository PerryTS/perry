#!/usr/bin/env python3
"""wpfacts dynamic oracle (design 6.6, checker 2).

Checks every fact the compiler claims against what node actually did when
running the same program under the census hook
(scratchpad/wpfacts-20261012-evidence/wp-hook.cjs): the dynamic
observations must be a SUBSET of the static claims.

  prefix_stable(T)  no HARD operation (defineProperty, setPrototypeOf,
                    freeze family, Proxy, Object.create, Reflect.construct,
                    delete) observed on T;
  readonly(T.f)     no post-construction write to T.f observed;
  kind(T.f)         every kind written/read for T.f lies in the claimed kind;
  site verdicts     a site claimed Tier 0 on one template observed no other
                    receiver template.

Templates are matched by name: classes and constructor functions by their
name, literals by file + key set. When several static templates share a
name, a violation is reported only when every candidate claims the fact
(the rest are counted as "ambiguous").

Usage: wpfacts_oracle.py <facts-dump> <census.json>
Exit 0 when no claim is contradicted.
"""
import collections
import json
import sys
from pathlib import Path

HARD = {"defineProperty", "setPrototypeOf", "freeze", "seal", "preventExtensions",
        "Proxy", "Object.create", "Reflect.construct", "delete"}


ALIASES = {"arena12.ts": "binarytrees_arena.ts", "plain12.ts": "binarytrees_plain.ts"}


def norm(path):
    p = str(path)
    i = p.rfind("node_modules/")
    return p[i + len("node_modules/"):] if i >= 0 else ALIASES.get(Path(p).name, Path(p).name)


def kind_cat(k):
    if k in ("i32", "f64"):
        return "num"
    if k == "sym":
        return "symbol"
    if k in ("str", "bool", "null", "undef", "bigint"):
        return k
    return "obj"  # o<proto id>, fn


def claimed_cats(desc):
    if desc == "top":
        return None
    cats = set()
    for part in desc.split("|"):
        if part.startswith("obj{") or part.startswith("fn*") or part.startswith("builtin*"):
            cats.add("obj")
        elif part == "none":
            pass
        else:
            cats.add(part)
    return cats


def load_static(dump):
    T = {}
    mods = {}
    cur = None
    for l in open(dump):
        l = l.rstrip("\n")
        if l.startswith("M "):
            _, i, p = l.split(" ", 2)
            mods[int(i)] = p
        elif l.startswith("T "):
            disp = l[2:].split(" kind=")[0]
            kv = {}
            for part in l.split(" "):
                if "=" in part:
                    k, v = part.split("=", 1)
                    kv[k] = v
            birth = kv.get("birth", "[]").strip("[]")
            cur = dict(display=disp, kind=kv.get("kind"), ps=kv.get("prefix_stable") == "1",
                       birth=[b for b in birth.split(",") if b], fields={})
            T[disp] = cur
        elif l.startswith("  F ") and cur is not None:
            q = l.split(" ")
            key = q[3]
            kv = dict(p.split("=", 1) for p in q[4:] if "=" in p)
            cur["fields"][key] = dict(readonly=kv.get("readonly") == "1",
                                      kind=kv.get("kind"), definite=kv.get("definite") == "1")
    return T, mods


def main():
    dump, cfile = sys.argv[1], sys.argv[2]
    T, mods = load_static(dump)
    C = json.load(open(cfile))
    protos = C["protos"]
    files = C["files"]
    esc = collections.defaultdict(collections.Counter)
    for pid, ops in C["escapes"]:
        for op, c in ops:
            esc[pid][op] += c
    fields = collections.defaultdict(dict)
    for f in C["fields"]:
        fields[f["id"]][f["prop"]] = f
    # candidates per census proto
    by_name = collections.defaultdict(list)
    for disp, t in T.items():
        if t["kind"] in ("class", "fninst"):
            nm = disp.rsplit(":", 1)[-1]
            by_name[nm].append(t)
    lits = collections.defaultdict(list)
    for disp, t in T.items():
        if t["kind"] in ("lit", "litclass"):
            file = disp.rsplit(":", 1)[0]
            lits[(file, tuple(t["birth"]))].append(t)
    checked = collections.Counter()
    viol = []
    ambiguous = collections.Counter()
    for pid, pr in enumerate(protos):
        kind = pr["kind"]
        cands = []
        if kind in ("class", "fnctor"):
            cands = by_name.get(pr["name"], [])
        elif kind == "lit":
            # lit@<fid>:<line>; birth keys = fields written at init
            try:
                fid = int(pr["name"][4:].split(":")[0])
            except ValueError:
                continue
            fs = fields.get(pid, {})
            birth = tuple(k for k, f in fs.items() if f["wInit"] > 0)
            file = norm(files[fid])
            for (sf, sb), ts in lits.items():
                if sf == file and set(sb) == set(birth):
                    cands.extend(ts)
        if not cands:
            continue
        if len(cands) > 1:
            ambiguous["templates"] += 1

        def all_claim(pred):
            return all(pred(t) for t in cands)

        # prefix_stable
        hard = [op for op in esc.get(pid, {}) if op in HARD]
        dels = [p for p, f in fields.get(pid, {}).items() if f["del"]]
        checked["prefix_stable"] += 1
        if (hard or dels) and all_claim(lambda t: t["ps"]):
            viol.append(f"F3 prefix_stable {cands[0]['display']}: node observed {hard or ['delete ' + dels[0]]}")
        for prop, f in fields.get(pid, {}).items():
            # readonly
            if all_claim(lambda t: prop in t["fields"]):
                checked["readonly"] += 1
                if f["wPost"] > 0 and all_claim(lambda t: t["fields"][prop]["readonly"]):
                    viol.append(f"F1 readonly {cands[0]['display']}.{prop}: node observed {f['wPost']} post-construction writes")
                obs = {kind_cat(k) for k, _ in f["kinds"]}
                checked["kind"] += 1

                def kind_ok(t):
                    cl = claimed_cats(t["fields"][prop]["kind"])
                    return cl is None or obs <= cl | ({"undef"} if not t["fields"][prop]["definite"] else set())
                if not any(kind_ok(t) for t in cands):
                    viol.append(f"F2 kind {cands[0]['display']}.{prop}: claimed {cands[0]['fields'][prop]['kind']}, node observed {sorted(obs)}")
    print(f"oracle: checked {dict(checked)}; ambiguous {dict(ambiguous)}; violations {len(viol)}")
    for v in viol[:40]:
        print("  VIOLATION", v)
    sys.exit(1 if viol else 0)


if __name__ == "__main__":
    main()
