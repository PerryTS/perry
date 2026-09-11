#!/usr/bin/env python3
"""Measure stringify argument/fallback paths with fixed work and output verification."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import random
import shutil
import statistics
import subprocess
import time
import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from run_dispatch_focus import run, ROOT
WORK = Path(__file__).resolve().parent

CASES = [('small_record','plain',2000000,5000,7), ('small_record','dynamic-zero',2000000,5000,7), ('small_record','zero',2000000,5000,7), ('small_record','pretty',1000000,5000,7), ('small_record','keys',1000000,5000,7), ('small_record','callback',200000,5000,7), ('records_array_16k','pretty',1000,8,7)]


def parse_case(value):
    """Accept an explicit fixture:operation:iterations:warmup:repetitions case."""
    try:
        fixture, operation, count, warmup, repetitions = value.split(':')
        count, warmup, repetitions = int(count), int(warmup), int(repetitions)
        if (not fixture or not all(c.isascii() and (c.isalnum() or c == '_') for c in fixture)
                or operation not in {'plain', 'zero', 'dynamic-zero', 'pretty', 'keys', 'callback'}
                or count < 1 or warmup < 0 or repetitions < 1):
            raise ValueError
        return fixture, operation, count, warmup, repetitions
    except ValueError:
        raise argparse.ArgumentTypeError('expected fixture:operation:positive-count:nonnegative-warmup:positive-repetitions') from None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--worker', type=Path, required=True)
    parser.add_argument('--baseline-worker', type=Path, required=True)
    parser.add_argument('--prior-worker', type=Path)
    parser.add_argument('--node', default='node')
    parser.add_argument('--bun', help='also time this Bun executable and --node')
    parser.add_argument('--results-dir', type=Path, required=True)
    parser.add_argument('--filter', default='', help='comma-separated fixtures')
    parser.add_argument('--case', type=parse_case, action='append', help='explicit case; repeat for multiple cases')
    args = parser.parse_args()
    if args.case and args.filter:
        parser.error('--case and --filter cannot be combined')
    cases = args.case or CASES
    if args.filter:
        selected = set(args.filter.split(','))
        available = CASES
        cases = [case for case in available if case[0] in selected]
        if {case[0] for case in cases} != selected:
            parser.error('Unknown fixture in --filter')
    args.results_dir.mkdir(parents=True, exist_ok=False)
    engines = {'perry': [str(args.worker.resolve())],
               'baseline': [str(args.baseline_worker.resolve())]}
    if args.prior_worker:
        engines['prior'] = [str(args.prior_worker.resolve())]
    if args.bun:
        for engine, executable in [('node', args.node), ('bun', args.bun)]:
            resolved = shutil.which(executable)
            if resolved is None:
                parser.error(f'{engine} executable not found: {executable}')
            engines[engine] = [str(Path(resolved).resolve()), str(WORK / 'options-worker.js')]
    meta = dict(started_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
                load_before=os.getloadavg(), cases=cases,
                workers={e: hashlib.sha256(Path(cmd[0]).read_bytes()).hexdigest()
                         for e, cmd in engines.items()},
                sources={str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in [Path(__file__),ROOT/'run_dispatch_focus.py',WORK/'options-worker.ts',WORK/'options-worker.js']},
                node_version=subprocess.check_output([args.node, '--version'], text=True).strip())
    if args.bun:
        meta['bun_version'] = subprocess.check_output([args.bun, '--version'], text=True).strip()
    def save(phase, row):
        with (args.results_dir / (phase + '.jsonl')).open('a') as out:
            out.write(json.dumps(row) + '\n')
    (args.results_dir / 'host.json').write_text(json.dumps(meta, indent=2) + '\n')
    rng = random.Random(1527)
    summaries = []
    for fixture, operation, count, warmup, repetitions in cases:
        expected = run([args.node, str(WORK / 'options-worker.js')], fixture, 7, 8, True, operation)
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
