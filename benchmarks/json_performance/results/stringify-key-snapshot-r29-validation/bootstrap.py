from pathlib import Path
import hashlib,json,shutil,subprocess,sys
w=Path(__file__).resolve().parent;root=w.parents[3];old=w.with_name('trusted-materialized-read-r26');sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()
assert not subprocess.check_output(['git','status','--porcelain'],cwd=root)
assert head=='db78b06ec522f8a6a62a4e773ab4e42d0525bdee'
assert (root/'benchmarks/json_performance/results/trusted-materialized-read-r26-artifacts.json').exists()
records=[]
def copy(p,d):
 assert not d.exists(),d
 d.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(p,d);assert sha(p)==sha(d)
 records.append(dict(source=str(p.relative_to(root)),copy=str(d.relative_to(root)),sha256=sha(d)))
for p in old.rglob('*'):
 if not p.is_file() or '__pycache__' in p.parts:continue
 rel=p.relative_to(old);first=rel.parts[0]
 if first.startswith('candidate-'):
  copy(p,w/Path('main-'+first.removeprefix('candidate-'),*rel.parts[1:]))
 elif first=='frozen-build':copy(p,w/'frozen-main'/Path(*rel.parts[1:]))
for p in old.glob('*.py'):
 if p.name in ['bootstrap.py','write-report.py','index-artifacts.py','archive-validation.py','refresh-validation.py']:continue
 copy(p,w/p.name)
for p in old.iterdir():
 if p.is_file() and (p.suffix in {'.ts','.js'} or p.name in ['full-cases.json','fraction-baseline.json']):
  if p.name in ['cached-read-next.rs']:continue
  copy(p,w/p.name)
for p in (old/'harness').rglob('*'):
 if p.is_file():copy(p,w/'harness'/p.relative_to(old/'harness'))
copy(old/'build-provenance.json',w/'main-build-provenance.json')
copy(old/'lazy-candidate-probes.json',w/'lazy-main-probes.json')
for p in old.glob('candidate-lazy-*.stdout'):copy(p,w/p.name.removeprefix('candidate-'))
# Preserve the exact original getter receipt, then adapt only its arm label.
g=json.loads((old/'getter-baseline-comparison.json').read_text())
copy(old/'getter-baseline-comparison.json',w/'getter-baseline-r26-original.json')
rows=[dict(r,arm='main') for r in g['rows'] if r['arm']=='candidate'];assert len(rows)==3
(w/'getter-baseline-comparison.json').write_text(json.dumps(dict(rows=rows,note='R26 candidate receipts relabeled as reference; original preserved separately.'),indent=2)+'\n')
(w/'base.json').write_text(json.dumps(dict(base_commit=head,reference_name='R25'),indent=2)+'\n')

# Preserve immutable compiler input paths for all six worker objects and IR.
p=w/'check-roots.py';p.write_text(p.read_text().replace('01f2878dad8efc92e394c49b88fe0800b871f593','3aac4d6335da54abeeed73df842decbbe6dd5d71').replace('R25','R26'))
p=w/'stage.py';p.write_text(p.read_text().replace('json-r26-stage-','json-r29-stage-'))
p=w/'run-after-build.py';p.write_text(p.read_text().replace('R26 behavior','R29 behavior').replace('R25 unrooted','R26 unrooted'))
(w/'base.json').write_text(json.dumps(dict(base_commit='a510f0fcb7c38670d82b10f3f074a4b66b1aef4d',reference_name='R26'),indent=2)+'\n')
for record in records:
 assert sha(root/record['source'])==record['sha256']
 if sha(root/record['copy'])!=record['sha256']:
  assert Path(record['copy']).suffix=='.py'
  record['adapted_controller_sha256']=sha(root/record['copy'])
(w/'main-reuse-provenance.json').write_text(json.dumps(dict(source_commit='3aac4d6335da54abeeed73df842decbbe6dd5d71',evidence_commit='a510f0fcb7c38670d82b10f3f074a4b66b1aef4d',note='Reference arm called main/baseline is measured R26, not current main. All 81 reference checks and 24 IR verdicts are reused; copied original commands identify their actual execution paths.',files=records),indent=2)+'\n')
print('VERIFIED',len(records),'copied R26 reference/harness artifacts; no production edit made.')
