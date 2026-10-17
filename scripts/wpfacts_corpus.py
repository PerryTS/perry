#!/usr/bin/env python3
"""wpfacts adversarial corpus runner (design 6.3 / 6.4 / 6.5).

For every program in test-files/wpfacts:
  1. compile it (facts dump + relations), run it, run node on it: the
     outputs must be byte-identical;
  2. check its `// WPFACTS:` assertions against the facts dump (a fact the
     test tries to break must be OFF; the positive control's must be ON);
  3. run the independent recomputation (scripts/wpfacts_recompute.py): its
     diff must be empty;
  4. (--contracts) compile it again with PERRY_CHECK_CONTRACTS=1, run it:
     no contract abort and the same output as node.

--sabotage runs the negative controls instead: each plants one wrong fact
(PERRY_FACTS_SABOTAGE) in a corpus program, and EVERY one of the three
detectors (the corpus assertion, the recomputation, the contract build)
must catch it. A sabotage that any detector misses fails the run.

Usage:
  wpfacts_corpus.py --perry PERRY --out DIR [--node NODE] [--contracts]
                    [--sabotage] [--only SUBSTR]
"""
import argparse
import os
import re
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
CORPUS = HERE.parent / "test-files" / "wpfacts"
RECOMPUTE = HERE / "wpfacts_recompute.py"

# (program, sabotage spec, the assertion it must break)
SABOTAGE = [
    ("e07_delete.ts", "prefix_stable:Del", "contract"),
    ("e04_object_assign.ts", "readonly:Cfg.mode", "contract"),
    ("e01_any_alias.ts", "kind:Box.v=num", "contract"),
    ("e02_computed_key.ts", "fixed_layout:Rec", "contract"),
    ("f1_closure_writer.ts", "readonly:Cnt.c", "static"),
    ("e10_object_create.ts", "prefix_stable:Made", "contract"),
]


def sh(cmd, env=None, cwd=None, timeout=900):
    p = subprocess.run(cmd, env=env, cwd=cwd, capture_output=True, timeout=timeout)
    return p.returncode, p.stdout.decode("utf-8", "replace"), p.stderr.decode("utf-8", "replace")


def compile_one(args, src, exe, dump, extra_env):
    env = dict(os.environ)
    env.update(extra_env)
    env["PERRY_FACTS_DUMP"] = str(dump)
    env["PERRY_FACTS_RELATIONS"] = "1"
    cache = Path(args.out) / "cache"
    rc, out, err = sh([args.perry, "compile", "--cache-dir", str(cache), str(src), "-o", str(exe)],
                      env=env, cwd=src.parent, timeout=1800)
    return rc, out + err


def run_exe(exe, cwd):
    return sh([str(exe)], cwd=cwd, timeout=300)


def run_node(args, src):
    return sh([args.node, str(src)], cwd=src.parent, timeout=300)


def facts_lines(dump):
    return Path(dump).read_text().splitlines()


def entry_module_index(lines, src):
    for l in lines:
        if l.startswith("M "):
            _, idx, path = l.split(" ", 2)
            if Path(path).resolve() == src.resolve():
                return int(idx)
    return None


def check_assertions(src, dump):
    lines = facts_lines(dump)
    entry = entry_module_index(lines, src)
    failures = []
    for a in re.findall(r"//\s*WPFACTS:\s*(.*)", src.read_text()):
        a = a.strip()
        ok, why = check_one(a, lines, entry)
        if not ok:
            failures.append(f"{a}  [{why}]")
    return failures


def name_matches(display, name):
    return display == name or display.endswith(":" + name)


def check_one(a, lines, entry):
    p = a.split()
    if p[0] == "fact":
        # fact F3 prefix_stable Name = 0   /   fact F1 readonly Name.key = 1
        code, fact, target, _, want = p[1], p[2], p[3], p[4], p[5]
        tname, key = (target.rsplit(".", 1) + [None])[:2] if "." in target else (target, None)
        found = []
        for l in lines:
            if not l.startswith(f"FACT {code} {fact} "):
                continue
            body, val = l[len(f"FACT {code} {fact} "):].rsplit(" = ", 1)
            if key is not None:
                if "." not in body:
                    continue
                disp, k = body.rsplit(".", 1)
                if k != key:
                    continue
            else:
                disp = body
            if name_matches(disp, tname):
                found.append(val)
        if not found:
            return (want == "0", "fact absent from the dump")
        return (all(v == want for v in found), f"dump says {found}")
    if p[0] == "kind":
        # kind Name.key has str | kind Name.key is num
        tname, key = p[1].rsplit(".", 1)
        vals = []
        for l in lines:
            if l.startswith("FACT F2 kind "):
                body, val = l[len("FACT F2 kind "):].rsplit(" = ", 1)
                disp, k = body.rsplit(".", 1)
                if k == key and name_matches(disp, tname):
                    vals.append(val)
        if not vals:
            return (False, "kind absent")
        if p[2] == "has":
            return (all(v == "top" or p[3] in v.split("|") or p[3] in v for v in vals), f"kinds {vals}")
        return (all(v == p[3] for v in vals), f"kinds {vals}")
    if p[0] == "recv":
        # `recv .key A B`: every read site of .key lists templates A and B
        # in its receiver set (an alias the analysis must not miss).
        key = p[1].lstrip(".")
        sets = []
        for l in lines:
            q = l.split(" ")
            if l.startswith("S ") and q[3] == "." + key and q[4] == "call=0":
                r = l.split(" recv={", 1)[1].split("}", 1)[0]
                sets.append({x.rsplit(":", 1)[-1] for x in r.split(",")})
        if not sets:
            return (False, "no such site")
        return (all(set(p[2:]) <= st for st in sets), f"recv {sets}")
    if p[0] in ("site", "call"):
        key = p[1].lstrip(".")
        want_on = p[2] == "on"
        call = "1" if p[0] == "call" else "0"
        vs = []
        for l in lines:
            if not l.startswith("S "):
                continue
            q = l.split(" ")
            if entry is not None and int(q[1]) != entry:
                continue
            if q[3] != "." + key or q[4] != f"call={call}":
                continue
            vs.append(l.rsplit("verdict=", 1)[1])
        if not vs:
            return (False, "no such site")
        on = [v in ("T0", "T0set") for v in vs]
        return ((all(on) if want_on else not any(on)), f"verdicts {vs}")
    if p[0] == "guard":
        g = next((l for l in lines if l.startswith("# interp_guard ")), "# interp_guard none")
        is_on = g != "# interp_guard none"
        return (is_on == (p[1] == "on"), g)
    return (False, "unknown assertion")


def recompute(dump):
    rc, out, err = sh([sys.executable, str(RECOMPUTE), str(dump)], timeout=3600)
    return rc == 0, (out + err).strip().splitlines()[0:6]


def programs(args):
    for src in sorted(CORPUS.iterdir()):
        if src.suffix not in (".ts", ".js", ".cjs") or "helper" in src.name:
            continue
        if args.only and args.only not in src.name:
            continue
        yield src


def run_corpus(args):
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    fails = 0
    rows = []
    for src in programs(args):
        name = src.stem
        exe = out / name
        dump = out / f"{name}.facts"
        rc, log = compile_one(args, src, exe, dump, {})
        status = []
        if re.search(r"//\s*WPFACTS:\s*rejected", src.read_text()):
            # The escape source cannot reach a compiled program: the
            # compiler must keep rejecting it.
            ok = rc != 0
            rows.append((name, "rejected=ok" if ok else "rejected=COMPILED", []))
            fails += 0 if ok else 1
            continue
        if rc != 0:
            rows.append((name, "COMPILE-FAIL", log.strip().splitlines()[-3:]))
            fails += 1
            continue
        prc, pout, perr = run_exe(exe, src.parent)
        nrc, nout, nerr = run_node(args, src)
        same = pout == nout and (prc == 0) == (nrc == 0)
        status.append("out=ok" if same else "out=DIFF")
        afails = check_assertions(src, dump)
        status.append("facts=ok" if not afails else f"facts=FAIL({len(afails)})")
        rok, rlines = recompute(dump)
        status.append("recompute=ok" if rok else "recompute=DIFF")
        cok = True
        if args.contracts:
            cexe = out / f"{name}.contract"
            crc, clog = compile_one(args, src, cexe, out / f"{name}.contract.facts",
                                    {"PERRY_CHECK_CONTRACTS": "1", "PERRY_CONTRACTS_REPORT": "1"})
            if crc != 0:
                cok = False
                status.append("contract=COMPILE-FAIL")
            else:
                c_rc, c_out, c_err = run_exe(cexe, src.parent)
                violated = "wpfacts contract violated" in c_err
                cok = not violated and c_out == nout
                checks = re.search(r"(\d+) checks against (\d+) class claims", c_err)
                status.append("contract=ok" if cok else "contract=FAIL")
                if checks:
                    status.append(f"checks={checks.group(1)}/claims={checks.group(2)}")
        ok = same and not afails and rok and cok
        fails += 0 if ok else 1
        detail = []
        if not same:
            detail += [f"perry: {pout[:200]!r} rc={prc} {perr[:200]!r}", f"node:  {nout[:200]!r} rc={nrc}"]
        detail += afails
        if not rok:
            detail += rlines
        rows.append((name, " ".join(status), detail))
    for name, st, detail in rows:
        print(f"{name:28s} {st}")
        for d in detail:
            print(f"    {d}")
    print(f"CORPUS {'PASS' if fails == 0 else 'FAIL'}: {len(rows) - fails}/{len(rows)} programs")
    return fails == 0


def run_sabotage(args):
    out = Path(args.out) / "sabotage"
    out.mkdir(parents=True, exist_ok=True)
    allok = True
    for prog, spec, mode in SABOTAGE:
        src = CORPUS / prog
        name = src.stem
        env = {"PERRY_FACTS_SABOTAGE": spec}
        dump = out / f"{name}.facts"
        rc, log = compile_one(args, src, out / name, dump, env)
        if rc != 0:
            print(f"{prog} {spec}: COMPILE-FAIL")
            allok = False
            continue
        afails = check_assertions(src, dump)
        corpus_caught = bool(afails)
        rok, rlines = recompute(dump)
        recompute_caught = not rok
        contract = "n/a"
        caught_c = True
        if mode == "contract":
            cdump = out / f"{name}.contract.facts"
            crc, clog = compile_one(args, src, out / f"{name}.contract", cdump,
                                    dict(env, PERRY_CHECK_CONTRACTS="1"))
            if crc != 0:
                contract = "COMPILE-FAIL"
                caught_c = False
            else:
                c_rc, c_out, c_err = run_exe(out / f"{name}.contract", src.parent)
                m = re.search(r"wpfacts contract violated: (.*)", c_err)
                caught_c = m is not None
                contract = ("caught: " + m.group(1)[:140]) if m else f"MISSED rc={c_rc}"
        ok = corpus_caught and recompute_caught and caught_c
        allok &= ok
        print(f"{'OK  ' if ok else 'MISS'} {prog} sabotage={spec}")
        print(f"     corpus assertion: {'caught' if corpus_caught else 'MISSED'} {afails[:1]}")
        print(f"     recompute:        {'caught' if recompute_caught else 'MISSED'} {rlines[1:2]}")
        print(f"     contract build:   {contract}")
    print(f"SABOTAGE {'PASS' if allok else 'FAIL'}")
    return allok


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--perry", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--node", default="node")
    ap.add_argument("--contracts", action="store_true")
    ap.add_argument("--sabotage", action="store_true")
    ap.add_argument("--only")
    args = ap.parse_args()
    args.perry = str(Path(args.perry).resolve())
    args.out = str(Path(args.out).resolve())
    ok = run_sabotage(args) if args.sabotage else run_corpus(args)
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
