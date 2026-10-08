#!/usr/bin/env python3
"""B5 instructions-only, interleaved A/B on qb6 CPUs 0-55, ASLR disabled.

Use separate main-target/target trees with identical archive features. Reuse
B4's output oracle and GC/full-count sampler; collect no cycles or wall time.
"""
import argparse
import json
from pathlib import Path
import statistics

from buffer_b4_validate import (PROGRAMS, KERNELS, compile_arm, environment,
                                full_collections, run)

DEFAULT = 'buffer_heavy,worker_heavy,tsc,zod5k,fastify,matmul,prime_sieve,bench_buffer_readwrite,ecs_u32'


def measure(root, names, runs, thp_off):
    folder = root / 'measure' / ('b5-thp-off' if thp_off else 'b5-trials')
    folder.mkdir(parents=True, exist_ok=True)
    status = {a: json.loads((root/'measure'/a/'status.json').read_text()) for a in ['main', 'head']}
    records = {}
    for name in names:
        if not all(status[a].get(name, {}).get('output_equal') for a in status):
            print(f'{name}: excluded; main/head output check failed', flush=True)
            continue
        args = [] if name in KERNELS else PROGRAMS[name][1]
        package = False if name in KERNELS else PROGRAMS[name][2]
        cwd = root/'drivers'/('pk' if package else '')
        trials = {'main': [], 'head': [], 'noise': []}
        for trial in range(runs):
            for arm in (['main', 'head'] if trial % 2 == 0 else ['head', 'main']):
                _, _, env = environment(root, arm)
                if thp_off: env['MIMALLOC_ALLOW_THP'] = '0'
                cmd = [str(root/'measure'/arm/name), *args]
                prefix = folder/f'{name}.{trial}.{arm}'
                stats_path, rss_path = Path(str(prefix)+'.stat'), Path(str(prefix)+'.rss')
                result = run(['perf', 'stat', '-x', ';', '-e', 'instructions:u', '-o', str(stats_path),
                              '/usr/bin/time', '-f', '%M', '-o', str(rss_path), *cmd], cwd, env, prefix)
                if result.returncode: raise RuntimeError(f'perf failed: {prefix}')
                instructions = next(int(l.split(';')[0]) for l in stats_path.read_text().splitlines()
                                    if ';instructions:u;' in l)
                stats = {'instructions:u': instructions, 'rss_kb': int(rss_path.read_text().strip())}
                stats.update(full_collections(root, arm, name, cmd, cwd, env, trial))
                trials[arm].append(stats)
                print(f'{name}/{trial}/{arm}: {stats}', flush=True)
                if arm == 'main':
                    prefix = folder/f'{name}.{trial}.noise'
                    counter, rss = Path(str(prefix)+'.stat'), Path(str(prefix)+'.rss')
                    control = run(['perf', 'stat', '-x', ';', '-e', 'instructions:u', '-o', str(counter),
                                   '/usr/bin/time', '-f', '%M', '-o', str(rss), *cmd], cwd, env, prefix)
                    if control.returncode: raise RuntimeError(f'noise control failed: {prefix}')
                    trials['noise'].append({'instructions:u': next(int(l.split(';')[0]) for l in counter.read_text().splitlines()
                                               if ';instructions:u;' in l), 'rss_kb': int(rss.read_text().strip())})
        def median_present(rows, key):
            values = [t[key] for t in rows if t[key] is not None]
            return statistics.median(values) if values else None
        medians = {a: {k: median_present(trials[a], k) for k in trials[a][0]} for a in ['main', 'head']}
        noise = max(abs(v['instructions:u']/t['instructions:u']-1) for v, t in zip(trials['noise'], trials['main']))*100
        records[name] = {'trials': trials, 'medians': medians, 'instruction_noise_floor_pct': noise,
                         'rss_noise_floor_kb': max(abs(v['rss_kb']-t['rss_kb']) for v, t in zip(trials['noise'], trials['main'])),
                         'instruction_delta_pct': (medians['head']['instructions:u']/medians['main']['instructions:u']-1)*100}
    path = root/'measure'/('b5-summary-thp-off.json' if thp_off else 'b5-summary.json')
    prior = json.loads(path.read_text()) if path.exists() else {}
    prior.update(records); path.write_text(json.dumps(prior, indent=2)+'\n')


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--hostdir', type=Path, required=True)
    ap.add_argument('--mode', choices=['compile-main', 'compile-head', 'measure'], required=True)
    ap.add_argument('--programs', default=DEFAULT)
    ap.add_argument('--runs', type=int, default=5)
    ap.add_argument('--thp-off', action='store_true')
    ap.add_argument('--compile-timeout', type=int, default=7200)
    ap.add_argument('--compile-jobs', type=int, choices=range(1, 9), default=4)
    args = ap.parse_args()
    (args.hostdir/'tmp').mkdir(parents=True, exist_ok=True)
    names = args.programs.split(',')
    if args.mode.startswith('compile-'):
        # Worker count changes scheduling only: keep the logical units and
        # optimization settings identical between arms. Four per arm lets the
        # full TypeScript package compile within an eight-worker lane budget.
        compile_arm(args.hostdir, args.mode.removeprefix('compile-'), names,
                    compile_timeout=args.compile_timeout, unit_jobs=args.compile_jobs)
    else: measure(args.hostdir, names, args.runs, args.thp_off)


if __name__ == '__main__':
    main()
