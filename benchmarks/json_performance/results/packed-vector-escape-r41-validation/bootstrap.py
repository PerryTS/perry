from pathlib import Path
import hashlib, json, shutil, subprocess

w = Path(__file__).resolve().parent
root = w.parents[3]
primary = Path('/Users/amlug/projects/perry/json-merged-pr10022')
old = primary / 'benchmarks/json_performance/.work/native-vector-escape-r40'
head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
assert head == '3952e832058a5e1efcd4cdf9cf28f26e31d60ea3'
assert not subprocess.check_output(['git', 'status', '--porcelain'], cwd=root)
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
records = []

def copy(source, destination, adapt=False):
    raw = source.read_bytes()
    data = raw
    if adapt:
        data = raw.decode().replace('native-vector-escape-r40', w.name).replace('NATIVE_VECTOR_ESCAPE_R40', 'PACKED_VECTOR_ESCAPE_R41').replace('R40', 'R41').replace("'r40'", "'r41'").replace('json-r40-', 'json-r41-').replace('3bde9394e021039603c84801762af4f083b1f9c4', head).encode()
    existed = destination.exists()
    if existed:
        assert destination.read_bytes() == data, destination
    else:
        destination.parent.mkdir(parents=True, exist_ok=True)
        if adapt:
            destination.write_bytes(data)
        else:
            shutil.copy2(source, destination)
    assert destination.read_bytes() == data
    records.append(dict(source=str(source), path=str(destination.relative_to(root)), source_sha256=hashlib.sha256(raw).hexdigest(), sha256=sha(destination), adapted=adapt, existed=existed))

for p in old.rglob('*'):
    if not p.is_file() or '__pycache__' in p.parts:
        continue
    rel = p.relative_to(old)
    if (rel.parts[0].startswith('main-') and rel.parts[0] != 'main-reuse-provenance.json') or rel.parts[0] == 'frozen-main':
        copy(p, w/rel)
excluded = {'bootstrap.py', 'build-release.py', 'validate-and-build.py', 'run-lint.py', 'copy-frozen-build.py', 'check-roots.py', 'run-fresh-root-checks.py', 'resume-candidate-validation.py'}
for p in old.glob('*.py'):
    if p.name not in excluded:
        copy(p, w/p.name, True)
for p in old.iterdir():
    if p.is_file() and (p.suffix in {'.ts', '.js'} or p.name in {'full-cases.json', 'fraction-baseline.json', 'large-cases.json', 'changing-options-cases.json', 'korean-plan.md'}):
        copy(p, w/p.name)
for folder in ['harness', 'korean-fixtures']:
    for p in (old/folder).rglob('*'):
        if p.is_file() and '__pycache__' not in p.parts:
            copy(p, w/folder/p.relative_to(old/folder))
copy(old/'lazy-main-probes.json', w/'lazy-main-probes.json')
for p in old.glob('lazy-*.stdout'):
    copy(p, w/p.name)
g = json.loads((old/'getter-baseline-comparison.json').read_text())
copy(old/'getter-baseline-comparison.json', w/'getter-baseline-parent-original.json')
(w/'getter-baseline-comparison.json').write_text(json.dumps(dict(rows=[r for r in g['rows'] if r['arm']=='main'], note='Only hash-verified reference receipts reused.'), indent=2)+'\n')
for folder in ['materialized-read-r23', 'zero-spacing-r25']:
    for p in (old.with_name(folder)).rglob('*'):
        if p.is_file() and p.suffix in {'.ts', '.js', '.py'} and '__pycache__' not in p.parts:
            copy(p, root/p.relative_to(primary))
for n in ['main-workers-provenance.json', 'main-changing-workers-provenance.json', 'main-retained-zero-workers-provenance.json', 'main-emitter-validation.json', 'main-primitive-keys-validation.json', 'main-large-emitter-validation.json']:
    value = json.loads((w/n).read_text())
    for row in value if isinstance(value, list) else [value]:
        for path, digest in row['files'].items():
            assert sha(primary/path) == digest
            copy(primary/path, root/path)
            assert sha(root/path) == digest
for p in (old.parents[1]/'.work/fixtures').glob('*.json'):
    copy(p, root/p.relative_to(primary))
copy(old/'main-reuse-provenance.json', w/'reference-parent-reuse-provenance.json')
(w/'base.json').write_text(json.dumps(dict(base_commit='a510f0fcb7c38670d82b10f3f074a4b66b1aef4d', reference_name='R26', candidate_source=head, branch_parent_source='3bde9394e021039603c84801762af4f083b1f9c4'), indent=2)+'\n')
(w/'main-reuse-provenance.json').write_text(json.dumps(dict(source_commit='3aac4d6335da54abeeed73df842decbbe6dd5d71', reference_evidence_commit='a510f0fcb7c38670d82b10f3f074a4b66b1aef4d', checker_source_commit='e69d1292110cf9abee24f8690c584652805a4167', note='Frozen R26 reference artifacts and receipts copied with exact hashes through R40 evidence df1c4e18edc15ea37ec294a2b7c25af070001276. The original-suite checker commands were freshly executed at R39 and remain historical receipts here. All candidate output executions and emitted IR will be fresh. No current-main or R39-candidate timing reference.', files=records), indent=2)+'\n')
print('Verified', len(records), 'reference, source and controller payloads')
