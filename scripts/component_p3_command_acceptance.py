#!/usr/bin/env python3
"""Qualify P3 command execution from an actual compiled TypeScript host."""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess


def immediate(status):
    return f'''(component
      (core func $return (canon task.return (result (result))))
      (core module $m
        (import "host" "return" (func $return (param i32)))
        (func (export "run") (result i32) (call $return (i32.const {status})) (i32.const 0))
        (func (export "callback") (param i32 i32 i32) (result i32) unreachable))
      (core instance $m (instantiate $m (with "host" (instance (export "return" (func $return))))))
      (func $run async (result (result)) (canon lift (core func $m "run") async (callback (core func $m "callback"))))
      (instance $run (export "run" (func $run)))
      (export "wasi:cli/run@0.3.0" (instance $run)))'''


def exit_source(status, start=False, malformed=False):
    startup = f'(func $start (call $exit (i32.const {status})) unreachable) (start $start)' if start else ''
    run = 'unreachable' if start else f'(call $exit (i32.const {status})) unreachable'
    result = 'u32' if malformed else '(result)'
    return f'''(component
      (import "wasi:cli/exit@0.3.0" (instance $exit (export "exit-with-code" (func (param "status-code" u8)))))
      (alias export $exit "exit-with-code" (func $exit))
      (core func $exit (canon lower (func $exit)))
      (core module $m
        (import "host" "exit" (func $exit (param i32))) {startup}
        (func (export "run") (result i32) {run})
        (func (export "callback") (param i32 i32 i32) (result i32) unreachable))
      (core instance $m (instantiate $m (with "host" (instance (export "exit" (func $exit))))))
      (func $run async (result {result}) (canon lift (core func $m "run") async (callback (core func $m "callback"))))
      (instance $run (export "run" (func $run)))
      (export "wasi:cli/run@0.3.0" (instance $run)))'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['perry', 'runtime-dir', 'wat-encoder', 'output']:
        parser.add_argument('--' + name, required=True, type=Path)
    parser.add_argument('--moving-gc', action='store_true')
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    shutil.copyfile(root / 'test-wasi/components/p3-command/host.ts', out / 'host.ts')
    env = os.environ | {'PERRY_RUNTIME_DIR': str(args.runtime_dir.resolve()),
                        'PERRY_NO_AUTO_OPTIMIZE': '1', 'PERRY_CODEGEN_CPU_SLOTS': '4'}
    rows = []

    def run(name, command, extra=None, data=None):
        result = subprocess.run(list(map(str, command)), cwd=out, env=env | (extra or {}),
                                input=data, capture_output=True, timeout=600)
        (out / (name + '.stdout')).write_bytes(result.stdout)
        (out / (name + '.stderr')).write_bytes(result.stderr)
        rows.append({'phase': name, 'exitCode': result.returncode})
        (out / 'results.json').write_text(json.dumps(rows, indent=2) + '\n')
        print(('PASS ' if result.returncode == 0 else 'FAIL ') + name, flush=True)
        if result.returncode:
            raise SystemExit(result.returncode)
        return result

    commands = {'ok': immediate(0), 'err': immediate(1), 'exit': exit_source(23),
                'init-exit': exit_source(255, start=True), 'bad-init': exit_source(0, start=True, malformed=True),
                'wrong-version': immediate(0).replace('@0.3.0', '@0.3.1')}
    for name, fixture in [('clock', 'p3-clock.component.wat'), ('closed-stdio', 'p3-stdio.component.wat')]:
        commands[name] = (root / 'crates/perry-component-host/src/command' / fixture).read_text()
    commands['late-trap'] = commands['clock'].replace('(call $return (i32.const 0))', '').replace('(global.set $start (call $now))', '(call $return (i32.const 0)) (global.set $start (call $now))').replace('(call $drop-set (global.get $set))', '(call $drop-set (global.get $set)) unreachable')
    for name, source in commands.items():
        (out / (name + '.wat')).write_text(source)
        run('encode-' + name, [args.wat_encoder.resolve(), out / (name + '.wat'), out / (name + '.wasm')])
    payload = bytes((n * 17 + 31) % 256 for n in range(3 * 1024 * 1024 + 137))
    for moving in ([False, True] if args.moving_gc else [False]):
        name = 'moving' if moving else 'ordinary'
        instruments = {key: '1' for key in ['PERRY_GC_INSTRUMENTS', 'PERRY_GC_FORCE_EVACUATE',
            'PERRY_GC_VERIFY_EVACUATION', 'PERRY_GC_PROTECT_FROMSPACE', 'PERRY_GC_DIAG']} if moving else {}
        executable = out / ('host-' + name)
        run('compile-' + name, [args.perry.resolve(), 'compile', out / 'host.ts', '--no-cache', '--no-auto-optimize', '-o', executable], instruments)
        results = run(name, [executable, 'cases', out], instruments)
        assert b'PASS P3 command: returns, exit, startup validation, versions, real async clock, closed defaults' in results.stdout
        io = run('stdio-' + name, [executable, 'stdio', out / 'closed-stdio.wasm'], instruments, payload)
        assert io.stdout == payload and b'PASS P3 command: exact native stdio completed' in io.stderr
        if moving:
            diagnostics = results.stderr.decode() + io.stderr.decode()
            assert sum(map(int, re.findall(r'\bcopied_objects=(\d+)', diagnostics))) > 0
            assert 'evacuation_ok' in diagnostics
            assert re.search(r'mode=ProtectPages retired_set=#\d+[^\n]*\bbytes_protected=[1-9]\d*', diagnostics)


if __name__ == '__main__':
    main()
