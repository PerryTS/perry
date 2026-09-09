#!/usr/bin/env python3
"""Replicate the remaining R6 timing/RSS concerns with fixed work counts.

All timing arms are Perry binaries; Node is the output oracle. Run under the
same quiet/lock admission as the full matrix and preserve window.json.
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
from run_dispatch_focus import run, ROOT

CASES = [('long_string_1m', 'stringify', 32768, 8, 9),
         ('records_array_16k', 'roundtrip', 6185, 2, 25),
         ('records_array_16k', 'roundtrip', 20000, 8, 9)]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--worker', type=Path, required=True)
    parser.add_argument('--baseline-worker', type=Path, required=True)
    parser.add_argument('--prior-worker', type=Path)
    parser.add_argument('--node', default='node')
    parser.add_argument('--results-dir', type=Path, required=True)
    args = parser.parse_args()
    args.results_dir.mkdir(parents=True, exist_ok=False)
    engines = {'perry': [str(args.worker.resolve())],
               'baseline': [str(args.baseline_worker.resolve())]}
    if args.prior_worker:
        engines['prior'] = [str(args.prior_worker.resolve())]
    meta = dict(started_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
                load_before=os.getloadavg(), cases=CASES,
                workers={e: hashlib.sha256(Path(cmd[0]).read_bytes()).hexdigest()
                         for e, cmd in engines.items()},
                sources={p: hashlib.sha256((ROOT / p).read_bytes()).hexdigest()
                         for p in ['run_regression_focus.py', 'run_dispatch_focus.py', 'worker.js']},
                node_version=subprocess.check_output([args.node, '--version'], text=True).strip())
    def save(phase, row):
        with (args.results_dir / (phase + '.jsonl')).open('a') as out:
            out.write(json.dumps(row) + '\n')
    (args.results_dir / 'host.json').write_text(json.dumps(meta, indent=2) + '\n')
    rng = random.Random(1527)
    summaries = []
    for fixture, operation, count, warmup, repetitions in CASES:
        expected = run([args.node, str(ROOT / 'worker.js')], fixture, 7, 8, True, operation)
        expected.update(engine='node', operation=operation, case_iterations=count)
        save('verify', expected)
        for engine, cmd in engines.items():
            row = run(cmd, fixture, 7, 8, True, operation)
            row.update(engine=engine, operation=operation, case_iterations=count)
            row['correct'] = all(row[k] == expected[k] for k in ['verify_sha256', 'checksum', 'retained'])
            save('verify', row)
            if not row['correct']:
                raise RuntimeError(f'Incorrect output: {engine} {fixture} {operation}')
        rows = []
        unit = expected['checksum'] / 15
        for rep in range(repetitions):
            order = list(engines)
            rng.shuffle(order)
            for engine in order:
                row = run(engines[engine], fixture, count, warmup, operation=operation)
                row.update(engine=engine, operation=operation, rep=rep)
                assert row['checksum'] == unit * (count + warmup) and row['retained'] == 0
                save('timing', row)
                rows.append(row)
        for engine in engines:
            selected = [r for r in rows if r['engine'] == engine]
            assert len(selected) == repetitions
            samples = [(r['user_us'] + r['system_us']) / r['iterations'] for r in selected]
            peaks = [r['peak_rss'] / 1048576 for r in selected]
            summaries.append(dict(fixture=fixture, operation=operation, iterations=count,
                                  engine=engine, repetitions=repetitions,
                                  cpu_us=statistics.median(samples), cpu_samples_us=samples,
                                  peak_rss_mib=statistics.median(peaks), peak_rss_samples_mib=peaks))
        print('FINISHED', fixture, operation, count, flush=True)
    (args.results_dir / 'summary.json').write_text(json.dumps(summaries, indent=2) + '\n')
    meta.update(finished_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()), load_after=os.getloadavg())
    (args.results_dir / 'host.json').write_text(json.dumps(meta, indent=2) + '\n')


if __name__ == '__main__':
    main()
