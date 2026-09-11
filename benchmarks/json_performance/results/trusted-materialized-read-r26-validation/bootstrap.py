from pathlib import Path
import hashlib,json,shutil,subprocess,sys
w=Path(__file__).resolve().parent;root=w.parents[3];old=w.with_name('zero-spacing-r25');sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()
assert not subprocess.check_output(['git','status','--porcelain'],cwd=root)
assert not subprocess.check_output(['git','diff','01f2878dad8efc92e394c49b88fe0800b871f593','HEAD','--','crates','test-files'],cwd=root)
assert (root/'benchmarks/json_performance/results/zero-spacing-r25-artifacts.json').exists()
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
 if p.name in ['write-report.py','index-artifacts.py','archive-validation.py','refresh-validation.py']:continue
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
copy(old/'getter-baseline-comparison.json',w/'getter-baseline-r25-original.json')
rows=[dict(r,arm='main') for r in g['rows'] if r['arm']=='candidate'];assert len(rows)==3
(w/'getter-baseline-comparison.json').write_text(json.dumps(dict(rows=rows,note='R25 candidate receipts relabeled as reference; original preserved separately.'),indent=2)+'\n')
(w/'base.json').write_text(json.dumps(dict(base_commit=head,reference_name='R25'),indent=2)+'\n')

# Preserve immutable compiler input paths for all six worker objects and IR.
for name in ['build-changing-worker.py','build-retained-zero-worker.py']:
 p=w/name;s=p.read_text().replace("w/'changing-options-worker.ts'","w.with_name('zero-spacing-r25')/'changing-options-worker.ts'").replace("w/'retained-zero-worker.ts'","w.with_name('zero-spacing-r25')/'retained-zero-worker.ts'");p.write_text(s)
for name in ['check-zero-roots.py','check-changing-roots.py','check-retained-roots.py','validate-zero.py']:
 p=w/name;s=p.read_text().replace("source = w / ","source = w.with_name('zero-spacing-r25') / ");p.write_text(s)
p=w/'check-roots.py';s=p.read_text().replace('9495bfc95e2afcfb5a7cb535e440e61ec0722cb1','01f2878dad8efc92e394c49b88fe0800b871f593').replace('R24','R25');p.write_text(s)
p=w/'validate-and-build.py';s=p.read_text();start=s.index('paths = ');end=s.index('\nhashes =',start);s=s[:start]+"paths = ['crates/perry-runtime/src/json_tape/cached_read.rs']"+s[end:];p.write_text(s)
p=w/'stage.py';s=p.read_text().replace('json-r25-stage-','json-r26-stage-');p.write_text(s)
p=w/'run-after-build.py';p.write_text(p.read_text().replace('R25 behavior','R26 behavior').replace('R24 unrooted','R25 unrooted'))
for record in records:
 assert sha(root/record['source'])==record['sha256']
 if sha(root/record['copy'])!=record['sha256']:
  assert Path(record['copy']).suffix=='.py'
  record['adapted_controller_sha256']=sha(root/record['copy'])
(w/'main-reuse-provenance.json').write_text(json.dumps(dict(source_commit='01f2878dad8efc92e394c49b88fe0800b871f593',evidence_commit=head,note='Reference arm called main/baseline is measured R25, not current main. All 81 reference checks and 24 IR verdicts are reused; copied original commands identify their actual execution paths.',files=records),indent=2)+'\n')
print('VERIFIED',len(records),'copied R25 reference/harness artifacts; no production edit made.')
