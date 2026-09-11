from pathlib import Path
import hashlib, gzip, json

work = Path(__file__).resolve().parent
bench = work.parents[1]
dest = bench / ('results/' + work.name + '-validation')
dest.mkdir(exist_ok=False)
entries = []
allowed = {'.py', '.json', '.ts', '.log', '.stdout', '.stderr', '.ll', '.md', '.rs', '.txt', '.diff', '.patch', '.js', '.s', '.gz', '.commit'}
for source in sorted(work.rglob('*')):
    if source.name in {'next-read.patch', 'cached-read-next.rs', 'next-read-candidate.md'}:
        continue
    if not source.is_file() or source.suffix not in allowed or '__pycache__' in source.parts:
        continue
    relative = source.relative_to(work)
    raw = source.read_bytes()
    compressed = source.suffix in {'.log', '.stdout', '.stderr', '.ll', '.s', '.patch', '.diff'} or source.name in {'callback-only.ts', 'run-zero-retained.py'}
    name = str(relative) + ('.gz' if compressed else '')
    target = dest / name
    target.parent.mkdir(parents=True, exist_ok=True)
    data = gzip.compress(raw, compresslevel=9, mtime=0) if compressed else raw
    target.write_bytes(data)
    assert (gzip.decompress(data) if compressed else data) == raw
    entries.append(dict(path=name, original_path=str(relative), original_bytes=len(raw),
                        original_sha256=hashlib.sha256(raw).hexdigest(),
                        sha256=hashlib.sha256(data).hexdigest(), compression='gzip' if compressed else None))
manifest = dict(measured_source_commit=json.loads((work / 'provenance.json').read_text())['source_commit'],
                note=(work / 'validation-verdict.txt').read_text().strip(),
                files=entries)
(dest / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
for entry in entries:
    data = (dest / entry['path']).read_bytes()
    assert hashlib.sha256(data).hexdigest() == entry['sha256']
    raw = gzip.decompress(data) if entry['compression'] else data
    assert len(raw) == entry['original_bytes'] and hashlib.sha256(raw).hexdigest() == entry['original_sha256']
print('VERIFIED', len(entries), 'validation artifacts;', sum((dest / e['path']).stat().st_size for e in entries), 'bytes')
