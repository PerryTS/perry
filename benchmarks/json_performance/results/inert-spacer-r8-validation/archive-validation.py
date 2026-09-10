from pathlib import Path
import hashlib, gzip, json

work = Path(__file__).resolve().parent
bench = work.parents[1]
dest = bench / 'results/inert-spacer-r8-validation'
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
                note='R8 bounded inert spacer admission on fresh main 1a9c0de. Both exact production builds and linked worker hashes retained. Nine native fixture hazards remain unsuppressed and match main; shadow, ordinary workers, and callback-only controls are clean. All 24 lazy-array probe outcomes, including six crashes and two output mismatches, match main. Fractional spacing preserves the main/Bun vs Node difference. Script lint public benchmark freshness fails; compile tier and full matrices not run.',
                files=entries)
(dest / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
for entry in entries:
    data = (dest / entry['path']).read_bytes()
    assert hashlib.sha256(data).hexdigest() == entry['sha256']
    raw = gzip.decompress(data) if entry['compression'] else data
    assert len(raw) == entry['original_bytes'] and hashlib.sha256(raw).hexdigest() == entry['original_sha256']
print('VERIFIED', len(entries), 'validation artifacts;', sum((dest / e['path']).stat().st_size for e in entries), 'bytes')
