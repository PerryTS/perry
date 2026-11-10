#!/usr/bin/env python3
"""Check the solver's missing-member boundary against compiled runtime values.

Export the actual solver member tables with:
  cargo test -p perry-wpfacts --lib runtime_member_description -- --nocapture > description.tsv
Then run:
  missing_members.py --perry BIN --description description.tsv --out DIR
Every described namespace/constructor and two identified native functions
must have no own string key outside that description. Missing calls must
throw TypeError without touching their argument, and program stores must run.
--weaken HOLDER=KEY removes a claimed key to demonstrate a failing check.
Compilation/execution reuse behave.py's harness and its timeouts.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import sys

from behave import compile_run


def descriptions(path: Path) -> dict[str, set[str]]:
    result = {}
    for line in path.read_text().splitlines():
        if line.startswith("DESCRIPTION\t"):
            _, holder, *keys = line.split("\t")
            result[holder] = set(keys)
    if not result:
        raise ValueError("no DESCRIPTION rows; export runtime_member_description with --nocapture")
    return result


def generate(holder: str) -> str:
    return "const X: any = " + holder + ";\n" + r'''
console.log("EV own " + JSON.stringify(Object.getOwnPropertyNames(X)));
const o: any = { field: 1 };
let ran = 0;
let completion = "returned";
try { X.zzNoSuchMember(o); }
catch (e) { completion = e instanceof TypeError ? "TypeError" : "other"; }
console.log("EV missing " + completion + " " + o.field + " " + ran);
X.zap = function(p: any) { p.field = this === X ? 2 : 3; ran += 1; };
X.zap(o);
console.log("EV stored " + o.field + " " + ran);
console.log("EV end");
'''


def check(allowed: set[str], text: str) -> list[str]:
    reds = []
    own = [line[len("EV own "):] for line in text.splitlines() if line.startswith("EV own ")]
    if len(own) != 1:
        reds.append("expected one own-key census")
    else:
        keys = json.loads(own[0])
        unexpected = sorted(set(keys) - allowed)
        if unexpected:
            reds.append("runtime keys absent from description: " + ", ".join(unexpected))
    for event in ("EV missing TypeError 1 0", "EV stored 2 1", "EV end"):
        if text.splitlines().count(event) != 1:
            reds.append("expected exactly once: " + event)
    return reds


def additional_programs():
    pre = "const o: any = { field: 1 }; let ran = 0; function g(p: any) { p.field = 2; ran += 1; }\n"
    for holder in ("Math", "Object"):
        yield "direct-" + holder, pre + f'''let completion = "returned";
try {{ {holder}.zzNoSuchMember(o); }} catch(e) {{ completion = e instanceof TypeError ? "TypeError" : "other"; }}
console.log("EV direct " + completion + " " + o.field + " " + ran);
''', ["EV direct TypeError 1 0"]
    for holder in ("Array.prototype.map", "Object.prototype.hasOwnProperty"):
        for form, change in (("assign", "f.__proto__ = { zap: g };"), ("set", "Object.setPrototypeOf(f, { zap: g });")):
            yield "prototype-" + holder + "-" + form, pre + f"const f: any = {holder}; {change} f.zap(o);" + 'console.log("EV prototype " + o.field + " " + ran);', ["EV prototype 2 1"]
    operations = ("P.zap = g;", 'Object.defineProperty(P, "zap", { value: g });', 'Reflect.set(P, "zap", g);', "Object.assign(P, { zap: g });")
    for parent in ("Function.prototype", "Object.prototype"):
        for i, operation in enumerate(operations):
            body = pre + f"const P: any = {parent}; try {{ {operation} }} catch(e) {{}}\n"
            body += 'console.log("EV lock " + typeof P.zap);\n'
            for holder in ("Array.prototype.map", "Object.prototype.hasOwnProperty"):
                body += f'''{{ const f: any = {holder};
console.log("EV read {holder} " + typeof f.zap);
let completion = "returned";
try {{ f.zap(o); }} catch(e) {{ completion = e instanceof TypeError ? "TypeError" : "other"; }}
console.log("EV locked-call {holder} " + completion + " " + o.field + " " + ran); }}
'''
            expected = ["EV lock undefined"]
            for holder in ("Array.prototype.map", "Object.prototype.hasOwnProperty"):
                expected += [f"EV read {holder} undefined", f"EV locked-call {holder} TypeError 1 0"]
            yield f"lock-{parent}-{i}", body, expected


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--perry", required=True)
    ap.add_argument("--description", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--only", action="append", default=[])
    ap.add_argument("--weaken", action="append", default=[], help="HOLDER=KEY")
    a = ap.parse_args(argv)
    claims = descriptions(a.description)
    for item in a.weaken:
        holder, key = item.split("=", 1)
        claims[holder].remove(key)
    if a.only:
        claims = {holder: keys for holder, keys in claims.items() if holder in a.only}
    if not claims:
        ap.error("no matching description")
    a.out.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, PERRY_NO_CACHE="1")
    red = 0
    for holder, keys in claims.items():
        src = a.out / (holder + ".ts")
        src.write_text(generate(holder))
        text, err = compile_run(a.perry, src, env)
        src.with_suffix(".log").write_text(text)
        reds = check(keys, text)
        if err:
            reds.insert(0, err)
        if reds:
            red += 1
            for error in reds:
                print(f"RED {holder}: {error}", flush=True)
        else:
            print(f"GREEN {holder}", flush=True)
    additional = 0
    if not a.only:
        for name, program, expected in additional_programs():
            additional += 1
            src = a.out / (name + ".ts")
            src.write_text(program)
            text, err = compile_run(a.perry, src, env)
            src.with_suffix(".log").write_text(text)
            errors = [event for event in expected if text.splitlines().count(event) != 1]
            if err or errors:
                red += 1
                print(f"RED {name}: {err} missing events {errors}", flush=True)
            else:
                print(f"GREEN {name}", flush=True)
    print(f"holders={len(claims)} additional={additional} red_programs={red}")
    return int(red != 0)


if __name__ == "__main__":
    sys.exit(main())
