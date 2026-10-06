#!/usr/bin/env python3
"""B1 reproducible output checks and interleaved instruction/RSS measurements.

Builds are separate, external to this script. Compile under CPUs 0-55; invoke
measure through the qb6 measurement lock on CPUs 56-63 with ASLR disabled.
Every artifact is under --hostdir. GC diagnostics run separately so their JSON
formatting is not included in program instruction counts.
"""
import argparse
import json
import os
from pathlib import Path
import re
import statistics
import subprocess

PROGRAMS = {
    'tsc': ('tscwork.ts', ['1'], False),
    'zod5k': ('zodwork.ts', ['5000'], False),
    'qsparse': ('qs/parse_nested.ts', ['20000', '1000'], True),
    'qsstr': ('qs/stringify_nested.ts', ['20000', '1000'], True),
    'commander': ('commander/parse_argv.ts', ['5000', '200'], True),
    'hello': ('hello.ts', [], False),
    'fastify': ('fastify/inject.ts', ['500', '30'], True),
    'buffer_heavy': ('buffer_heavy.ts', [], False),
    'worker_heavy': ('worker_heavy.ts', [], False),
}
KERNELS = {
    'matmul': '16_matrix_multiply.ts',
    'prime_sieve': '11_prime_sieve.ts',
    'bench_buffer_readwrite': 'bench_buffer_readwrite.ts',
}
NODE = ['node', '--disable-warning=MODULE_TYPELESS_PACKAGE_JSON', '--experimental-strip-types']

def environment(root, arm):
    source = root / ('base/src' if arm == 'main' else 'src')
    target = root / ('base/target' if arm == 'main' else 'target')
    env = dict(os.environ, CARGO_TARGET_DIR=str(target), PERRY_RUNTIME_DIR=str(target/'release'),
               PERRY_WORKSPACE_ROOT=str(source), RUST_TEST_THREADS='1', CARGO_BUILD_JOBS='8',
               PERRY_NO_AUTO_OPTIMIZE='1', PERRY_NO_CACHE='1', PERRY_SKIP_BUILD='1',
               PERRY_ALLOW_PERRY_FEATURES='1', TMPDIR=str(root/'tmp'))
    env.pop('PERRY_GC_DIAG', None)
    return source, target, env

def run(cmd, cwd, env, prefix, timeout=1800):
    result = subprocess.run(cmd, cwd=cwd, env=env, capture_output=True, timeout=timeout)
    prefix.parent.mkdir(parents=True, exist_ok=True)
    Path(str(prefix)+'.out').write_bytes(result.stdout)
    Path(str(prefix)+'.err').write_bytes(result.stderr)
    return result

def normalize_kernel(data):
    return re.sub(rb'(?m)^(matrix_multiply|matmul|prime_sieve|buffer_readwrite):\d+', rb'\1:<time>', data)

def compile_arm(root, arm, names):
    source, target, env = environment(root, arm)
    status = {}
    for name in names:
        if name in KERNELS:
            relative, args, cwd = KERNELS[name], [], source/'benchmarks/suite'
        else:
            relative, args, package = PROGRAMS[name]
            cwd = root/'realprog'/('pk' if package else '')
        out = root/'measure'/arm
        compiled = run([str(target/'release/perry'), 'compile', relative, '-o', str(out/name)],
                       cwd, env, out/f'{name}.compile')
        if compiled.returncode:
            status[name] = {'compile': compiled.returncode}
            print(f'{arm}/{name}: compile failed', flush=True)
            continue
        node = run([*NODE, relative, *args], cwd, env, out/f'{name}.node')
        perry = run([str(out/name), *args], cwd, env, out/f'{name}.perry')
        norm = normalize_kernel if name in KERNELS else lambda b: b
        matches = node.returncode == 0 and perry.returncode == 0 and norm(node.stdout) == norm(perry.stdout)
        status[name] = {'node': node.returncode, 'perry': perry.returncode, 'output_equal': matches}
        print(f'{arm}/{name}: {status[name]}', flush=True)
    path = root/'measure'/arm/'status.json'
    prior = json.loads(path.read_text()) if path.exists() else {}
    prior.update(status)
    path.write_text(json.dumps(prior, indent=2)+'\n')

def full_collections(root, arm, name, cmd, cwd, env, trial):
    diag = dict(env, PERRY_GC_DIAG='1')
    prefix = root/'measure/trials'/f'{name}.{trial}.{arm}.gc'
    result = run(cmd, cwd, diag, prefix)
    if result.returncode:
        raise RuntimeError(f'GC diagnostic run failed: {prefix}')
    events = []
    for line in result.stderr.decode(errors='replace').splitlines():
        if line.startswith('{'):
            try:
                obj = json.loads(line)
                if obj.get('event') == 'gc_cycle': events.append(obj)
            except ValueError: pass
    if not events and b'diagnostics feature disabled' in result.stderr:
        return None
    return sum(e['collection_kind'] == 'full' for e in events)

def measure(root, names, thp_off):
    status = {a: json.loads((root/'measure'/a/'status.json').read_text()) for a in ['main', 'head']}
    records = {}
    for name in names:
        if not all(status[a].get(name, {}).get('output_equal') for a in status):
            print(f'{name}: excluded; baseline/head output check failed', flush=True)
            continue
        args = [] if name in KERNELS else PROGRAMS[name][1]
        package = False if name in KERNELS else PROGRAMS[name][2]
        cwd = root/'realprog'/('pk' if package else '')
        trials = {'main': [], 'head': [], 'noise': []}
        for trial in range(5):
            for arm in (['main', 'head'] if trial % 2 == 0 else ['head', 'main']):
                _, _, env = environment(root, arm)
                if thp_off: env['MIMALLOC_ALLOW_THP'] = '0'
                cmd = [str(root/'measure'/arm/name), *args]
                prefix = root/'measure/trials'/f'{name}.{trial}.{arm}'
                prefix.parent.mkdir(parents=True, exist_ok=True)
                counter, rss = Path(str(prefix)+'.stat'), Path(str(prefix)+'.rss')
                result = run(['perf', 'stat', '-x', ';', '-e', 'instructions:u,cycles:u', '-o', str(counter),
                              '/usr/bin/time', '-f', '%M', '-o', str(rss), *cmd], cwd, env, prefix)
                if result.returncode: raise RuntimeError(f'perf failed: {prefix}')
                stats = {}
                for line in counter.read_text().splitlines():
                    c = line.split(';')
                    if len(c)>2 and c[2] in ['instructions:u', 'cycles:u']:
                        stats[c[2]] = int(c[0])
                stats['rss_kb'] = int(rss.read_text().strip())
                stats['fulls'] = full_collections(root, arm, name, cmd, cwd, env, trial)
                trials[arm].append(stats)
                print(f'{name}/{trial}/{arm}: {stats}', flush=True)
                if arm == 'main':
                    # Identical binary control, interleaved with each A/B pair.
                    prefix = root/'measure/trials'/f'{name}.{trial}.noise'
                    noise_counter = Path(str(prefix)+'.stat')
                    control = run(['perf', 'stat', '-x', ';', '-e', 'instructions:u', '-o', str(noise_counter), *cmd],
                                  cwd, env, prefix)
                    if control.returncode: raise RuntimeError(f'noise control failed: {prefix}')
                    instructions = next(int(l.split(';')[0]) for l in noise_counter.read_text().splitlines()
                                        if ';instructions:u;' in l)
                    trials['noise'].append(instructions)
        medians = {a: {k: statistics.median(t[k] for t in trials[a]) for k in trials[a][0]}
                   for a in ['main','head']}
        noise = max(abs(v/t['instructions:u']-1) for v,t in zip(trials['noise'], trials['main']))*100
        records[name] = {'trials': trials, 'medians': medians, 'instruction_noise_floor_pct': noise,
                         'instruction_delta_pct': (medians['head']['instructions:u']/medians['main']['instructions:u']-1)*100}
    path = root/'measure'/('summary-thp-off.json' if thp_off else 'summary.json')
    prior = json.loads(path.read_text()) if path.exists() else {}
    prior.update(records)
    path.write_text(json.dumps(prior,indent=2)+'\n')

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--hostdir', type=Path, required=True)
    ap.add_argument('--mode', choices=['compile-main','compile-head','measure'], required=True)
    ap.add_argument('--programs', default=','.join([*PROGRAMS,*KERNELS]))
    ap.add_argument('--thp-off', action='store_true')
    args=ap.parse_args(); root=args.hostdir
    (root/'tmp').mkdir(parents=True,exist_ok=True)
    names=args.programs.split(',')
    if args.mode.startswith('compile-'): compile_arm(root,args.mode.removeprefix('compile-'),names)
    else: measure(root,names,args.thp_off)
if __name__=='__main__': main()
