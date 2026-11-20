#!/usr/bin/env python3
"""Accessor lookup returns the installed function identity without invoking it."""
from __future__ import annotations
import argparse
import os
from pathlib import Path
import subprocess
from behave import compile_run


def program(kind: str, form: str) -> tuple[str, str, str]:
    method = '__lookupGetter__' if kind == 'get' else '__lookupSetter__'
    define = '__defineGetter__' if kind == 'get' else '__defineSetter__'
    parameter = '' if kind == 'get' else 'value'
    body = 'trace.push("BAD-accessor"); return 7;'
    prefix = 'function run() {\nconst trace: string[] = [];\n'
    expected_identity = 'true'
    expected_hooks = ''
    key = '"x"'
    if form in ('literal', 'class', 'inherited'):
        member = f'{kind} x({parameter}) {{ {body} }}'
        if form == 'literal':
            setup = f'const receiver = {{ {member} }};\n'
            holder = 'receiver'
        else:
            setup = f'class Holder {{ {member} }}\n'
            if form == 'inherited':
                setup += 'class Child extends Holder {}\nconst receiver = new Child();\n'
            else:
                setup += 'const receiver = new Holder();\n'
            holder = 'Holder.prototype'
        setup += f'const installed = Object.getOwnPropertyDescriptor({holder}, "x").{kind};\n'
    else:
        setup = f'const installed = function({parameter}) {{ {body} }};\nconst receiver = {{}};\n'
        if form in ('define-property', 'key-coercion'):
            setup += f'Object.defineProperty(receiver, "x", {{ {kind}: installed }});\n'
        elif form == 'annex-b':
            setup += f'receiver.{define}("x", installed);\n'
        elif form in ('data-shadow', 'opposite-shadow'):
            setup += f'const parent = {{}}; parent.{define}("x", installed);\nObject.setPrototypeOf(receiver, parent);\n'
            descriptor = 'value: 0' if form == 'data-shadow' else ('set(value) {}' if kind == 'get' else 'get() { return 0; }')
            setup += f'Object.defineProperty(receiver, "x", {{ {descriptor} }});\n'
            expected_identity = 'undefined'
        elif form == 'missing':
            expected_identity = 'undefined'
    if form == 'key-coercion':
        setup += 'const key = { toString() { trace.push("key"); return "x"; } };\n'
        key = 'key'
        expected_hooks = 'key'
    setup += 'receiver.unrelated = () => trace.push("BAD-member");\n'
    setup += 'Object.defineProperty(receiver, "other", { get() { trace.push("BAD-other-getter"); return 0; } });\n'
    call = f'receiver.{method}({key})'
    body = prefix + setup + f'const found = {call};\n'
    if expected_identity == 'undefined':
        body += 'console.log("EV identity " + (found === undefined ? "undefined" : "BAD"));\n'
    else:
        body += 'console.log("EV identity " + (found === installed));\n'
    body += 'console.log("EV hooks " + trace.join("|"));\n}\nrun();\n'
    # Only the native scratch input breaks the claimed passivity; Node remains
    # the unchanged oracle, so this must make the comparison red.
    broken = body.replace(f'const found = {call};', 'receiver.unrelated();\n' + f'const found = {call};')
    expected = f'EV identity {expected_identity}\nEV hooks {expected_hooks}\n'
    return body, broken, expected


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--perry', required=True)
    ap.add_argument('--node', default='node')
    ap.add_argument('--out', type=Path, required=True)
    ap.add_argument('--break-passivity', action='store_true', help='invoke an unrelated receiver member in the native scratch input')
    args = ap.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, PERRY_NO_CACHE='1', NODE_NO_WARNINGS='1')
    count = red = 0
    for kind in ('get', 'set'):
        for form in ('literal', 'class', 'inherited', 'define-property', 'annex-b', 'data-shadow', 'opposite-shadow', 'missing', 'key-coercion'):
            count += 1
            name = kind + '-' + form
            source, broken, expected = program(kind, form)
            node_src = args.out / (name + '.node.ts')
            native_src = args.out / (name + '.ts')
            node_src.write_text(source)
            native_src.write_text(broken if args.break_passivity else source)
            node = subprocess.run([args.node, '--experimental-strip-types', str(node_src)], env=env, capture_output=True, text=True, timeout=60)
            native, error = compile_run(args.perry, native_src, env)
            (args.out / (name + '.node.log')).write_text(node.stdout + node.stderr)
            (args.out / (name + '.perry.log')).write_text(native + error)
            good = not error and node.returncode == 0 and node.stdout == expected and native == node.stdout
            red += not good
            print(f'{"GREEN" if good else "RED"} {name}', flush=True)
            if not good:
                print(f'expected={expected!r} node={node.stdout!r} native={native!r} error={error!r}', flush=True)
    print(f'programs={count} red={red}')
    return int(red != 0)


if __name__ == '__main__':
    raise SystemExit(main())
