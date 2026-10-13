#!/usr/bin/env python3
"""Schema-decode acceptance: Node parity, THP-off n=5 A/B, text and GC counts."""
import argparse
import ctypes
import json
from pathlib import Path
import re
import statistics

import perf_objops_lane as common

PROGRAMS = dict(common.PROGRAMS, **{
    'binarytrees_plain': ('binarytrees_plain.ts', []),
    'binarytrees_arena': ('binarytrees_arena.ts', []),
    'root-survival': ('root-survival.ts', []),
})
MICROS = ('class_getter', 'object_array', 'class_virtual', 'object_spread')
original_environment = common.environment
original_normalized = common.normalized
original_compile_one = common.compile_one


def environment(root, arm):
    env = original_environment(root, arm)
    env.update(PERRY_TARGET_CPU='x86-64-v3', PERRY_MODULE_JOBS='8', PERRY_CODEGEN_UNIT_JOBS='1')
    env['RUSTFLAGS'] = ('-C force-unwind-tables=yes -Z use-sync-unwind=yes '
                        '-C force-frame-pointers=yes -C target-cpu=x86-64-v3')
    return env


def normalized(name, output):
    if name in MICROS:
        return re.sub(rb'^elapsed_ms:.*\n', b'', output, flags=re.M)
    if name == 'root-survival':
        return re.sub(rb'^ms [0-9.]+\n', b'', output, flags=re.M)
    return original_normalized(name, output)


common.PROGRAMS = PROGRAMS
common.environment = environment
common.normalized = normalized


def compile_one(root, arm, name, source, cwd, expected, args):
    # tsc is one large module; the package drivers have many smaller modules.
    # Both schedules cap LLVM workers at eight and preserve the same IR units.
    def compile_environment(root, arm):
        env = environment(root, arm)
        if name == 'tsc':
            env.update(PERRY_MODULE_JOBS='1', PERRY_CODEGEN_UNIT_JOBS='8')
        return env
    common.environment = compile_environment
    try:
        return original_compile_one(root, arm, name, source, cwd, expected, args)
    finally:
        common.environment = environment


common.compile_one = compile_one


def class_micros(root, arms, measure):
    path = root / 'measure/class-micros.json'
    results = json.loads(path.read_text()) if path.exists() else {}
    for name in MICROS:
        source = root / 'programs/micros' / f'{name}.ts'
        row = results.setdefault(name, {})
        expected = {}
        for n in (1000000, 5000000):
            expected[n] = common.execute(['node', str(source.with_suffix('.js')), str(n)],
                                         source.parent, environment(root, 'head'),
                                         root / 'measure' / f'{name}.node.N{n}').stdout
        for arm in arms:
            row[arm] = common.compile_one(root, arm, name, source, source.parent,
                                          expected[1000000], ['1000000'])
            assert row[arm]['status'] == 'pass', (name, arm, row[arm])
        if measure:
            samples = {arm: {n: [] for n in expected} for arm in ('main', 'head', 'node')}
            for trial in range(5):
                order = ('main', 'head', 'node') if trial % 2 == 0 else ('node', 'head', 'main')
                for n in expected:
                    for arm in order:
                        prefix = root / 'measure/trials' / f'{name}.{arm}.{trial}.N{n}'
                        cmd = ['node', str(source.with_suffix('.js'))] if arm == 'node' else [
                            str(root / 'measure' / arm / name)]
                        result = common.execute(['setarch', '-R', 'perf', 'stat', '-x', ';',
                                                 '-e', 'instructions:u', '-o', str(prefix) + '.stat',
                                                 '/usr/bin/time', '-f', '%M', '-o', str(prefix) + '.rss',
                                                 *cmd, str(n)], source.parent, environment(root, arm), prefix)
                        assert normalized(name, result.stdout) == normalized(name, expected[n])
                        instructions = next(int(line.split(';')[0]) for line in
                                            Path(str(prefix) + '.stat').read_text().splitlines()
                                            if ';instructions:u;' in line)
                        samples[arm][n].append({'instructions': instructions,
                                               'rss_kb': int(Path(str(prefix) + '.rss').read_text())})
            row['trials'] = samples
            row['instructions_per_iteration'] = {
                arm: (statistics.median(s['instructions'] for s in ns[5000000]) -
                      statistics.median(s['instructions'] for s in ns[1000000])) / 4000000
                for arm, ns in samples.items()}
        path.write_text(json.dumps(results, indent=2))
        print(name, row.get('instructions_per_iteration', row), flush=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--hostdir', type=Path, required=True)
    parser.add_argument('--arms', default='main,head')
    parser.add_argument('--programs', default=','.join(PROGRAMS))
    parser.add_argument('action', choices=('compile', 'compare', 'gc', 'micros', 'compile-micros'))
    args = parser.parse_args()
    root = args.hostdir.resolve()
    for directory in ('tmp', 'measure/main', 'measure/head', 'measure/trials'):
        (root / directory).mkdir(parents=True, exist_ok=True)
    if ctypes.CDLL(None, use_errno=True).prctl(41, 1, 0, 0, 0):
        raise OSError(ctypes.get_errno(), 'PR_SET_THP_DISABLE')
    names = args.programs.split(',')
    if args.action == 'compile':
        common.compile_set(root, names, arms=args.arms.split(','))
    elif args.action == 'compare':
        common.compare(root, names)
    elif args.action == 'gc':
        common.gc(root, names)
    else:
        class_micros(root, args.arms.split(','), args.action == 'micros')


if __name__ == '__main__':
    main()
