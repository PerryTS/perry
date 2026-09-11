from pathlib import Path
import hashlib,json,shutil,subprocess
w=Path(__file__).resolve().parent;root=w.parents[3];old=w.with_name('stringify-token-proof-r31');sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()
assert head=='c695f6509ed85999894a2f34418f057f32bcb363'
assert not subprocess.check_output(['git','status','--porcelain'],cwd=root)
records=[]
def copy(p,d):
 assert not d.exists(),d
 d.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(p,d);assert sha(p)==sha(d)
 records.append(dict(source=str(p.relative_to(root)),copy=str(d.relative_to(root)),sha256=sha(d)))
for p in old.rglob('*'):
 if not p.is_file() or '__pycache__' in p.parts:continue
 rel=p.relative_to(old);first=rel.parts[0]
 if first.startswith('main-') and first!='main-reuse-provenance.json' and not first.startswith('main-emitter'):copy(p,w/rel)
 elif first=='frozen-main':copy(p,w/rel)
copy(old/'main-reuse-provenance.json',w/'reference-parent-reuse-provenance.json')
for p in old.glob('*.py'):
 if p.name not in ['bootstrap.py','write-report.py','index-artifacts.py','archive-validation.py','refresh-validation.py']:copy(p,w/p.name)
for p in old.iterdir():
 if p.is_file() and (p.suffix in {'.ts','.js'} or p.name in ['full-cases.json','fraction-baseline.json']):copy(p,w/p.name)
for p in (old/'harness').rglob('*'):
 if p.is_file():copy(p,w/'harness'/p.relative_to(old/'harness'))
copy(old/'lazy-main-probes.json',w/'lazy-main-probes.json')
for p in old.glob('lazy-*.stdout'):copy(p,w/p.name)
g=json.loads((old/'getter-baseline-comparison.json').read_text());copy(old/'getter-baseline-comparison.json',w/'getter-baseline-parent-original.json')
rows=[r for r in g['rows'] if r['arm']=='main'];assert len(rows)==3
(w/'getter-baseline-comparison.json').write_text(json.dumps(dict(rows=rows,note='Exact R26 reference rows copied from R27; original parent comparison retained.'),indent=2)+'\n')
# R26 reference base includes callback rooting, scalar emitters and string-flag
# invalidation in the source patch; no earlier experiment evidence in the diff.
base='a510f0fcb7c38670d82b10f3f074a4b66b1aef4d'
(w/'base.json').write_text(json.dumps(dict(base_commit=base,reference_name='R26',branch_parent_source=head,previous_candidate_source='f0bc3aeb6a2597a941ad4afe1b70e79e2df11a17'),indent=2)+'\n')
p=w/'run-after-build.py';p.write_text(p.read_text().replace('R29 behavior','R30 behavior'))
p=w/'complete-candidate-validation.py';p.write_text(p.read_text().replace('R29 candidate','R30 candidate'))
p=w/'stage.py';p.write_text(p.read_text().replace('json-r29-stage-','json-r30-stage-'))
p=w/'compare-json-entry-disassembly.py';p.write_text(p.read_text().replace("'r29':w/'candidate-worker'","'r30':w/'candidate-worker'"))
for r in records:
 assert sha(root/r['source'])==r['sha256']
 if sha(root/r['copy'])!=r['sha256']:
  assert Path(r['copy']).suffix=='.py'
  r['adapted_controller_sha256']=sha(root/r['copy'])
(w/'main-reuse-provenance.json').write_text(json.dumps(dict(source_commit='3aac4d6335da54abeeed73df842decbbe6dd5d71',reference_evidence_commit='a510f0fcb7c38670d82b10f3f074a4b66b1aef4d',copy_via_r31_source='f0bc3aeb6a2597a941ad4afe1b70e79e2df11a17',note='Exact frozen R26 reference copied from R31 main arm. All 81 original reference behavior receipts and 24 IR verdicts are reused. The changed emitter fixture is freshly executed and checked in both arms. Reference is not current main or R29 candidate.',files=records),indent=2)+'\n')
print('VERIFIED',len(records),'R26 reference/controller payloads via R31; no production edit.')
