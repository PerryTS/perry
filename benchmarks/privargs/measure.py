#!/usr/bin/env python3
"""Instruction-only, interleaved measurements on perrymaster. No builds."""
import argparse
import ctypes
import json
import os
from pathlib import Path
import statistics
import subprocess

MODES = ['plain', 'public_field', 'private_field', 'public_method',
         'private_method', 'private_bound', 'arguments_read',
         'arguments_escape', 'arguments_cold', 'u8', 'u8_alias', 'u32', 'u32_alias',
         'buffer', 'buffer_alias']
PROGRAMS = {
    'hello': ('hello.ts', []),
    'zod': ('zodwork.ts', ['5000']),
    'tsc': ('tscwork.ts', ['1']),
    'qsparse': ('pk/qs/parse_nested.ts', ['20000', '1000']),
    'qsstr': ('pk/qs/stringify_nested.ts', ['20000', '1000']),
    'commander': ('pk/commander/parse_argv.ts', ['5000', '200']),
    'fastify': ('pk/fastify/inject.ts', ['500', '30']),
    'effect': ('effect.ts', []),
    'buffer_heavy': ('buffer_heavy.ts', []),
    'worker_heavy': ('worker_heavy.ts', []),
}
CLASS_MICROS = ['class_getter', 'object_array', 'class_virtual', 'object_spread']

def construct_sources(root):
    original = (root / 'micros/constructs.ts').read_text()
    prefix = original[:original.index('const mode =')]
    expressions = {
        'plain': 'sum += plain(v, 3);',
        'public_field': 'sum += pub.read(v);',
        'private_field': 'sum += priv.read(v);',
        'public_method': 'sum += pub.m(v);',
        'private_method': 'sum += priv.m(v);',
        'private_bound': 'sum += bound(v);',
        'arguments_read': 'sum += argsRead(v, 3);',
        'arguments_escape': 'const a = argsEscape(v, 3); sum += a[0] + a[1];',
        'arguments_cold': 'sum += argsCold(v, 3);',
        'u8': 'sum += bytes(u8, i & 1023);',
        'u32': 'sum += words(u32, i & 1023);',
        'buffer': 'sum += buffers(buffer, i & 1023);',
    }
    for mode in MODES:
        base = mode.removesuffix('_alias')
        alias = '' if mode == base else '''const backing = new ArrayBuffer(1024);
const view = new Uint32Array(backing);
view[1] = 17;
if (new Uint8Array(backing)[4] !== 17) throw new Error('alias');
'''
        source = prefix + alias + '''function kernel(n: number) {
    let sum = 0;
    for (let i = 0; i < n; i++) {
        const v = i & 255;
        ''' + expressions[base] + '''
    }
    return sum;
}
for (let i = 0; i < 10; i++) kernel(20000);
console.log(kernel(Number(process.argv[2] || 200000)));
'''
        (root / 'micros' / (mode + '.ts')).write_text(source)
        js = source.replace(': number', '').replace(': Uint8Array', '').replace(': Uint32Array', '').replace(': Buffer', '')
        prepare = '''
const targets = [plain, argsRead, argsEscape, argsCold, bytes, words, buffers,
                 Public.prototype.read, Public.prototype.m,
                 Private.prototype.read, Private.prototype.m, kernel];
for (const fn of targets) %PrepareFunctionForOptimization(fn);
'''
        optimize = '''
for (const fn of targets) %OptimizeFunctionOnNextCall(fn);
plain(1, 3); argsRead(1, 3); argsEscape(1, 3); argsCold(1, 3);
bytes(u8, 1); words(u32, 1); pub.read(1); pub.m(1); priv.read(1); priv.m(1);
buffers(buffer, 1);
kernel(20000);
'''
        js = js.replace('for (let i = 0; i < 10; i++) kernel(20000);', prepare + 'for (let i = 0; i < 10; i++) kernel(20000);' + optimize)
        markers = '''
const fs = require('node:fs');
const control = process.env.PERRY_PERF_CONTROL ? fs.openSync(process.env.PERRY_PERF_CONTROL, 'w') : null;
const ack = control === null ? null : fs.openSync(process.env.PERRY_PERF_ACK, 'r');
const ackBuf = Buffer.alloc(4);
function counter(command) {
    if (control === null) return;
    fs.writeSync(control, command + '\\n');
    fs.readSync(ack, ackBuf, 0, 4, null);
}
counter('enable');
const result = kernel(Number(process.argv[2] || 200000));
counter('disable');
console.log(result);
'''
        js = js.replace('console.log(kernel(Number(process.argv[2] || 200000)));', markers)
        (root / 'micros' / (mode + '.js')).write_text(js)

def run(cmd, env, cwd, timeout=1800):
    return subprocess.run(cmd, env=env, cwd=cwd, capture_output=True, timeout=timeout)

def environment(root, arm):
    env = dict(os.environ)
    env.update(CARGO_TARGET_DIR=str(root / arm / 'target'),
               PERRY_RUNTIME_DIR=str(root / arm / 'target/release'),
               PERRY_WORKSPACE_ROOT=str(root / arm / 'src'), RUST_TEST_THREADS='1',
               PERRY_NO_AUTO_OPTIMIZE='1', PERRY_NO_CACHE='1',
               PERRY_KEEP_SYMBOLS='1', PERRY_ALLOW_PERRY_FEATURES='1',
               PERRY_LL_RS4GC_MAX_INSTRS='2097152',
               RAYON_NUM_THREADS='4',
               CARGO_BUILD_JOBS='4',
               XDG_CACHE_HOME=str(root / 'cache'),
               PERF_BUILDID_DIR=str(root / 'perf-cache'),
               TMPDIR=str(root / 'tmp'))
    return env

def compile_one(root, arm, name, source):
    env = environment(root, arm)
    dest = root / 'bin' / arm
    dest.mkdir(parents=True, exist_ok=True)
    result = run([str(root / arm / 'target/release/perry'), 'compile',
                  str(source), '-o', str(dest / name)], env, root)
    (dest / (name + '.compile.log')).write_bytes(result.stdout + result.stderr)
    return result.returncode == 0

def driver_cwd(root, relative):
    return root / 'drivers' / ('pk' if relative.startswith('pk/') else '')

def stat(root, label, cmd, env, cwd):
    logs = root / 'trials'
    logs.mkdir(exist_ok=True)
    counter, rss = logs / (label + '.stat'), logs / (label + '.rss')
    control_args = []
    if cmd[0] == 'node':
        ctl, ack = logs / (label + '.ctl'), logs / (label + '.ack')
        for path in [ctl, ack]:
            path.unlink(missing_ok=True)
            os.mkfifo(path)
        env = dict(env, PERRY_PERF_CONTROL=str(ctl), PERRY_PERF_ACK=str(ack))
        control_args = ['--delay=-1', '--control=fifo:' + str(ctl) + ',' + str(ack)]
    result = run(['setarch', '-R', 'perf', 'stat', *control_args, '-x', ';', '-e', 'instructions:u',
                  '-o', str(counter), '/usr/bin/time', '-f', '%M', '-o', str(rss),
                  *cmd], env, cwd)
    (logs / (label + '.out')).write_bytes(result.stdout)
    (logs / (label + '.err')).write_bytes(result.stderr)
    if result.returncode:
        raise RuntimeError(f'{label}: {result.returncode}: {result.stderr[-2000:]}')
    count = next(int(s.split(';')[0]) for s in counter.read_text().splitlines()
                 if ';instructions:u;' in s)
    return dict(instructions=count, rss=int(rss.read_text().strip()))

def normalized(data):
    return b'\n'.join(line for line in data.splitlines()
                       if not line.startswith((b'elapsed_ms', b'elapsed:', b'ms:')))

def measure_micro(root, name, arms, source, args):
    trials = {arm: [] for arm in arms}
    for i in range(5):
        for arm in arms if i % 2 == 0 else arms[::-1]:
            env = environment(root, 'main' if arm == 'node' else arm)
            cmd = ['node', '--disable-warning=MODULE_TYPELESS_PACKAGE_JSON',
                   '--allow-natives-syntax', '--no-concurrent-recompilation',
                   '--max-inlined-bytecode-size=0', str(source)] if arm == 'node' else [str(root / 'bin' / arm / name)]
            pair = [stat(root, f'{name}.{args}.{arm}.{i}.{n}', [*cmd, str(n), *args], env, root)
                    for n in [50000, 200000]]
            trials[arm].append(dict(instructions_per_op=(pair[1]['instructions'] - pair[0]['instructions']) / 150000,
                                    rss=pair[1]['rss']))
    return {arm: {**{key: statistics.median(row[key] for row in rows) for key in rows[0]},
                  'instruction_range': max(row['instructions_per_op'] for row in rows) - min(row['instructions_per_op'] for row in rows),
                  'trials': rows} for arm, rows in trials.items()}

def micro_metadata(root, name, arms, results):
    for arm in arms:
        env = environment(root, arm)
        binary = root / 'bin' / arm / name
        size = run(['size', '-A', str(binary)], env, root).stdout.decode()
        results[arm]['text'] = int(next(l.split()[1] for l in size.splitlines() if l.startswith('.text ')))
        diag = run([str(binary), '200000'], dict(env, PERRY_GC_TRACE='1'), root)
        (root / 'trials' / f'{name}.{arm}.gc').write_bytes(diag.stderr)

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('root', type=Path)
    parser.add_argument('phase', choices=['rank', 'compile', 'compare'])
    parser.add_argument('--arms', nargs='+', default=['main', 'head'])
    parser.add_argument('--set', choices=['all', 'real', 'micros'], default='all')
    parser.add_argument('--label', default='ab')
    parser.add_argument('--programs', nargs='+', choices=list(PROGRAMS), default=list(PROGRAMS))
    a = parser.parse_args()
    root = a.root.resolve()
    allowed = set(range(24)) | set(range(32, 56))
    os.sched_setaffinity(0, os.sched_getaffinity(0) & allowed)
    ctypes.CDLL(None).prctl(41, 1, 0, 0, 0)  # inherited PR_SET_THP_DISABLE
    (root / 'tmp').mkdir(exist_ok=True)
    construct_sources(root)
    results = {}
    if a.phase == 'rank':
        arm = a.arms[0]
        for mode in MODES:
            assert compile_one(root, arm, mode, root / 'micros' / (mode + '.ts'))
            results[mode] = measure_micro(root, mode, [arm, 'node'], root / 'micros' / (mode + '.js'), [])
            (root / ('rank.json' if arm == 'main' else 'rank-' + arm + '.json')).write_text(json.dumps(results, indent=2))
            print(mode, results[mode], flush=True)
        return
    arms = a.arms
    if a.phase == 'compile':
        for arm in arms:
            prior = root / ('correctness-' + arm + '.json')
            for name in ([] if a.set == 'micros' else a.programs):
                relative, args = PROGRAMS[name]
                source = root / 'drivers' / relative
                good = compile_one(root, arm, name, source)
                if good:
                    env = environment(root, arm)
                    cwd = driver_cwd(root, relative)
                    actual = run([str(root / 'bin' / arm / name), *args], env, cwd)
                    expected = run(['node', '--disable-warning=MODULE_TYPELESS_PACKAGE_JSON', str(source), *args], env, cwd)
                    good = actual.returncode == expected.returncode == 0 and normalized(actual.stdout) == normalized(expected.stdout)
                    (root / 'bin' / arm / (name + '.node.out')).write_bytes(expected.stdout)
                    (root / 'bin' / arm / (name + '.out')).write_bytes(actual.stdout)
                    (root / 'bin' / arm / (name + '.err')).write_bytes(actual.stderr)
                results[f'{arm}/{name}'] = good
                print(f'{arm}/{name}: {good}', flush=True)
                saved = json.loads(prior.read_text()) if prior.exists() else {}
                saved.update({k: v for k, v in results.items() if k.startswith(arm + '/')})
                prior.write_text(json.dumps(saved, indent=2))
            for name in ([] if a.set == 'real' else [*MODES, *CLASS_MICROS]):
                source = root / ('micros' if name in MODES else 'class-micros') / (name + '.ts')
                assert compile_one(root, arm, name, source), name
                node = ['node', '--allow-natives-syntax', str(source.with_suffix('.js')), '200000']
                env = environment(root, arm)
                expected = run(node, env, root)
                actual = run([str(root / 'bin' / arm / name), '200000'], env, root)
                assert actual.returncode == expected.returncode == 0 and normalized(actual.stdout) == normalized(expected.stdout), name
        return
    correctness = {}
    for arm in arms:
        correctness.update(json.loads((root / ('correctness-' + arm + '.json')).read_text()))
    ab_path = root / (a.label + ('' if a.set == 'all' else '-' + a.set) + '.json')
    for name in ([] if a.set == 'micros' else a.programs):
        relative, args = PROGRAMS[name]
        if not all(correctness[f'{arm}/{name}'] for arm in arms):
            continue
        rows = {arm: [] for arm in arms}
        for i in range(5):
            for arm in arms if i % 2 == 0 else arms[::-1]:
                env = environment(root, arm)
                binary = root / 'bin' / arm / name
                rows[arm].append(stat(root, f'{name}.{arm}.{i}', [str(binary), *args], env, driver_cwd(root, relative)))
                expected = (root / 'bin' / arm / (name + '.node.out')).read_bytes()
                assert normalized((root / 'trials' / f'{name}.{arm}.{i}.out').read_bytes()) == normalized(expected), name
        results[name] = {arm: {k: statistics.median(r[k] for r in row) for k in row[0]} for arm, row in rows.items()}
        for arm in arms:
            results[name][arm]['trials'] = rows[arm]
        for arm in arms:
            binary = root / 'bin' / arm / name
            size = run(['size', '-A', str(binary)], environment(root, arm), root).stdout.decode()
            results[name][arm]['text'] = int(next(l.split()[1] for l in size.splitlines() if l.startswith('.text ')))
            diag = run([str(binary), *args], dict(environment(root, arm), PERRY_GC_TRACE='1'), driver_cwd(root, relative))
            (root / 'trials' / f'{name}.{arm}.gc').write_bytes(diag.stderr)
        print(name, results[name], flush=True)
        ab_path.write_text(json.dumps(results, indent=2))
    for name in ([] if a.set == 'real' else CLASS_MICROS):
        results[name] = measure_micro(root, name, arms, root / 'class-micros' / (name + '.js'), [])
        micro_metadata(root, name, arms, results[name])
        print(name, results[name], flush=True)
        ab_path.write_text(json.dumps(results, indent=2))
    for mode in ([] if a.set == 'real' else MODES):
        results[mode] = measure_micro(root, mode, arms, root / 'micros' / (mode + '.js'), [])
        micro_metadata(root, mode, arms, results[mode])
        print(mode, results[mode], flush=True)
        ab_path.write_text(json.dumps(results, indent=2))

if __name__ == '__main__':
    main()
