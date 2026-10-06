#!/usr/bin/env python3
"""B1 reproducible output checks and interleaved instruction/RSS measurements.

Builds are separate, external to this script. Compile under CPUs 0-55; invoke
measure through the qb6 measurement lock on CPUs 56-63 with ASLR disabled.
Every artifact is under --hostdir. GC diagnostics run separately so their diagnostic
formatting is not included in program instruction counts.
"""
import argparse
import ctypes
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import threading

PROGRAMS = {
    'tsc': ('tscwork.ts', ['1'], False),
    'zod5k': ('zodwork.ts', ['5000'], False),
    'qsparse': ('qs/parse_nested.ts', ['20000', '1000'], True),
    'qsstr': ('qs/stringify_nested.ts', ['20000', '1000'], True),
    'commander': ('commander/parse_argv.ts', ['5000', '200'], True),
    'hello': ('hello.ts', [], False),
    'fastify': ('fastify/inject.ts', ['500', '30'], True),
    'effect': ('effect/main.ts', [], False),
    'buffer_heavy': ('buffer_heavy.ts', [], False),
    'worker_heavy': ('worker_heavy.ts', [], False),
}
KERNELS = {
    'matmul': 'matmul.ts',
    'prime_sieve': 'prime_sieve.ts',
    'bench_buffer_readwrite': 'bench_buffer_readwrite.ts',
}
NODE = ['node', '--disable-warning=MODULE_TYPELESS_PACKAGE_JSON', '--experimental-strip-types']

def disable_thp():
    # PR_SET_THP_DISABLE applies to every mapping, including arena mmap.
    # The mimalloc option alone does not cover those mappings.
    if ctypes.CDLL(None, use_errno=True).prctl(41, 1, 0, 0, 0) != 0:
        raise OSError(ctypes.get_errno(), 'PR_SET_THP_DISABLE failed')

def environment(root, arm):
    source = root / ('base/src' if arm == 'main' else 'src')
    target = root / ('base/target' if arm == 'main' else 'target')
    env = dict(os.environ, CARGO_TARGET_DIR=str(target), PERRY_RUNTIME_DIR=str(target/'release'),
               PERRY_WORKSPACE_ROOT=str(source), RUST_TEST_THREADS='1', CARGO_BUILD_JOBS='8',
               PERRY_NO_AUTO_OPTIMIZE='1', PERRY_NO_CACHE='1', PERRY_SKIP_BUILD='1',
               PERRY_ALLOW_PERRY_FEATURES='1', PERRY_KEEP_SYMBOLS='1',
               TMPDIR=str(root/'tmp'), RAYON_NUM_THREADS='8',
               PERRY_FORCE_WELL_KNOWN='http,net,ws,zlib',
               PERF_BUILDID_DIR=str(root/'perf-buildid'), XDG_CACHE_HOME=str(root/'cache'))
    env.pop('PERRY_GC_DIAG', None)
    return source, target, env

def run(cmd, cwd, env, prefix, timeout=1800):
    prefix.parent.mkdir(parents=True, exist_ok=True)
    stdout_path, stderr_path = Path(str(prefix)+'.out'), Path(str(prefix)+'.err')
    with stdout_path.open('wb') as stdout, stderr_path.open('wb') as stderr:
        result = subprocess.run(cmd, cwd=cwd, env=env, stdout=stdout, stderr=stderr, timeout=timeout,
                                preexec_fn=disable_thp if env.get('MIMALLOC_ALLOW_THP') == '0' else None)
    return subprocess.CompletedProcess(cmd, result.returncode, stdout_path.read_bytes(), stderr_path.read_bytes())

def normalize_kernel(data):
    return re.sub(rb'(?m)^(matrix_multiply|matmul|prime_sieve|buffer_readwrite):\d+', rb'\1:<time>', data)

def normalize_effect(data):
    return re.sub(rb'(construct2000|decode20000)=\d+ms', rb'\1=<time>ms', data)

def gc_counts(stderr):
    text = stderr.decode(errors='replace')
    if '[gc-incremental]' not in text:
        raise RuntimeError('GC diagnostics missing; refusing to report a guessed zero')
    # diag_sites.rs emits one gc-full line at the synchronous chokepoint,
    # and one budgeted done line for each completed incremental cycle.
    synchronous = len(re.findall(r'(?m)^\[gc-full\] ', text))
    budgeted = len(re.findall(r'(?m)^\[gc-budgeted\] done [^\n]*\bkind=full\b', text))
    copying = sum(map(int, re.findall(r'(?m)^\[gc-incremental\] [^\n]*\bcopying_minors=(\d+)', text)))
    return {'fulls': synchronous+budgeted, 'copying_minors': copying}

def compile_arm(root, arm, names):
    source, target, env = environment(root, arm)
    path = root/'measure'/arm/'status.json'
    path.parent.mkdir(parents=True, exist_ok=True)
    status = json.loads(path.read_text()) if path.exists() else {}
    status.update({name: {'pending': True} for name in names})
    path.write_text(json.dumps(status, indent=2)+'\n')
    for name in names:
        if name in KERNELS:
            relative, args, cwd = KERNELS[name], [], root/'realprog/kernels'
        else:
            relative, args, package = PROGRAMS[name]
            cwd = root/'realprog'/('pk' if package else '')
        out = root/'measure'/arm
        try:
            compiled = run([str(target/'release/perry'), 'compile', relative, '-o', str(out/name)],
                           cwd, env, out/f'{name}.compile', timeout=2700)
        except subprocess.TimeoutExpired:
            status[name] = {'compile_timeout': 2700}
            path.write_text(json.dumps(status, indent=2)+'\n')
            print(f'{arm}/{name}: compile timed out', flush=True)
            continue
        if compiled.returncode:
            status[name] = {'compile': compiled.returncode}
            path.write_text(json.dumps(status, indent=2)+'\n')
            print(f'{arm}/{name}: compile failed', flush=True)
            continue
        node = run([*NODE, relative, *args], cwd, env, out/f'{name}.node')
        perry = run([str(out/name), *args], cwd, env, out/f'{name}.perry')
        norm = normalize_kernel if name in KERNELS else normalize_effect if name == 'effect' else lambda b: b
        matches = node.returncode == 0 and perry.returncode == 0 and norm(node.stdout) == norm(perry.stdout)
        status[name] = {'node': node.returncode, 'perry': perry.returncode, 'output_equal': matches}
        path.write_text(json.dumps(status, indent=2)+'\n')
        print(f'{arm}/{name}: {status[name]}', flush=True)

def full_collections(root, arm, name, cmd, cwd, env, trial):
    diag = dict(env, PERRY_GC_DIAG='1')
    trials_dir = 'trials-thp-off' if env.get('MIMALLOC_ALLOW_THP') == '0' else 'trials'
    prefix = root/'measure'/trials_dir/f'{name}.{trial}.{arm}.gc'
    samples = []
    stop = threading.Event()
    proc = subprocess.Popen(cmd, cwd=cwd, env=diag, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            preexec_fn=disable_thp if env.get('MIMALLOC_ALLOW_THP') == '0' else None)
    def sample():
        while not stop.is_set():
            try:
                data = Path(f'/proc/{proc.pid}/smaps_rollup').read_text()
                fields = {k: int(v) for k, v in re.findall(r'(?m)^(Rss|Anonymous|AnonHugePages):\s+(\d+)', data)}
                if len(fields) == 3: samples.append(fields)
            except (OSError, ProcessLookupError): pass
            stop.wait(0.02)
    sampler = threading.Thread(target=sample); sampler.start()
    try:
        stdout, stderr = proc.communicate(timeout=1800)
    except subprocess.TimeoutExpired:
        proc.kill(); proc.communicate(); raise
    finally:
        stop.set(); sampler.join()
    Path(str(prefix)+'.out').write_bytes(stdout)
    Path(str(prefix)+'.err').write_bytes(stderr)
    if proc.returncode:
        raise RuntimeError(f'GC diagnostic run failed: {prefix}')
    return {**gc_counts(stderr),
            'anon_huge_kb': max(s['AnonHugePages'] for s in samples) if samples else None,
            'diag_anon_kb': max(s['Anonymous'] for s in samples) if samples else None,
            'diag_file_rss_kb': max(s['Rss']-s['Anonymous'] for s in samples) if samples else None}

def measure(root, names, thp_off):
    trials_dir = 'trials-thp-off' if thp_off else 'trials'
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
                prefix = root/'measure'/trials_dir/f'{name}.{trial}.{arm}'
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
                stats.update(full_collections(root, arm, name, cmd, cwd, env, trial))
                trials[arm].append(stats)
                print(f'{name}/{trial}/{arm}: {stats}', flush=True)
                if arm == 'main':
                    # Identical binary control, interleaved with each A/B pair.
                    prefix = root/'measure'/trials_dir/f'{name}.{trial}.noise'
                    noise_counter = Path(str(prefix)+'.stat')
                    control = run(['perf', 'stat', '-x', ';', '-e', 'instructions:u', '-o', str(noise_counter), '/usr/bin/time', '-f', '%M', '-o', str(Path(str(prefix)+'.rss')), *cmd],
                                  cwd, env, prefix)
                    if control.returncode: raise RuntimeError(f'noise control failed: {prefix}')
                    instructions = next(int(l.split(';')[0]) for l in noise_counter.read_text().splitlines()
                                        if ';instructions:u;' in l)
                    trials['noise'].append({'instructions:u': instructions,
                                            'rss_kb': int(Path(str(prefix)+'.rss').read_text().strip())})
        def median_present(trials, key):
            values = [t[key] for t in trials if t[key] is not None]
            return statistics.median(values) if values else None
        medians = {a: {k: median_present(trials[a], k) for k in trials[a][0]}
                   for a in ['main','head']}
        noise = max(abs(v['instructions:u']/t['instructions:u']-1) for v,t in zip(trials['noise'], trials['main']))*100
        rss_noise = max(abs(v['rss_kb']-t['rss_kb']) for v,t in zip(trials['noise'], trials['main']))
        records[name] = {'trials': trials, 'medians': medians, 'instruction_noise_floor_pct': noise,
                         'rss_noise_floor_kb': rss_noise,
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
