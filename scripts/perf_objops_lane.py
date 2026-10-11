#!/usr/bin/env python3
"""Isolated objops verification on qb6: Node parity, n=5 instruction/RSS A/B, .text.

Invocation: taskset -c 0-55 setarch -R python3 scripts/perf_objops_lane.py
--hostdir /root/codex-lanes/cx-objops <micro|compile|compare|gc|gaps>.
No builds, installs, locks, or writes outside --hostdir. Both compiler arms
must first be built in separate targets using the same package set.
"""
import argparse
import ctypes
import json
import os
from pathlib import Path
import re
import statistics
import subprocess

PROGRAMS = {
    'tsc': ('tscwork.ts', ['3']), 'zod5k': ('zodwork.ts', ['5000']),
    'qsparse': ('pk/qs/parse_nested.ts', ['20000', '1000']),
    'qsstr': ('pk/qs/stringify_nested.ts', ['20000', '1000']),
    'commander': ('pk/commander/parse_argv.ts', ['5000', '200']),
    'commander-construct': ('pk/commander/construct.ts', ['5000']),
    'hello': ('hello.ts', []), 'fastify': ('pk/fastify/inject.ts', ['500', '30']),
    'effect': ('effect/main.ts', []), 'schema-record': ('effect/schema-record.ts', []),
    'buffer_heavy': ('buffer_heavy.ts', []), 'worker_heavy': ('worker_heavy.ts', []),
}
NODE = ['node', '--disable-warning=MODULE_TYPELESS_PACKAGE_JSON', '--experimental-strip-types']


def environment(root, arm):
    base = root / 'main' if arm == 'main' else root
    return dict(os.environ, CARGO_TARGET_DIR=str(base / 'target'),
                PERRY_RUNTIME_DIR=str(base / 'target/release'),
                PERRY_WORKSPACE_ROOT=str(base / 'src'), RUST_TEST_THREADS='1',
                CARGO_BUILD_JOBS='8', RAYON_NUM_THREADS='8', PERRY_CODEGEN_UNIT_JOBS='8',
                PERRY_LL_RS4GC_MAX_INSTRS='2097152', PERRY_KEEP_SYMBOLS='1',
                PERRY_NO_CACHE='1', PERRY_NO_AUTO_OPTIMIZE='1', PERRY_ALLOW_PERRY_FEATURES='1',
                PERRY_CACHE_DIR=str(root / 'cache' / arm),
                TMPDIR=str(root / 'tmp'))


def execute(cmd, cwd, env, prefix, allow_fail=False):
    result = subprocess.run(cmd, cwd=cwd, env=env, capture_output=True, timeout=2700)
    prefix.with_suffix(prefix.suffix + '.out').write_bytes(result.stdout)
    prefix.with_suffix(prefix.suffix + '.err').write_bytes(result.stderr)
    if result.returncode and not allow_fail:
        raise RuntimeError(f'{result.returncode}: {cmd}: {result.stderr[-2500:]!r}')
    return result


def normalized(name, output):
    if name == 'schema-record':
        return re.sub(rb'^ms [0-9.]+\n', b'', output, flags=re.M)
    if name == 'effect':
        return re.sub(rb'construct2000=\d+ms decode20000=\d+ms ', b'', output)
    return output


def program_cwd(root, relative):
    return root / 'programs/pk' if relative.startswith('pk/') else (root / 'programs' / relative).parent


def oracle(root, name, source, args, cwd):
    return execute([*NODE, str(source), *args], cwd, environment(root, 'head'),
                   root / 'measure' / f'{name}.node').stdout


def compile_one(root, arm, name, source, cwd, expected, args):
    env = environment(root, arm)
    binary = root / 'measure' / arm / name
    result = execute([str(Path(env['CARGO_TARGET_DIR']) / 'release/perry'),
                      'compile', str(source), '--keep-intermediates', '-o', str(binary)], cwd, env,
                     binary.with_suffix('.compile'), allow_fail=True)
    if result.returncode:
        return {'status': 'compile-fail', 'exit': result.returncode}
    result = execute([str(binary), *args], cwd, env, binary.with_suffix('.check'), allow_fail=True)
    same = result.returncode == 0 and normalized(name, result.stdout) == normalized(name, expected)
    text = subprocess.check_output(['size', '-A', str(binary)], text=True)
    emitted = int(re.search(r'^\.text\s+(\d+)', text, re.M)[1])
    return {'status': 'pass' if same else 'output-fail', 'exit': result.returncode, 'text': emitted}


def compile_set(root, names, micro=False, arms=('main', 'head')):
    result_path = root / 'measure' / ('micro-status.json' if micro else 'status.json')
    status = json.loads(result_path.read_text()) if result_path.exists() else {}
    for name in names:
        if micro:
            source, args, cwd = root / 'src/benchmarks/objops' / f'{name}.ts', ['200000'], root / 'src'
        else:
            relative, args = PROGRAMS[name]
            source = root / 'programs' / relative
            cwd = program_cwd(root, relative)
        expected = oracle(root, name, source, args, cwd)
        status.setdefault(name, {})
        for arm in arms:
            status[name][arm] = compile_one(root, arm, name, source, cwd, expected, args)
            print(name, arm, status[name][arm], flush=True)
        result_path.write_text(json.dumps(status, indent=2))


def stat(root, arm, name, trial, args, cwd, node=False):
    env = environment(root, arm)
    prefix = root / 'measure/trials' / f'{name}-{arm}-{trial}'
    command = [*NODE, str(root / 'src/benchmarks/objops' / f'{name}.ts')] if node else [str(root / 'measure' / arm / name)]
    command += args
    result = execute(['perf', 'stat', '-x', ';', '-e', 'instructions:u', '-o', str(prefix.with_suffix('.stat')),
                      '/usr/bin/time', '-f', '%M', '-o', str(prefix.with_suffix('.rss')), *command],
                     cwd, env, prefix)
    expected = (root / 'measure' / f'{name}.node.out').read_bytes()
    if normalized(name, result.stdout) != normalized(name, expected):
        raise RuntimeError(f'{name}/{arm}: trial output differs')
    count = re.search(r'^(\d+);;instructions:u;', prefix.with_suffix('.stat').read_text(), re.M)
    if count is None:
        raise RuntimeError('missing instruction counter')
    return {'instructions': int(count[1]), 'rss_kb': int(prefix.with_suffix('.rss').read_text().strip())}


def compare(root, names, micro=False):
    if ctypes.CDLL(None, use_errno=True).prctl(41, 1, 0, 0, 0):
        raise OSError(ctypes.get_errno(), 'PR_SET_THP_DISABLE')
    path = root / 'measure' / ('micro-results.json' if micro else 'results-thp-off.json')
    result = json.loads(path.read_text()) if path.exists() else {}
    status = json.loads((root / 'measure' / ('micro-status.json' if micro else 'status.json')).read_text())
    for name in names:
        if any(status[name][arm]['status'] != 'pass' for arm in ('main', 'head')):
            print('skip known failure', name, flush=True)
            continue
        if micro:
            args, cwd = ['200000'], root / 'src'
        else:
            relative, args = PROGRAMS[name]
            cwd = program_cwd(root, relative)
        arms = ('main', 'head', 'node') if micro else ('main', 'head')
        rows = {arm: [] for arm in arms}
        for trial in range(5):
            for arm in arms if trial % 2 == 0 else reversed(arms):
                rows[arm].append(stat(root, arm, name, trial, args, cwd, arm == 'node'))
        result[name] = {'trials': rows, 'medians': {
            arm: {key: statistics.median(row[key] for row in values) for key in values[0]}
            for arm, values in rows.items()}, 'text': {arm: status[name][arm]['text'] for arm in ('main', 'head')}}
        path.write_text(json.dumps(result, indent=2))
        print(name, result[name]['medians'], result[name]['text'], flush=True)


def gc(root, names):
    if ctypes.CDLL(None, use_errno=True).prctl(41, 1, 0, 0, 0):
        raise OSError(ctypes.get_errno(), 'PR_SET_THP_DISABLE')
    status = json.loads((root / 'measure/status.json').read_text())
    counts = {}
    for name in names:
        if any(status[name][arm]['status'] != 'pass' for arm in ('main', 'head')):
            print('skip known failure', name, flush=True)
            continue
        counts[name] = {}
        source, args = PROGRAMS[name]
        for arm in ('main', 'head'):
            env = environment(root, arm)
            env.update(PERRY_GC_DIAG='1', PERRY_GC_TRACE='1')
            result = execute([str(root / 'measure' / arm / name), *args],
                             program_cwd(root, source), env,
                             root / 'measure' / arm / f'{name}.gc')
            expected = (root / 'measure' / f'{name}.node.out').read_bytes()
            if normalized(name, result.stdout) != normalized(name, expected):
                raise RuntimeError(f'GC {name}/{arm}: output differs')
            cycles = {'full': 0, 'minor': 0}
            for line in result.stderr.decode(errors='replace').splitlines():
                if line.startswith('{'):
                    event = json.loads(line)
                    if event.get('event') == 'gc_cycle':
                        kind = event['collection_kind']
                        cycles[kind] = cycles.get(kind, 0) + 1
            counts[name][arm] = cycles
            print('GC', name, arm, cycles, flush=True)
        (root / 'measure/gc-counts.json').write_text(json.dumps(counts, indent=2))


def gaps(root):
    source = root / 'src/test-files/test_gap_objops_shape_keys_typeof_instanceof.ts'
    expected = oracle(root, 'gaps', source, [], root / 'src')
    for arm in ('main', 'head'):
        result = compile_one(root, arm, 'gaps', source, root / 'src', expected, [])
        print('gaps', arm, result, flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--hostdir', type=Path, required=True)
    parser.add_argument('--programs', default=','.join(PROGRAMS))
    parser.add_argument('--arms', default='main,head')
    parser.add_argument('action', choices=['micro', 'compile', 'compare', 'gc', 'gaps'])
    opts = parser.parse_args()
    root = opts.hostdir.resolve()
    for directory in ('tmp', 'measure/main', 'measure/head', 'measure/trials'):
        (root / directory).mkdir(parents=True, exist_ok=True)
    names = opts.programs.split(',')
    if opts.action == 'micro':
        compile_set(root, ['keys', 'typeof', 'instanceof'], True)
        compare(root, ['keys', 'typeof', 'instanceof'], True)
    elif opts.action == 'compile':
        compile_set(root, names, arms=opts.arms.split(','))
    elif opts.action == 'compare':
        compare(root, names)
    elif opts.action == 'gc':
        gc(root, names)
    else:
        gaps(root)
