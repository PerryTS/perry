#!/usr/bin/env python3
"""Longer, interleaved stringify trials for resolving small dispatch differences.

Node supplies the output oracle. The timing arms are Perry candidates/reference,
with identical call counts and fresh processes. This supplements the full
Node/Bun comparison; it does not replace it. Run under the host quiet/lock gate.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import random
import re
import signal
import statistics
import subprocess
import time

ROOT = Path(__file__).resolve().parent


def run(command, fixture, count, warmup, verify=False, operation='stringify'):
    env = {k: v for k, v in os.environ.items() if not k.startswith('PERRY_')}
    argv = command + [str(ROOT / '.work/fixtures' / (fixture + '.json')),
                      operation, str(count), str(warmup)]
    if verify:
        argv.append('verify')
    process = subprocess.Popen(['/usr/bin/time', '-l'] + argv, env=env,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               start_new_session=True)
    try:
        stdout, stderr = process.communicate(timeout=90)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
        process.communicate()
        raise RuntimeError(f'Timeout: {argv}')
    text, diagnostics = stdout.decode(), stderr.decode(errors='replace')
    if process.returncode != 0:
        raise RuntimeError(f'Exit {process.returncode}: {argv}\n{diagnostics[-4000:]}')
    match = re.search(r'^RESULT (.+)$', text, re.M)
    if not match:
        raise ValueError('Missing RESULT')
    values = list(map(float, match.group(1).split()))
    if len(values) != 7 or not all(math.isfinite(v) for v in values):
        raise ValueError('Invalid RESULT')
    row = dict(zip(['wall_ms', 'user_us', 'system_us', 'rss_before',
                    'rss_after', 'checksum', 'retained'], values))
    row.update(fixture=fixture, iterations=count, warmup=warmup)
    for label, key in [('maximum resident set size', 'peak_rss'),
                       ('instructions retired', 'instructions'), ('cycles elapsed', 'cycles')]:
        match = re.search(r'(\d+)\s+' + label, diagnostics)
        if match:
            row[key] = int(match.group(1))
    if verify:
        if '\nVERIFY ' not in text or '\nKEEP ' not in text:
            raise ValueError('Missing verification')
        result = text.split('\nVERIFY ', 1)[1].rsplit('\nKEEP ', 1)[0]
        row['verify_sha256'] = hashlib.sha256(result.encode()).hexdigest()
    return row


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--worker', type=Path, required=True)
    parser.add_argument('--baseline-worker', type=Path, required=True)
    parser.add_argument('--prior-worker', type=Path)
    parser.add_argument('--node', default='node')
    parser.add_argument('--repeat', type=int, default=9)
    parser.add_argument('--results-dir', type=Path, required=True)
    args = parser.parse_args()
    if args.repeat < 1:
        parser.error('--repeat must be at least 1')
    args.results_dir.mkdir(parents=True, exist_ok=False)
    engines = {'perry': [str(args.worker.resolve())],
               'baseline': [str(args.baseline_worker.resolve())]}
    if args.prior_worker:
        engines['prior'] = [str(args.prior_worker.resolve())]
    oracle = [args.node, str(ROOT / 'worker.js')]
    fixtures = {'string_a': 50_000_000, 'small_record': 50_000_000,
                'unicode_1m': 16_384}
    meta = dict(started_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
                load_before=os.getloadavg(), fixtures=fixtures, repeat=args.repeat,
                worker_sha256={e: hashlib.sha256(Path(cmd[0]).read_bytes()).hexdigest()
                               for e, cmd in engines.items()},
                node_version=subprocess.check_output([args.node, '--version'], text=True).strip(),
                harness_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                worker_source_sha256=hashlib.sha256((ROOT / 'worker.js').read_bytes()).hexdigest())
    (args.results_dir / 'host.json').write_text(json.dumps(meta, indent=2) + '\n')
    def save(phase, row):
        with (args.results_dir / (phase + '.jsonl')).open('a') as out:
            out.write(json.dumps(row) + '\n')
    rng = random.Random(10022)
    rows = []
    for fixture, count in fixtures.items():
        expected = run(oracle, fixture, 7, 8, True)
        expected['engine'] = 'node'
        save('verify', expected)
        for engine, cmd in engines.items():
            row = run(cmd, fixture, 7, 8, True)
            row['engine'] = engine
            row['correct'] = all(row[key] == expected[key]
                                 for key in ['verify_sha256', 'checksum', 'retained'])
            save('verify', row)
            if not row['correct']:
                raise RuntimeError(f'Incorrect output: {engine} {fixture}')
        warmup = 5000 if fixture != 'unicode_1m' else 8
        unit = expected['checksum'] / 15
        for rep in range(args.repeat):
            order = list(engines)
            rng.shuffle(order)
            for engine in order:
                row = run(engines[engine], fixture, count, warmup)
                row.update(engine=engine, rep=rep)
                assert row['checksum'] == unit * (count + warmup) and row['retained'] == 0
                save('timing', row)
                rows.append(row)
        print('FINISHED', fixture, count, 'calls per trial', flush=True)
    summary = []
    for fixture in fixtures:
        for engine in engines:
            selected = [r for r in rows if r['fixture'] == fixture and r['engine'] == engine]
            assert len(selected) == args.repeat
            samples = [(r['user_us'] + r['system_us']) / r['iterations'] for r in selected]
            summary.append(dict(fixture=fixture, engine=engine, cpu_us=statistics.median(samples),
                                cpu_samples_us=samples,
                                peak_rss_mib=statistics.median(r['peak_rss'] / 1048576 for r in selected)))
    (args.results_dir / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    meta.update(finished_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()), load_after=os.getloadavg())
    (args.results_dir / 'host.json').write_text(json.dumps(meta, indent=2) + '\n')


if __name__ == '__main__':
    main()
