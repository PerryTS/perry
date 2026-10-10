#!/usr/bin/env python3
"""qb6 lane verification; outputs remain inside the explicitly supplied hostdir."""
import argparse, json, os, re, statistics, subprocess
from pathlib import Path

PROGRAMS = {
    'hello': ('hello.ts', [], ''),
    'qsparse': ('qs/parse_nested.ts', ['20000', '1000'], 'pk'),
    'qsstr': ('qs/stringify_nested.ts', ['20000', '1000'], 'pk'),
    'commander': ('commander/parse_argv.ts', ['5000', '200'], 'pk'),
    'fastify': ('fastify/inject.ts', ['500', '30'], 'pk'),
    'zod5k': ('zodwork.ts', ['5000'], ''),
    'effect': ('main.ts', [], 'effect'),
    'tsc': ('tscwork.ts', ['1'], ''),
    'buffer_heavy': ('buffer_heavy.ts', [], 'heavy'),
    'worker_heavy': ('worker_heavy.ts', [], 'heavy'),
}
HEAVY = {'buffer_heavy', 'worker_heavy'}
NODE = ['node', '--disable-warning=MODULE_TYPELESS_PACKAGE_JSON', '--experimental-strip-types']


def env_for(root, arm, name=''):
    env = dict(os.environ)
    target = root / ('base-target' if arm == 'base' else 'target')
    source = root / ('base-src' if arm == 'base' else 'src')
    env.update(CARGO_TARGET_DIR=str(target), PERRY_RUNTIME_DIR=str(target / 'release'),
               PERRY_WORKSPACE_ROOT=str(source), RUST_TEST_THREADS='1', CARGO_BUILD_JOBS='8',
               RAYON_NUM_THREADS='8', PERRY_KEEP_SYMBOLS='1', PERRY_NO_AUTO_OPTIMIZE='1',
               PERRY_NO_CACHE='1', PERRY_SKIP_BUILD='1', PERRY_ALLOW_PERRY_FEATURES='1',
               TMPDIR=str(root / 'tmp'), PERF_BUILDID_DIR=str(root / 'perf-buildid'),
               XDG_CACHE_HOME=str(root / 'xdg-cache'))
    env.pop('PERRY_GC_DIAG', None)
    if name == 'tsc': env['PERRY_LL_RS4GC_MAX_INSTRS'] = '2097152'
    return env


def run(cmd, cwd, env, timeout=1800):
    return subprocess.run(cmd, cwd=cwd, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout)


def save_result(path, result):
    Path(str(path) + '.out').write_bytes(result.stdout)
    Path(str(path) + '.err').write_bytes(result.stderr)
    Path(str(path) + '.status').write_text(str(result.returncode))


def compile_arm(root, arm, names):
    out = root / 'verify' / arm
    out.mkdir(parents=True, exist_ok=True)
    statuses = {}
    for name in names:
        relative, args, directory = PROGRAMS[name]
        cwd = root / 'drivers' / directory
        env = env_for(root, arm, name)
        compiler = Path(env['CARGO_TARGET_DIR']) / 'release/perry'
        binary = out / name
        compiled = run([str(compiler), 'compile', relative, '-o', str(binary)], cwd, env)
        (out / (name + '.compile.log')).write_bytes(compiled.stdout + compiled.stderr)
        if compiled.returncode:
            statuses[name] = {'compile': compiled.returncode, 'parity': False}
            print(arm, name, statuses[name], flush=True)
            continue
        expected = run([*NODE, relative, *args], cwd, env)
        actual = run([str(binary), *args], cwd, env)
        save_result(out / (name + '.node'), expected)
        save_result(out / (name + '.perry'), actual)
        parity = expected.returncode == actual.returncode == 0 and expected.stdout == actual.stdout and expected.stderr == actual.stderr
        statuses[name] = {'compile': 0, 'node_exit': expected.returncode, 'perry_exit': actual.returncode, 'parity': parity}
        print(arm, name, statuses[name], flush=True)
    (out / 'statuses.json').write_text(json.dumps(statuses, indent=2))


def stat(root, label, command, cwd, env, thp_off=False):
    out = root / 'verify' / 'trials'
    out.mkdir(parents=True, exist_ok=True)
    counters, rss = out / (label + '.stat'), out / (label + '.rss')
    cmd = ['perf', 'stat', '-x', ';', '-e', 'instructions:u', '-o', str(counters),
           '/usr/bin/time', '-f', '%M', '-o', str(rss), *command]
    if thp_off:
        cmd = ['python3', str(root / 'src/lane-strops/thp_off.py'), *cmd]
    result = run(cmd, cwd, env)
    save_result(out / label, result)
    if result.returncode: raise RuntimeError(f'{label}: exit {result.returncode}: {result.stderr[-2000:]}')
    instructions = next(int(line.split(';')[0]) for line in counters.read_text().splitlines() if ';instructions:u;' in line)
    return {'instructions': instructions, 'rss_kb': int(rss.read_text().strip())}


def measure(root, names, thp_off=False):
    report = {'mode': 'THP off' if thp_off else 'normal', 'programs': {}}
    dest = root / 'verify' / ('thp-results.json' if thp_off else 'results.json')
    statuses = {arm: json.loads((root / 'verify' / arm / 'statuses.json').read_text()) for arm in ['base', 'head']}
    for name in names:
        if not all(statuses[arm].get(name, {}).get('parity') for arm in statuses):
            print(name, 'baseline/head parity failure; excluded from perf gate', flush=True)
            continue
        relative, args, directory = PROGRAMS[name]
        cwd = root / 'drivers' / directory
        rows = {arm: [] for arm in ['base', 'head', 'node']}
        for trial in range(5):
            for arm in (['base', 'head', 'node'] if trial % 2 == 0 else ['node', 'head', 'base']):
                cmd = [*NODE, relative, *args] if arm == 'node' else [str(root / 'verify' / arm / name), *args]
                label = f'{"thp-" if thp_off else ""}{name}-{arm}-{trial}'
                row = stat(root, label, cmd, cwd, env_for(root, arm, name), thp_off)
                actual = (root / 'verify/trials' / (label + '.out')).read_bytes()
                expected = (root / 'verify/base' / (name + '.node.out')).read_bytes()
                if actual != expected: raise RuntimeError(f'{label}: changed stdout')
                rows[arm].append(row)
        medians = {arm: {field: statistics.median(row[field] for row in armrows) for field in ['instructions', 'rss_kb']} for arm, armrows in rows.items()}
        # Diag runs are separate, so logging does not pollute instruction measurements.
        diag = {}
        for arm in ['base', 'head']:
            env = env_for(root, arm, name)
            env['PERRY_GC_DIAG'] = '1'
            result = run([str(root / 'verify' / arm / name), *args], cwd, env)
            (root / 'verify' / arm / (name + '.gcdiag')).write_bytes(result.stderr)
            diag[arm] = {'exit': result.returncode, 'full': len(re.findall(rb'^\[gc-full\] ', result.stderr, re.M))}
        report['programs'][name] = {'trials': rows, 'medians': medians, 'collections': diag}
        dest.write_text(json.dumps(report, indent=2))
        print(name, medians, diag, flush=True)


def main():
    p = argparse.ArgumentParser()
    p.add_argument('mode', choices=['compile', 'measure'])
    p.add_argument('--hostdir', type=Path, required=True)
    p.add_argument('--arm', choices=['base', 'head'])
    p.add_argument('--programs', nargs='+', default=list(PROGRAMS))
    p.add_argument('--thp-off', action='store_true')
    a = p.parse_args()
    if a.mode == 'compile': compile_arm(a.hostdir, a.arm, a.programs)
    else: measure(a.hostdir, a.programs, a.thp_off)

if __name__ == '__main__': main()
