from pathlib import Path
import hashlib
import json
import subprocess

w = Path(__file__).resolve().parent
root = w.parents[3]
source = json.loads((w / 'build-provenance.json').read_text())['source_commit']
assert source == subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
assert not subprocess.check_output(['git', 'status', '--porcelain'], cwd=root)
for step in [
    ['build-workers.py'],
    ['validate-fixtures.py'],
    ['validate-options.py', '--candidate'],
    ['probe-lazy-baseline.py', '--candidate'],
    ['validate-fraction.py'],
    ['validate-getter-baseline.py'],
    ['check-roots.py'],
    ['compare-roots.py'],
]:
    subprocess.run(['python3', str(w / step[0]), *step[1:]], cwd=root, check=True)
for name in ['worker', 'access-worker', 'rotating-worker', 'options']:
    assert (w / ('main-' + name + '.o')).read_bytes() == (w / ('candidate-' + name + '.o')).read_bytes(), name
for arm in ['main', 'candidate']:
    rows = json.loads((w / (arm + '-fixture-validation.json')).read_text())
    assert len(rows) == 46 and all(r['exit_code'] == 0 and r['matches_node'] for r in rows)
    rows = json.loads((w / (arm + '-options-validation.json')).read_text())
    assert len(rows) == 14 and all(r['matches_node'] for r in rows)
print('VERIFIED both-arm behavior/options/static checks and all four worker object equivalences.', flush=True)
