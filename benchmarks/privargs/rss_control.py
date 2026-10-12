#!/usr/bin/env python3
"""Separate max-RSS accounting noise from actual resident-page differences."""
import ctypes
import json
from pathlib import Path
import statistics
import subprocess
import sys
import time
from measure import environment, stat

root = Path(sys.argv[1]).resolve()
ctypes.CDLL(None).prctl(41, 1, 0, 0, 0)
results = {}
for case in ['same_binary', 'actual_arms']:
    rows = {arm: [] for arm in ['main', 'args']}
    for i in range(5):
        for arm in (['main', 'args'] if i % 2 == 0 else ['args', 'main']):
            binary_arm = 'main' if case == 'same_binary' else arm
            rows[arm].append(stat(root, f'rss-control.{case}.{arm}.{i}',
                [str(root / 'bin' / binary_arm / 'hello')], environment(root, arm), root))
    results[case] = {arm: {'rss': statistics.median(r['rss'] for r in row),
                          'trials': row} for arm, row in rows.items()}
for arm in ['main', 'args']:
    process = subprocess.Popen([str(root / 'bin' / arm / 'commander'), '5000', '200'],
                               env=environment(root, arm), cwd=root / 'drivers/pk',
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    time.sleep(0.1)
    proc = Path('/proc') / str(process.pid)
    results['pages-' + arm] = (proc / 'smaps_rollup').read_text()
    results['status-' + arm] = '\n'.join(line for line in (proc / 'status').read_text().splitlines()
                                        if line.startswith('THP_enabled:'))
    process.communicate(timeout=60)
    assert process.returncode == 0
(root / 'rss-control.json').write_text(json.dumps(results, indent=2))
print(results)
