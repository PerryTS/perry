"""Attest actual-GC residency contract and deliberately failing baseline control."""
import pathlib,json,hashlib,tarfile,re
B=pathlib.Path('/root/rss-header-20261002');R=B/'primary-entry-eden-v35r1-collection-test'
def sha(p):
 with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
assert (B/'gc-entry-eden-v35r1-collection-test-driver.exit').read_text().strip()=='0' and (R/'complete.exit').read_text().strip()=='0'
rows=json.loads((R/'commands.json').read_text());assert len(rows)==2 and [x['arm'] for x in rows]==['entry','control']
manifest={}
for row in rows:
 a=row['arm'];E=R/('export-'+a);S=R/('source-'+a);m=json.loads((E/'source-inputs.json').read_text());manifest[a]=m
 assert len(m)==5000 and all(sha(S/n)==h for n,h in m.items())
 assert sha(E/'source-inputs.json')==row['source_inputs_sha256'] and sha(E/'test.log')==row['log_sha256']
 assert row['command']==['cargo','test','--locked','--release','-p','perry-runtime','--lib','real_full_collection_gives_eden_a_reuse_interval_then_discards_idle_pages','--','--nocapture']
 text=(E/'test.log').read_text();assert 'Compiling perry-runtime ' in text
 if a=='entry':assert row['rc']==0 and 'test result: ok. 1 passed; 0 failed;' in text
 else:
  assert row['rc']==101 and 'test result: FAILED. 0 passed; 1 failed;' in text and 'next real collection entry discards unused interior pages' in text
  assert 'fixture allocated several Eden blocks' not in text and 'first reset gives warm pages' not in text
private=json.loads((B/'primary-entry-eden-v32/export/source-inputs.json').read_text());control=json.loads((B/'primary-latest2026-v31/export/gc-source-inputs.json').read_text())
for a,original in [('entry',private),('control',control)]:
 diff=[n for n in sorted(set(manifest[a])|set(original)) if manifest[a].get(n)!=original.get(n)]
 assert diff==['crates/perry-runtime/src/gc/tests/eden_entry_residency.rs','crates/perry-runtime/src/gc/tests/mod.rs']
source=R/'source-entry/crates/perry-runtime/src/gc/tests/eden_entry_residency.rs'
assert sha(source)==sha(R/'source-control/crates/perry-runtime/src/gc/tests/eden_entry_residency.rs')
assert re.search(r'\bblock\.dead_cycles\s*=(?!=)',source.read_text()) is None and 'arena.current >=' not in source.read_text()
proof=dict(base='2026ecfe6dd9df1a0a8616e3bc5c5561e8e0cf63',production_runtime='6c761801fa4c76ad79b67bbc4338bee7739093b7',private_entry='6e311851fa6a6e0c981b7e2f2c34d3bdc5d3d2f9',commands=rows,source_counts={a:len(m) for a,m in manifest.items()},test_source_sha256=sha(source),scope='Real nursery strings, actual full publication, intervening mutator allocation and second real collection. Candidate residency/reuse contract passes; baseline fails precisely that contract, not a fixture precondition. Original cursor-fixture failure preserved; shipping policy unadopted.')
(R/'verification.json').write_text(json.dumps(proof,indent=2)+'\n')
files=[p for p in R.rglob('*') if p.is_file() and 'target-' not in str(p) and (p.suffix in ['.json','.log'] or p.name=='complete.exit') and '/source-' not in str(p)]+[source,B/'gc-entry-eden-v35r1-collection-test-driver.exit',B/'gc-entry-eden-v35r1-collection-test-driver.log',pathlib.Path(__file__)]
m=B/'entry-eden-v35r1-collection-evidence-files.json';m.write_text(json.dumps({str(p.relative_to(B)):sha(p) for p in files},indent=2)+'\n')
with tarfile.open(B/'entry-eden-v35r1-collection-evidence.tar.gz','w:gz') as t:
 for p in files+[m]:t.add(p,arcname=str(p.relative_to(B)))
print(json.dumps(proof))
