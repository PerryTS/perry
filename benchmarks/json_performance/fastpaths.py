#!/usr/bin/env python3
"""Fresh-runtime A/B, Node and Bun. Invoke through with_lock.py fastpaths."""
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent
OUT = ROOT / 'results/fastpaths'
OUT.mkdir(parents=True, exist_ok=True)
if any(OUT.glob('*.jsonl')):
    raise SystemExit('Refusing to append to an existing A/B. Use a fresh run directory.')
for phase in ['verify', 'timing', 'memory']:
    subprocess.run([
        'python3', str(ROOT / 'run.py'), '--phase', phase,
        '--worker', str(ROOT / '.work/worker'),
        '--baseline-worker', str(ROOT / '.work/baseline-worker'),
        '--operations', 'parse,stringify', '--results-dir', str(OUT),
    ], check=True)
    rows = [json.loads(line) for line in (OUT / (phase + '.jsonl')).read_text().splitlines()]
    assert rows and all('error' not in row for row in rows), (phase, 'failed trial')
    if phase == 'verify':
        assert len(rows) == 19 * 2 * 4 and all(row['correct'] for row in rows), 'output mismatch'
    print('PASSED', phase, len(rows), flush=True)
