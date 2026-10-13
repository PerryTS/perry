#!/usr/bin/env python3
"""Decode-only Callgrind deltas. All artifacts are written below --hostdir."""
import argparse
from collections import Counter
import ctypes
import json
import os
from pathlib import Path
import subprocess


def callgrind(path):
    names, own, entries, edges = {}, Counter(), Counter(), Counter()
    current = target = None
    pending = None
    events = 1
    total = 0

    def symbol(value):
        if value.startswith('('):
            key, _, name = value.partition(')')
            if name.strip():
                names[key] = name.strip()
            return names.get(key, key)
        return value

    for line in path.read_text().splitlines():
        if line.startswith('events:'):
            events = len(line.split()) - 1
        elif line.startswith('summary:'):
            total += int(line.split()[1])
        elif line.startswith('fn='):
            current = symbol(line[3:])
        elif line.startswith('cfn='):
            target = symbol(line[4:])
        elif line.startswith('calls='):
            pending = int(line[6:].split()[0])
        elif line and line[0] in '0123456789+-*':
            fields = line.split()
            cost = int(fields[-events])
            if pending is not None:
                entries[target] += pending
                edges[(current, target)] += cost
                pending = None
            elif current is not None:
                own[current] += cost
    return {'total': total, 'self': dict(own), 'entries': dict(entries),
            'edges': [{'caller': a, 'callee': b, 'ir': n} for (a, b), n in edges.items()]}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--hostdir', type=Path, required=True)
    parser.add_argument('--arm', default='main')
    parser.add_argument('--label')
    parser.add_argument('--node-arg', action='append', default=[])
    parser.add_argument('--parse-only', action='store_true')
    args = parser.parse_args()
    root = args.hostdir.resolve()
    out = root / 'profiles' / (args.label or args.arm)
    out.mkdir(parents=True, exist_ok=True)
    if not args.parse_only:
        if ctypes.CDLL(None, use_errno=True).prctl(41, 1, 0, 0, 0):
            raise OSError(ctypes.get_errno(), 'PR_SET_THP_DISABLE')
        lane = root / 'main' if args.arm == 'main' else root
        env = dict(os.environ, CARGO_TARGET_DIR=str(lane / 'target'),
                   PERRY_RUNTIME_DIR=str(lane / 'target/release'),
                   PERRY_WORKSPACE_ROOT=str(lane / 'src'), PERRY_NO_AUTO_OPTIMIZE='1',
                   PERRY_NO_CACHE='1', PERRY_KEEP_SYMBOLS='1', PERRY_ALLOW_PERRY_FEATURES='1',
                   PERRY_LL_RS4GC_MAX_INSTRS='2097152', RAYON_NUM_THREADS='8',
                   PERRY_MODULE_JOBS='8', PERRY_CODEGEN_UNIT_JOBS='1',
                   PERRY_CACHE_DIR=str(root / 'cache' / args.arm),
                   TMPDIR=str(root / 'tmp'))
        env['RUSTFLAGS'] = ('-C force-unwind-tables=yes -Z use-sync-unwind=yes '
                            '-C force-frame-pointers=yes -C target-cpu=x86-64-v3')
        cwd = root / 'programs/effect'
        source = cwd / 'schema-decode.ts'
        source.write_text((cwd / 'schema-record.ts').read_text().replace(
            'i < 20;', 'i < Number(process.argv[2] || "20");'))
        binary = out / 'decode'
        command = ['node', *args.node_arg, str(source)] if args.arm == 'node' else [str(binary)]
        if args.arm != 'node':
            with (out / 'compile.log').open('wb') as log:
                subprocess.run([str(lane / 'target/release/perry'), 'compile', str(source),
                                '--march', 'x86-64-v3', '-o', str(binary)],
                               cwd=cwd, env=env, stdout=log, stderr=log, check=True)
        for n in (2, 6):
            with (out / f'N{n}.out').open('wb') as stdout, (out / f'N{n}.err').open('wb') as stderr:
                subprocess.run(['setarch', '-R', 'valgrind', '--tool=callgrind',
                                '--callgrind-out-file=' + str(out / f'N{n}.cg'),
                                *command, str(n)], cwd=cwd, env=env, stdout=stdout,
                               stderr=stderr, check=True, timeout=900)
            with (out / f'N{n}.gc').open('wb') as stderr:
                gc_command = ['node', *args.node_arg, '--trace-gc-nvp', str(source)] if args.arm == 'node' else command
                subprocess.run([*gc_command, str(n)], cwd=cwd,
                               env=dict(env, PERRY_GC_TRACE='1', PERRY_GC_DIAG='1'),
                               stdout=stderr if args.arm == 'node' else subprocess.DEVNULL,
                               stderr=stderr, check=True)
            print(f'{args.arm}: N={n} Callgrind complete', flush=True)
    low, high = [callgrind(out / f'N{n}.cg') for n in (2, 6)]
    delta = {'instructions_per_decode': (high['total'] - low['total']) / 4}
    for kind in ('self', 'entries'):
        delta[kind] = dict(sorted(((name, (high[kind].get(name, 0) - low[kind].get(name, 0)) / 4)
                                 for name in low[kind].keys() | high[kind].keys()),
                                key=lambda row: -row[1]))
    (out / 'delta.json').write_text(json.dumps(delta, indent=2))
    print('instructions/decode', delta['instructions_per_decode'])
    for name, cost in list(delta['self'].items())[:55]:
        print(f'{cost:12.0f} {delta["entries"].get(name, 0):12.2f} {name}')
    print('runtime entries/decode', sum(n for name, n in delta['entries'].items()
                                       if name.startswith('js_')))


if __name__ == '__main__':
    main()
