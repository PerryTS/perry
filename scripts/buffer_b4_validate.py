#!/usr/bin/env python3
"""B2c/B4 reproducible output checks and interleaved instruction/RSS measurements.

Builds are separate, external to this script. Compile under CPUs 0-55; invoke
measure through the qb6 measurement lock on CPUs 56-63 with ASLR disabled.
Every artifact is under --hostdir. GC diagnostics run separately so their JSON
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
    'effect': ('effectwork.ts', [], False),
    'buffer_heavy': ('buffer_heavy.ts', [], False),
    'worker_heavy': ('worker_heavy.ts', [], False),
}
KERNELS = {
    'matmul': 'matmul.ts',
    'prime_sieve': 'prime_sieve.ts',
    'bench_buffer_readwrite': 'bench_buffer_readwrite.ts',
    'ecs_u32': 'ecs_u32.ts',
}
NODE = ['node', '--disable-warning=MODULE_TYPELESS_PACKAGE_JSON', '--experimental-strip-types']

def disable_thp():
    if ctypes.CDLL(None, use_errno=True).prctl(41, 1, 0, 0, 0) != 0:
        raise OSError(ctypes.get_errno(), 'PR_SET_THP_DISABLE failed')

def environment(root, arm):
    source = root / ('main-src' if arm == 'main' else 'src')
    target = root / ('main-target' if arm == 'main' else 'target')
    # The no-auto HTTP path rebuilds whenever it finds crate source, even
    # when the requested pump features are already in the prebuilt archives.
    # Use a source-free workspace marker for compiler invocations so both
    # arms consume exactly the coherent archives built outside this script.
    prebuilt = root / ('prebuilt-' + arm)
    for crate in ['perry-runtime', 'perry-ui-geisterhand']:
        (prebuilt / 'crates' / crate).mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, CARGO_TARGET_DIR=str(target), PERRY_RUNTIME_DIR=str(target/'release'),
               PERRY_WORKSPACE_ROOT=str(prebuilt), RUST_TEST_THREADS='1', CARGO_BUILD_JOBS='8',
               PERRY_NO_AUTO_OPTIMIZE='1', PERRY_NO_CACHE='1', PERRY_SKIP_BUILD='1',
               PERRY_ALLOW_PERRY_FEATURES='1', TMPDIR=str(root/'tmp'), RAYON_NUM_THREADS=os.environ.get('PERRY_VERIFY_RAYON_THREADS', '8'),
               PERRY_MODULE_JOBS='1', PERRY_CODEGEN_UNIT_JOBS='1',
               PERRY_FORCE_WELL_KNOWN='http,net,ws,zlib',
               PERF_BUILDID_DIR=str(root/'perf-buildid'), XDG_CACHE_HOME=str(root/'cache'))
    env.pop('PERRY_GC_DIAG', None)
    env.pop('PERRY_GC_TRACE', None)
    return source, target, env

def run(cmd, cwd, env, prefix, timeout=1800):
    prefix.parent.mkdir(parents=True, exist_ok=True)
    try:
        result = subprocess.run(cmd, cwd=cwd, env=env, capture_output=True, timeout=timeout,
                                preexec_fn=disable_thp if env.get('MIMALLOC_ALLOW_THP') == '0' else None)
    except subprocess.TimeoutExpired as error:
        result = subprocess.CompletedProcess(cmd, 124, error.stdout or b'', error.stderr or b'')
    Path(str(prefix)+'.out').write_bytes(result.stdout)
    Path(str(prefix)+'.err').write_bytes(result.stderr)
    return result

def normalize_kernel(data):
    return re.sub(rb'(?m)^(matrix_multiply|matmul|prime_sieve|buffer_readwrite):\d+', rb'\1:<time>', data)

def normalize_effect(data):
    return re.sub(rb'(construct2000|decode20000)=\d+ms', rb'\1=<time>ms', data)

def compile_arm(root, arm, names, *, compile_timeout=1800, unit_jobs=1):
    source, target, env = environment(root, arm)
    env['PERRY_CODEGEN_UNIT_JOBS'] = str(unit_jobs)
    status = {}
    for name in names:
        if name in KERNELS:
            relative, args, cwd = KERNELS[name], [], root/'drivers/kernels'
        else:
            relative, args, package = PROGRAMS[name]
            cwd = root/'drivers'/('pk' if package else '')
        out = root/'measure'/arm
        compiled = run([str(target/'release/perry'), 'compile', relative, '-o', str(out/name)],
                       cwd, env, out/f'{name}.compile', timeout=compile_timeout)
        if compiled.returncode:
            status[name] = {'compile': compiled.returncode}
            print(f'{arm}/{name}: compile failed', flush=True)
            continue
        node = run([*NODE, relative, *args], cwd, env, out/f'{name}.node', timeout=60)
        perry = run([str(out/name), *args], cwd, env, out/f'{name}.perry', timeout=60)
        norm = normalize_kernel if name in KERNELS else normalize_effect if name == 'effect' else lambda b: b
        matches = node.returncode == 0 and perry.returncode == 0 and norm(node.stdout) == norm(perry.stdout)
        status[name] = {'node': node.returncode, 'perry': perry.returncode, 'output_equal': matches}
        print(f'{arm}/{name}: {status[name]}', flush=True)
        path = out/'status.json'
        prior = json.loads(path.read_text()) if path.exists() else {}
        prior.update(status); path.write_text(json.dumps(prior, indent=2)+'\n')
    path = root/'measure'/arm/'status.json'
    prior = json.loads(path.read_text()) if path.exists() else {}
    prior.update(status)
    path.write_text(json.dumps(prior, indent=2)+'\n')

def full_collections(root, arm, name, cmd, cwd, env, trial):
    diag = dict(env, PERRY_GC_TRACE='1')
    trials_dir = 'trials-thp-off' if env.get('MIMALLOC_ALLOW_THP') == '0' else 'trials'
    prefix = root/'measure'/trials_dir/f'{name}.{trial}.{arm}.gc'
    prefix.parent.mkdir(parents=True, exist_ok=True)
    huge = []
    peak = {}
    stop = threading.Event()
    proc = subprocess.Popen(cmd, cwd=cwd, env=diag, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            preexec_fn=disable_thp if env.get('MIMALLOC_ALLOW_THP') == '0' else None)
    def sample():
        while not stop.is_set():
            try:
                data = Path(f'/proc/{proc.pid}/smaps_rollup').read_text()
                match = re.search(r'(?m)^AnonHugePages:\s+(\d+)', data)
                if match: huge.append(int(match[1]))
                fields = {k: int(v) for k, v in re.findall(r'(?m)^(Rss|Anonymous):\s+(\d+)', data)}
                if fields.get('Rss', 0) > peak.get('Rss', 0):
                    peak.update(fields)
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
    events = []
    for line in stderr.decode(errors='replace').splitlines():
        if line.startswith('{'):
            try:
                obj = json.loads(line)
                if obj.get('event') == 'gc_cycle': events.append(obj)
            except ValueError: pass
    if b'diagnostics feature disabled' in stderr:
        raise RuntimeError(f'GC trace feature is disabled: {prefix}')
    if any(e['collection_kind'] not in ['full', 'minor'] for e in events):
        raise RuntimeError(f'Unrecognized GC collection kind: {prefix}')
    fulls = sum(e['collection_kind'] == 'full' for e in events)
    minors = sum(e['collection_kind'] == 'minor' for e in events)
    return {'fulls': fulls, 'minors': minors, 'anon_huge_kb': max(huge) if huge else None,
            'sampled_anon_rss_kb': peak.get('Anonymous'),
            'sampled_file_rss_kb': peak['Rss'] - peak['Anonymous'] if 'Anonymous' in peak else None}

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
        cwd = root/'drivers'/('pk' if package else '')
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
