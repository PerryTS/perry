from pathlib import Path
import hashlib, gzip, json

work = Path(__file__).resolve().parent
bench = work.parents[1]
dest = bench / 'results/construction-context-r12-validation'
dest.mkdir(exist_ok=False)
entries = []
allowed = {'.py', '.json', '.ts', '.log', '.stdout', '.stderr', '.ll', '.md', '.rs', '.txt', '.diff', '.patch', '.js', '.s'}
for source in sorted(work.rglob('*')):
    if not source.is_file() or source.suffix not in allowed or '__pycache__' in source.parts:
        continue
    relative = source.relative_to(work)
    raw = source.read_bytes()
    compressed = source.suffix in {'.log', '.stdout', '.stderr', '.ll', '.s', '.patch'} or source.name == 'callback-only.ts'
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
                note='R12 rejected: source-length gains remain and array parsing improves, but small-record repeated-input parse +1.09% with separated ranges in the longer recheck; changing input +0.78% with overlapping ranges, all 11 pairs slower in both. 508 timing, 68 calibration, 116 full-output verification trials; two archived quiet windows. 296 JSON units, 19 fixtures and 14 options per arm pass. Seven native findings remain unsuppressed and match main; shadow checks pass. Known lazy/fraction gaps preserved, lint freshness fails. Broader qualification not run.',
                files=entries)
(dest / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
for entry in entries:
    data = (dest / entry['path']).read_bytes()
    assert hashlib.sha256(data).hexdigest() == entry['sha256']
    raw = gzip.decompress(data) if entry['compression'] else data
    assert len(raw) == entry['original_bytes'] and hashlib.sha256(raw).hexdigest() == entry['original_sha256']
print('VERIFIED', len(entries), 'validation artifacts;', sum((dest / e['path']).stat().st_size for e in entries), 'bytes')
