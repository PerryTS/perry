#!/usr/bin/env python3
"""Check URI ToString hooks and Symbol rejection, and compare other inputs with an arm."""
from __future__ import annotations
import argparse
import json
import os
from pathlib import Path
import subprocess
from behave import compile_run

OPERATIONS = ("encodeURI", "decodeURI", "encodeURIComponent", "decodeURIComponent")


def program(op: str, form: str) -> tuple[str, str | None]:
    prefix = "const trace: string[] = [];\n"
    unrelated = """
get unrelated() { trace.push("BAD-get"); return 0; },
other() { trace.push("BAD-call"); return 0; }
"""
    forms = {
        "to-string": """
get toString() { trace.push("get-toString"); return function() { trace.push("call-toString"); return "A"; }; },
get valueOf() { trace.push("get-valueOf"); return function() { trace.push("call-valueOf"); return "B"; }; },
""",
        "value-of": """
toString: undefined,
get valueOf() { trace.push("get-valueOf"); return function() { trace.push("call-valueOf"); return "A"; }; },
""",
        "symbol-hook": """
[Symbol.toPrimitive](hint: string) { trace.push("symbol:" + hint); return "A"; },
get toString() { trace.push("BAD-toString"); return () => "B"; },
get valueOf() { trace.push("BAD-valueOf"); return () => "C"; },
""",
        "other-members": """
toString() { trace.push("toString"); return "A"; },
""",
    }
    expected = {
        "to-string": "EV result A\nEV hooks get-toString|call-toString\n",
        "value-of": "EV result A\nEV hooks get-valueOf|call-valueOf\n",
        "symbol-hook": "EV result A\nEV hooks symbol:string\n",
        "other-members": "EV result A\nEV hooks toString\n",
    }
    if form == "symbol-throws":
        return f"""let completion = "returned";
try {{ {op}(Symbol("x")); }}
catch(e) {{ completion = e instanceof TypeError ? "TypeError" : "other"; }}
console.log("EV symbol " + completion);
""", "EV symbol TypeError\n"
    if form == "values":
        values = (
            ("string", '"A B"'), ("number", "42"), ("negative-zero", "-0"),
            ("null", "null"), ("undefined", "undefined"), ("true", "true"),
            ("false", "false"), ("bigint", "123n"), ("array", "[1, 2]"),
            ("date", "new Date(0)"),
        )
        return "\n".join(
            f'console.log("EV {name} " + {op}({value}));'
            for name, value in values
        ) + "\n", None
    body = prefix + "const o: any = {\n" + forms[form] + unrelated + "};\n"
    body += f'console.log("EV result " + {op}(o));\n'
    body += 'console.log("EV hooks " + trace.join("|"));\n'
    return body, expected[form]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--perry", required=True)
    ap.add_argument("--node", default="node")
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--baseline", type=Path, help="results.json from the unchanged arm")
    args = ap.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, PERRY_NO_CACHE="1", NODE_NO_WARNINGS="1", TZ="UTC")
    baseline = json.loads(args.baseline.read_text()) if args.baseline else {}
    records = {}
    red = 0
    count = 0
    for op in OPERATIONS:
        for form in ("to-string", "value-of", "symbol-hook", "other-members", "symbol-throws", "values"):
            count += 1
            name = op + "-" + form
            body, expected = program(op, form)
            src = args.out / (name + ".ts")
            src.write_text(body)
            node = subprocess.run([args.node, "--experimental-strip-types", str(src)],
                                  env=env, capture_output=True, text=True, timeout=60)
            native, error = compile_run(args.perry, src, env)
            (args.out / (name + ".node.log")).write_text(node.stdout + node.stderr)
            (args.out / (name + ".perry.log")).write_text(native + error)
            good = not error and node.returncode == 0
            if expected is not None:
                good = good and node.stdout == expected and native == node.stdout
            if name in baseline and form != "symbol-throws":
                good = good and native == baseline[name]["native"] and not baseline[name]["error"]
            records[name] = {"native": native, "node": node.stdout, "error": error, "form": form}
            if expected is None and native != node.stdout:
                print(f"REFERENCE-DIFF {name}: node={node.stdout!r} native={native!r}", flush=True)
            red += not good
            print(f"{'GREEN' if good else 'RED'} {name}", flush=True)
            if not good:
                print(f"expected={expected!r} node={node.stdout!r} native={native!r} error={error!r}", flush=True)
    (args.out / "results.json").write_text(json.dumps(records, indent=2) + "\n")
    print(f"programs={count} red={red}")
    return int(red != 0)


if __name__ == "__main__":
    raise SystemExit(main())
