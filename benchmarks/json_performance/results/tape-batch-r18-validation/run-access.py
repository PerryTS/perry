#!/usr/bin/env python3
"""Measure reads after one JSON.parse, including cache and random-access controls.

Run under with_lock.py on the quiet benchmark host. Fresh processes, identical
work counts, interleaved engines, and a Node checksum for each complete workload.
Parsing is outside the timed region; these rows supplement parse/scan timings.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import random
import statistics
import subprocess
import time
import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from run_dispatch_focus import run, ROOT
ACCESS = Path(__file__).resolve().parent / 'harness'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--worker', type=Path, required=True)
    parser.add_argument('--baseline-worker', type=Path, required=True)
    parser.add_argument('--node', required=True)
    parser.add_argument('--bun', required=True)
    parser.add_argument('--results-dir', type=Path, required=True)
    parser.add_argument('--iterations', type=int, default=1_000_000)
    parser.add_argument('--repeat', type=int, default=7)
    args = parser.parse_args()
    if args.iterations < 1 or args.repeat < 1:
        parser.error('iterations and repeat must be positive')
    args.results_dir.mkdir(parents=True, exist_ok=False)
    engines = {
        'perry': [str(args.worker.resolve())],
        'baseline': [str(args.baseline_worker.resolve())],
        'node': [args.node, str(ACCESS / 'access-worker.js')],
        'bun': [args.bun, str(ACCESS / 'access-worker.js')],
    }
    fixtures = ['records_array_16k', 'records_array_1m', 'records_array_20m']
    paths = [args.worker.resolve(), args.baseline_worker.resolve(),
             ACCESS / 'access-worker.ts', ACCESS / 'access-worker.js',
             Path(__file__), ROOT / 'run_dispatch_focus.py']
    paths += [ROOT / '.work/fixtures' / (name + '.json') for name in fixtures]
    meta = dict(started_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
                load_before=os.getloadavg(), iterations=args.iterations,
                repeat=args.repeat, warmup=0, commands=engines,
                hashes={str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths},
                versions={e: subprocess.check_output([engines[e][0], '--version'], text=True).strip()
                          for e in ['node', 'bun']})
    def save(phase, row):
        with (args.results_dir / (phase + '.jsonl')).open('a') as out:
            out.write(json.dumps(row) + '\n')
    (args.results_dir / 'host.json').write_text(json.dumps(meta, indent=2) + '\n')
    rng = random.Random(10038)
    summaries = []
    for fixture in fixtures:
        for operation in ['repeat', 'random', 'fields', 'sequential']:
            expected = run(engines['node'], fixture, args.iterations, 0, operation=operation)
            save('verify', dict(expected, engine='node', operation=operation))
            rows = []
            for rep in range(args.repeat):
                order = list(engines)
                rng.shuffle(order)
                for engine in order:
                    row = run(engines[engine], fixture, args.iterations, 0, operation=operation)
                    row.update(engine=engine, operation=operation, rep=rep)
                    row['correct'] = row['checksum'] == expected['checksum'] and row['retained'] == 0
                    save('timing', row)
                    if not row['correct']:
                        raise RuntimeError(f'Incorrect output: {engine} {fixture} {operation}')
                    rows.append(row)
            for engine in engines:
                chosen = [r for r in rows if r['engine'] == engine]
                cpu = [(r['user_us'] + r['system_us']) / args.iterations for r in chosen]
                rss = [r['peak_rss'] / 1048576 for r in chosen]
                summaries.append(dict(fixture=fixture, operation=operation, engine=engine,
                                      cpu_us=statistics.median(cpu), cpu_samples_us=cpu,
                                      peak_rss_mib=statistics.median(rss), peak_rss_samples_mib=rss))
            print('FINISHED', fixture, operation, flush=True)
    (args.results_dir / 'summary.json').write_text(json.dumps(summaries, indent=2) + '\n')
    meta.update(finished_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
                load_after=os.getloadavg())
    (args.results_dir / 'host.json').write_text(json.dumps(meta, indent=2) + '\n')


if __name__ == '__main__':
    main()
