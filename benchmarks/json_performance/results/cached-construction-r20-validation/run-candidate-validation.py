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
    ['check-roots.py'],
    ['compare-roots.py'],
]:
    subprocess.run(['python3', str(w / step[0]), *step[1:]], cwd=root, check=True)
for name in ['worker', 'access-worker', 'rotating-worker', 'options']:
    assert (w / ('main-' + name + '.o')).read_bytes() == (w / ('candidate-' + name + '.o')).read_bytes(), name
for arm in ['main', 'candidate']:
    rows = json.loads((w / (arm + '-fixture-validation.json')).read_text())
    assert len(rows) == 37 and all(r['exit_code'] == 0 and r['matches_node'] for r in rows)
    rows = json.loads((w / (arm + '-options-validation.json')).read_text())
    assert len(rows) == 14 and all(r['matches_node'] for r in rows)
symbol_evidence = []
for arm in ['main', 'candidate']:
    worker = w / (arm + '-worker')
    result = subprocess.run(['nm', '-n', str(worker)], cwd=root, capture_output=True, text=True, check=True)
    selected = [line for line in result.stdout.splitlines() if 'materialize_cached_array' in line]
    assert bool(selected) == (arm == 'candidate'), (arm, selected)
    symbol_evidence.append({'arm': arm, 'worker_sha256': hashlib.sha256(worker.read_bytes()).hexdigest(),
                            'symbols': selected})
(w / 'producer-symbols.json').write_text(json.dumps({
    'source_commit': source, 'workers': symbol_evidence,
    'note': 'Only a linkage witness. This does not establish runtime entry, phase savings or performance.'}, indent=2) + '\n')
print('VERIFIED both-arm behavioral/options/static comparison, unchanged worker objects and linked candidate producer. Performance not measured.', flush=True)
