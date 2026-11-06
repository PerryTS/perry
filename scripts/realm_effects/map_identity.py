#!/usr/bin/env python3
"""Native Map identity operations do not invoke operand member protocols."""
from __future__ import annotations
import argparse
import os
from pathlib import Path
import subprocess
from behave import compile_run


def program(operation: str, form: str) -> tuple[str, str, str]:
    prefix = """
function run() {
const trace: string[] = [];
let evaluations = 0;
const map: Map<unknown, unknown> = new Map();
map.unrelated = () => { trace.push("BAD-receiver"); return 0; };
"""
    key = 'const key: any = "key";\n'
    argument = "key"
    breaker = "map.unrelated();\n"
    expected_trace = ""
    expected_evaluations = 0
    result = "true"
    size = "1" if operation == "has" else "0"
    if operation == "clear":
        result = "undefined"
    if form == "object-key":
        key = """
const key: any = {
    toString() { trace.push("BAD-toString"); return "key"; },
    valueOf() { trace.push("BAD-valueOf"); return 1; },
    [Symbol.toPrimitive]() { trace.push("BAD-toPrimitive"); return "key"; },
    get unrelated() { trace.push("BAD-get"); return () => trace.push("BAD-call"); }
};
"""
        breaker = "key.unrelated();\n"
    elif form == "function-key":
        key = """
const key: any = Array.isArray;
key.unrelated = () => { trace.push("BAD-function-member"); return 0; };
"""
        breaker = "key.unrelated();\n"
    elif form == "key-expression":
        key += "function getKey() { evaluations++; return key; }\n"
        argument = "getKey()"
        expected_evaluations = 1
    elif form == "own-method":
        prefix += f'map.{operation} = (value: unknown) => {{ trace.push("own"); return true; }};\n'
        expected_trace = "own"
        size = "1"
        breaker = ""
    elif form == "method-getter":
        prefix += f'''Object.defineProperty(map, "{operation}", {{ get() {{
    trace.push("get-method"); return (value: unknown) => {{ trace.push("call-method"); return true; }};
}} }});\n'''
        expected_trace = "get-method|call-method"
        size = "1"
        breaker = ""
    body = prefix + key + "map.set(key, 1);\n"
    args = "" if operation == "clear" else argument
    call = f'map.{operation}({args})'
    body += f'const result = {call};\n'
    body += 'console.log("EV result " + result);\nconsole.log("EV size " + map.size);\n'
    body += 'console.log("EV hooks " + trace.join("|"));\nconsole.log("EV evaluations " + evaluations);\n}\nrun();\n'
    expected = f"EV result {result}\nEV size {size}\nEV hooks {expected_trace}\nEV evaluations {expected_evaluations}\n"
    broken = body.replace(f'const result = {call};', breaker + f'const result = {call};')
    return body, broken, expected


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--perry", required=True)
    ap.add_argument("--node", default="node")
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--break-native-protocol", action="store_true",
                    help="diagnostic: make the compiled wrapper invoke an unrelated member")
    args = ap.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, PERRY_NO_CACHE="1", NODE_NO_WARNINGS="1")
    red = count = 0
    for operation in ("has", "delete", "clear"):
        forms = ("receiver",) if operation == "clear" else (
            "receiver", "object-key", "function-key", "key-expression", "own-method", "method-getter")
        for form in forms:
            count += 1
            name = operation + "-" + form
            body, broken, expected = program(operation, form)
            node_src = args.out / (name + ".node.ts")
            native_src = args.out / (name + ".ts")
            node_src.write_text(body)
            native_src.write_text(broken if args.break_native_protocol else body)
            node = subprocess.run([args.node, "--experimental-strip-types", str(node_src)],
                                  env=env, capture_output=True, text=True, timeout=60)
            native, error = compile_run(args.perry, native_src, env)
            (args.out / (name + ".node.log")).write_text(node.stdout + node.stderr)
            (args.out / (name + ".perry.log")).write_text(native + error)
            good = not error and node.returncode == 0 and node.stdout == expected and native == node.stdout
            red += not good
            print(f"{'GREEN' if good else 'RED'} {name}", flush=True)
            if not good:
                print(f"expected={expected!r} node={node.stdout!r} native={native!r} error={error!r}", flush=True)
    print(f"programs={count} red={red}")
    return int(red != 0)


if __name__ == "__main__":
    raise SystemExit(main())
