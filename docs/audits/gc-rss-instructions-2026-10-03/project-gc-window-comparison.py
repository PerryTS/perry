"""Check a complete three-arm matrix, then project two measured pair comparisons."""
import hashlib
import json
from pathlib import Path
import subprocess
import argparse

parser = argparse.ArgumentParser()
parser.add_argument('--root', type=Path, default=Path('/root/rss-header-20261002'))
B = parser.parse_args().root
V = 'gc-auto-window-v6'
receipt = B / 'gc-runtime-window-v6-auto.exit'
assert receipt.read_text().strip() == '0'
raw = B / (V + '-runs.json')
rows = json.loads(raw.read_text())
oracle = json.loads((B / (V + '-oracle.json')).read_text())
assert len(oracle) == 17 and len(rows) == 306
arms = ['main69-base', 'main69-gc', 'main69-window']
expected = {(case, arm, mode, repeat) for case in oracle for arm in arms
            for mode in ['plain', 'perf'] for repeat in range(3)}
assert len({(r['case'], r['arm'], r['mode'], r['repeat']) for r in rows}) == 306
assert {(r['case'], r['arm'], r['mode'], r['repeat']) for r in rows} == expected
for row in rows:
    assert row['rc'] == 0 and row['reason'] is None and row['stdout'] == oracle[row['case']]
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
proof = dict(scope='306 actual executions, projected without altering recorded cells',
             raw_matrix_sha256=sha(raw), projected=[])
for baseline in ['main69-base', 'main69-gc']:
    prefix = B / (V + '-vs-' + baseline)
    paths = {}
    for suffix in ['builds', 'oracle']:
        source = B / (V + '-' + suffix + '.json')
        target = Path(str(prefix) + '-' + suffix + '.json')
        assert not target.exists()
        target.write_bytes(source.read_bytes())
        paths[suffix] = sha(target)
    subset = [r for r in rows if r['arm'] in [baseline, 'main69-window']]
    assert len(subset) == 204
    projected = Path(str(prefix) + '-runs.json')
    assert not projected.exists()
    projected.write_text(json.dumps(subset, indent=2) + '\n')
    paths['runs'] = sha(projected)
    # This verifier binds each cell to its workload, build and current binary,
    # and checks every RSS/instruction/cycle/fault counter; it makes no verdict
    # on whether a performance tradeoff is acceptable.
    subprocess.run(['python3', str(B / 'summarize-gc-comparison.py'), str(prefix),
                    '--arms', baseline, 'main69-window', '--suite', 'auto',
                    '--fault-events', '--complete', str(receipt)], check=True)
    proof['projected'].append(dict(baseline=baseline, candidate='main69-window', input_hashes=paths))
(B / (V + '-projection-proof.json')).write_text(json.dumps(proof, indent=2) + '\n')
