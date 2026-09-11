#!/usr/bin/env python3
"""Compare cached and rotating input parsing, including an input-selection control.

Eight equal-size documents are preloaded before timing. Rotating them defeats
Perry's single-source parse caches without charging file IO or string creation
to JSON.parse. This is a bounded corpus benchmark, not a fresh allocation on
every call. RSS includes all eight live inputs; never compare it directly to
the one-input worker's RSS. Raw selection overhead is reported, not subtracted.
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
import subprocess
import time

WORK = Path(__file__).resolve().parent
ROOT = WORK.parents[1]


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def run_one(command, prefix, operation, count, warmup, mode, verify=False):
    env = {key: value for key, value in os.environ.items() if not key.startswith('PERRY_')}
    argv = command + [str(prefix), operation, str(count), str(warmup),
                      'verify' if verify else 'time', mode]
    started = time.monotonic()
    process = subprocess.Popen(['/usr/bin/time', '-l'] + argv, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, env=env, start_new_session=True)
    try:
        stdout, stderr = process.communicate(timeout=90)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
        process.communicate()
        return dict(error='timeout', iterations=count)
    stdout, stderr = stdout.decode(), stderr.decode(errors='replace')
    row = dict(exit_code=process.returncode, process_wall_s=time.monotonic() - started,
               iterations=count, warmup=warmup, operation=operation, mode=mode)
    match = re.search(r'^RESULT (.+)$', stdout, re.M)
    if process.returncode != 0 or not match:
        return dict(row, error='process failed', stdout=stdout[:1000], stderr=stderr[-4000:])
    fields = ['wall_ms', 'user_us', 'system_us', 'rss_before', 'rss_after', 'checksum', 'retained']
    values = list(map(float, match.group(1).split()))
    if len(values) != len(fields):
        raise ValueError(f'Invalid result: {match.group(1)}')
    row.update(zip(fields, values))
    if not all(math.isfinite(value) for value in values):
        return dict(row, error='nonfinite result')
    peak = re.search(r'(\d+)\s+maximum resident set size', stderr)
    if peak:
        row['peak_rss'] = int(peak.group(1))
    if verify:
        outputs = re.findall(r'^VERIFY (\d+) (.*)$', stdout, re.M)
        row['verified_members'] = [int(index) for index, _ in outputs]
        row['verify_sha256'] = hashlib.sha256('\n'.join(text for _, text in outputs).encode()).hexdigest()
        last = re.search(r'^LAST (.*)$', stdout, re.M)
        row['last_sha256'] = hashlib.sha256(last.group(1).encode()).hexdigest() if last else None
    return row


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--worker', type=Path, required=True)
    parser.add_argument('--baseline-worker', type=Path, help='interleaved Perry reference arm')
    parser.add_argument('--node', default='node')
    parser.add_argument('--bun', default='bun')
    parser.add_argument('--results-dir', type=Path, required=True)
    parser.add_argument('--repeat', type=int, default=5)
    parser.add_argument('--filter', default='', help='comma-separated fixture names')
    parser.add_argument('--phase', choices=['verify', 'timing', 'all'], default='all')
    parser.add_argument('--modes', default='rotating,same,select')
    parser.add_argument('--source-commit', help='source revision when staging outside the checkout')
    args = parser.parse_args()
    # Refuse accidental trial mixing, including after an interrupted run.
    args.results_dir.mkdir(parents=True, exist_ok=False)
    fixtures = json.loads((ROOT / '.work/rotating/manifest.json').read_text())
    if args.filter:
        selected = set(args.filter.split(','))
        fixtures = [fixture for fixture in fixtures if fixture['fixture'] in selected]
        if {fixture['fixture'] for fixture in fixtures} != selected:
            parser.error('Unknown fixture in --filter')
    engines = {'perry': [str(args.worker.resolve())],
               'node': [args.node, str(WORK / 'harness/rotating-worker.js')],
               'bun': [args.bun, str(WORK / 'harness/rotating-worker.js')]}
    if args.baseline_worker:
        engines['baseline'] = [str(args.baseline_worker.resolve())]
    modes = args.modes.split(',')
    if set(modes) - {'rotating', 'same', 'select'}:
        parser.error('Unknown mode')
    metadata = dict(started_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
                    load_before=os.getloadavg(), host=os.uname().nodename,
                    cpu=subprocess.check_output(['sysctl', '-n', 'machdep.cpu.brand_string'], text=True).strip(),
                    memory=int(subprocess.check_output(['sysctl', '-n', 'hw.memsize'], text=True)),
                    worker_sha256=sha(args.worker), fixtures=fixtures,
                    baseline_worker_sha256=sha(args.baseline_worker) if args.baseline_worker else None,
                    engines=list(engines),
                    verification_counts_by_mode={'rotating': [2, 7, 9], 'same': [9], 'select': [9]},
                    source_sha256={str(path): sha(path) for path in
                                   [Path(__file__), WORK / 'harness/rotating-worker.ts', WORK / 'harness/rotating-worker.js']},
                    commit=args.source_commit or subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
                    versions={engine: subprocess.check_output([command[0], '--version'], text=True).strip()
                              for engine, command in engines.items() if engine in {'node', 'bun'}})
    meta_path = args.results_dir / 'host.json'
    meta_path.write_text(json.dumps(metadata, indent=2) + '\n')

    def save(phase, fixture, engine, row):
        row.update(engine=engine, fixture=fixture['fixture'], bytes=fixture['bytes'])
        with (args.results_dir / (phase + '.jsonl')).open('a') as output:
            output.write(json.dumps(row) + '\n')
        if 'error' in row:
            raise RuntimeError(f"{engine} {fixture['fixture']}: {row['error']}")

    rng = random.Random(10022)
    for fixture in fixtures:
        prefix = ROOT / '.work/rotating' / fixture['fixture']
        if args.phase in {'all', 'verify'}:
            for mode in modes:
                operation = 'select' if mode == 'select' else 'parse'
                # End the actual timed loop at multiple corpus indices. A
                # worker accidentally parsing sources[0] on every iteration
                # must fail even though its separate eight-member audit passes.
                for count in metadata['verification_counts_by_mode'][mode]:
                    results = {engine: run_one(command, prefix, operation, count, 8, mode, verify=True)
                               for engine, command in engines.items()}
                    reference = results['node']
                    for engine, row in results.items():
                        row['correct'] = ('error' not in row and 'error' not in reference
                                          and row['verified_members'] == list(range(8))
                                          and row['verify_sha256'] == reference['verify_sha256']
                                          and row['last_sha256'] == reference['last_sha256']
                                          and row['checksum'] == reference['checksum'])
                        save('verify', fixture, engine, row)
                        if not row['correct']:
                            raise RuntimeError(f"Incorrect output: {fixture['fixture']} {mode} {engine}")
                print('VERIFY', fixture['fixture'], mode, 'all engines passed', flush=True)
        if args.phase in {'all', 'timing'}:
            for mode in modes:
                operation = 'select' if mode == 'select' else 'parse'
                warmup = 5000 if fixture['bytes'] < 4096 else 8
                initial = 2000 if fixture['bytes'] < 4096 or mode == 'select' else 8
                rates = []
                for engine, command in engines.items():
                    row = run_one(command, prefix, operation, initial, warmup, mode)
                    save('calibration', fixture, engine, row)
                    rates.append(max(0.00001, (row['user_us'] + row['system_us']) / initial / 1000))
                count = max(8, min(2_000_000, int(min(150 / min(rates), 1500 / max(rates)))))
                for rep in range(args.repeat):
                    order = list(engines)
                    rng.shuffle(order)
                    for engine in order:
                        row = run_one(engines[engine], prefix, operation, count, warmup, mode)
                        row['rep'] = rep
                        save('timing', fixture, engine, row)
                print('TIMING', fixture['fixture'], mode, count, 'calls per engine', flush=True)
    metadata.update(load_after=os.getloadavg(), finished_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()))
    meta_path.write_text(json.dumps(metadata, indent=2) + '\n')


if __name__ == '__main__':
    main()
