#!/usr/bin/env python3
"""Supplemental paired API latency, whole-loop CPU, cleanup CPU and RSS.

Run from a staged benchmark directory through with_lock.py. Unlike worker.ts,
this worker charges per-call clocks and a final explicit full collection. Its
numbers are paired only against the same worker linked to the parent runtime.
"""
from pathlib import Path
import hashlib
import json
import os
import random
import re
import subprocess

ROOT = Path(__file__).resolve().parent
OUT = ROOT / 'results/lifetimes'
OUT.mkdir()
ENGINES = {name: ROOT / '.work' / ('worker-lifetime-' + name)
           for name in ['parent', 'candidate']}
COUNTS = {
    'small_record': (100_000, 100_000),
    'object_1k': (60_000, 20_000),
    'records_object_1m': (120, 16),
    'wide_1m': (60, 8),
    'records_object_8m': (20, 2),
    'records_object_20m': (8, 2),
}
(OUT / 'metadata.json').write_text(json.dumps({
    'workers': {key: hashlib.sha256(value.read_bytes()).hexdigest()
                for key, value in ENGINES.items()},
    'counts': COUNTS, 'repeats': 3, 'load_before': os.getloadavg(),
    'note': 'API clocks are included in loop CPU. Explicit cleanup CPU is charged separately and in totalCpuUs. Default GC; no trace instrumentation in timed pairs.',
}, indent=2) + '\n')


def one(engine, fixture, mode, operation, count, trace=False):
    command = [str(ENGINES[engine]), str(ROOT / '.work/fixtures' / (fixture + '.json')),
               mode, str(count), operation]
    env = {key: value for key, value in os.environ.items() if not key.startswith('PERRY_')}
    if trace:
        env['PERRY_GC_TRACE'] = '1'
    result = subprocess.run(['/usr/bin/time', '-l'] + command, env=env,
                            capture_output=True, text=True, timeout=180)
    prefix = next((line[9:] for line in result.stdout.splitlines()
                   if line.startswith('LIFETIME ')), None)
    assert result.returncode == 0 and prefix, (command, result.returncode, result.stderr[-4000:])
    row = dict(json.loads(prefix), engine=engine, fixture=fixture, trace=trace)
    assert row['iterations'] == count
    assert row['retained'] == (count if mode == 'retain' else 0)
    if operation == 'parse':
        assert row['checksum'] == count
    peak = re.search(r'(\d+)\s+maximum resident set size', result.stderr)
    assert peak
    row['peak_rss'] = int(peak.group(1))
    verify = next((line[7:] for line in result.stdout.splitlines() if line.startswith('VERIFY ')), None)
    if verify is not None:
        actual = json.loads(verify)
        expected = json.loads((ROOT / '.work/fixtures' / (fixture + '.json')).read_text())
        if operation == 'stringify':
            actual = json.loads(actual)
        assert actual == expected, (engine, fixture, mode, operation)
        row['output_verified'] = True
    if trace:
        events = [json.loads(line) for line in result.stderr.splitlines() if line.startswith('{')]
        events = [event for event in events if event.get('event') == 'gc_cycle']
        row['cycles'] = len(events)
        row['gc_pause_us'] = sum(event['pause_us'] for event in events)
        row['copied_objects'] = sum(event['copying_nursery']['copied_objects'] for event in events)
        row['copied_bytes'] = sum(event['copying_nursery']['copied_bytes'] for event in events)
        (OUT / (engine + '-' + fixture + '-' + mode + '.trace.jsonl')).write_text(
            '\n'.join(json.dumps(event, separators=(',', ':')) for event in events) + '\n')
    return row


rng = random.Random(395107)
for fixture, (count, retained_count) in COUNTS.items():
    cases = [(op, mode) for op in ['parse', 'stringify']
             for mode in ['discard', 'latest', 'retain']]
    cases.append(('roundtrip', 'discard'))
    for operation, mode in cases:
        n = retained_count if mode == 'retain' else count
        for repeat in range(3):
            order = list(ENGINES)
            rng.shuffle(order)
            pair = []
            for engine in order:
                row = dict(one(engine, fixture, mode, operation, n), repeat=repeat)
                pair.append(row)
                with (OUT / 'raw.jsonl').open('a') as output:
                    output.write(json.dumps(row, separators=(',', ':')) + '\n')
            assert pair[0]['checksum'] == pair[1]['checksum']
        print('LIFETIME', fixture, operation, mode, n, flush=True)

# Instrumented runs are diagnostics, excluded from the timing pairs.
for fixture in ['wide_1m', 'records_object_1m']:
    for mode in ['discard', 'latest', 'retain']:
        for engine in ENGINES:
            count = 8 if mode == 'retain' else 30
            row = one(engine, fixture, mode, 'parse', count, trace=True)
            with (OUT / 'trace-summary.jsonl').open('a') as output:
                output.write(json.dumps(row, separators=(',', ':')) + '\n')
print('LIFETIMES COMPLETE', flush=True)
