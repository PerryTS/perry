from pathlib import Path
import gzip
import hashlib
import json
import re

w = Path(__file__).resolve().parent
bench = w.parents[1]
d = bench / 'results' / (w.name + '-validation')
d.mkdir(exist_ok=False)
allowed = {'.py', '.json', '.ts', '.js', '.log', '.stdout', '.stderr', '.txt', '.md'}
entries = []
for source in sorted(w.rglob('*')):
    if not source.is_file() or source.suffix not in allowed or '__pycache__' in source.parts:
        continue
    raw = source.read_bytes()
    relative = str(source.relative_to(w))
    compressed = source.suffix in {'.log', '.stdout', '.stderr'}
    data = gzip.compress(raw, compresslevel=9, mtime=0) if compressed else raw
    target = d / (relative + ('.gz' if compressed else ''))
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(data)
    entries.append({'path': str(target.relative_to(d)), 'original_path': relative,
                    'compression': 'gzip' if compressed else None,
                    'sha256': hashlib.sha256(data).hexdigest(),
                    'original_sha256': hashlib.sha256(raw).hexdigest(), 'original_bytes': len(raw)})
(d / 'manifest.json').write_text(json.dumps({
    'measured_source_commit': '1a9c0de6cb790d2467b0ca22a660870025179b37',
    'diagnostic_only': True, 'runtime_changes': False, 'files': entries}, indent=2) + '\n')

count = 0
for manifest in [d / 'manifest.json', *(bench / 'results').glob('quiet-scan-profile-r17*/archive-manifest.json')]:
    record = json.loads(manifest.read_text())
    for e in record['files']:
        data = (manifest.parent / e['path']).read_bytes()
        assert hashlib.sha256(data).hexdigest() == e['sha256']
        raw = gzip.decompress(data) if e['compression'] else data
        assert len(raw) == e['original_bytes'] and hashlib.sha256(raw).hexdigest() == e['original_sha256']
        count += 1
report = (bench / 'SCAN_PROFILE_R17.md').read_text()
for link in re.findall(r'\]\(([^)]+)\)', report):
    if not link.startswith('https://'):
        assert (bench / link).is_file(), link
print('VERIFIED', count, 'diagnostic/window artifacts and all local report links.')
