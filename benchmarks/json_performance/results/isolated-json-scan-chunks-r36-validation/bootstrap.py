from pathlib import Path
import hashlib,json,shutil,subprocess
w=Path(__file__).resolve().parent;root=w.parents[3];old=w.with_name('compact-json-token-scan-r35');sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()
assert head=='2f7c36be28acc9e090d4aef1c2337335d83ee6b0'
assert not subprocess.check_output(['git','status','--porcelain'],cwd=root)
records=[]
def copy(p,d,adapt=False):
 assert not d.exists(),d
 d.parent.mkdir(parents=True,exist_ok=True)
 if adapt:
  d.write_text(p.read_text().replace('compact-json-token-scan-r35','isolated-json-scan-chunks-r36').replace('COMPACT_JSON_TOKEN_SCAN_R35','ISOLATED_JSON_SCAN_CHUNKS_R36').replace('R35','R36').replace("'r35'","'r36'").replace('json-r35-','json-r36-'))
 else:shutil.copy2(p,d)
 records.append(dict(source=str(p.relative_to(root)),copy=str(d.relative_to(root)),source_sha256=sha(p),sha256=sha(d),adapted=adapt))
for p in old.rglob('*'):
 if not p.is_file() or '__pycache__' in p.parts:continue
 rel=p.relative_to(old);first=rel.parts[0]
 if (first.startswith('main-') and first!='main-reuse-provenance.json') or first=='frozen-main':copy(p,w/rel)
copy(old/'main-reuse-provenance.json',w/'reference-parent-reuse-provenance.json')
for p in old.glob('*.py'):
 if p.name!='bootstrap.py':copy(p,w/p.name,True)
for p in old.iterdir():
 if p.is_file() and (p.suffix in {'.ts','.js'} or p.name in ['full-cases.json','fraction-baseline.json','large-options-cases.json','changing-options-cases.json']):copy(p,w/p.name)
for p in (old/'harness').rglob('*'):
 if p.is_file():copy(p,w/'harness'/p.relative_to(old/'harness'))
copy(old/'lazy-main-probes.json',w/'lazy-main-probes.json')
for p in old.glob('lazy-*.stdout'):copy(p,w/p.name)
g=json.loads((old/'getter-baseline-comparison.json').read_text());copy(old/'getter-baseline-comparison.json',w/'getter-baseline-parent-original.json')
rows=[r for r in g['rows'] if r['arm']=='main'];assert len(rows)==3
(w/'getter-baseline-comparison.json').write_text(json.dumps(dict(rows=rows,note='Exact frozen R26 reference outcomes copied; full parent comparison retained.'),indent=2)+'\n')
for stem,source in [('emitter','test-files/test_json_string_emitters.ts'),('primitive-keys','test-files/test_json_primitive_tojson_keys.ts')]:
 proof=json.loads((old/('main-'+stem+'-roots.json')).read_text());assert proof['source_sha256']==sha(root/source)
(w/'base.json').write_text(json.dumps(dict(base_commit='a510f0fcb7c38670d82b10f3f074a4b66b1aef4d',reference_name='R26',branch_parent_source=head,previous_candidate_source='891753f2cff604c87133ac2a4435af9a2286aa22'),indent=2)+'\n')
(w/'main-reuse-provenance.json').write_text(json.dumps(dict(source_commit='3aac4d6335da54abeeed73df842decbbe6dd5d71',reference_evidence_commit='a510f0fcb7c38670d82b10f3f074a4b66b1aef4d',copy_via_r35_evidence='8c02047d0e2bf18e53c3b58934f576b41e765b52',note='Exact frozen R26 reference copied through R35. Reuse81original plus20emitter plus4primitive reference executions and28IR only with source/fixture equivalence. Primitive native finding is preserved unsuppressed; shadowchecks pass. All candidate executions will be fresh. No current-main or R35-candidate reference.',files=records),indent=2)+'\n')
(w/'measurement-plan.md').write_text('R36 first validates all105candidate executions,28IR andsixworkerobjects, preserving knownrootfindings. Measure rotating15 first, thenoptions7, largepretty/callback6, full50 andchanging2ifqualified. Ifpositive onhistoricalrows, add bounded Korean-string performance controls because ED falsepositive handling changed. NoGCpolicy orcacheadmission changes.\n')
print('Verified',len(records),'reference/controller payloads; candidate build still independent.')
