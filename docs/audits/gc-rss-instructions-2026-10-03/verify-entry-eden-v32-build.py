"""Independent tracked-source, actual product and test-log attestation."""
import pathlib,json,hashlib,re
B=pathlib.Path('/root/rss-header-20261002');R=B/'primary-entry-eden-v32';E=R/'export';S=R/'source'
def sha(p):
    with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def load(p):return json.loads(p.read_text())
assert (B/'gc-entry-eden-v32-build-driver.exit').read_text().strip()=='0'
assert (E/'complete.exit').read_text().strip()=='0'
control=load(B/'primary-latest2026-v31/export/gc-source-inputs.json');inputs=load(E/'source-inputs.json')
assert len(inputs)==len(control)==4999 and inputs.keys()==control.keys()
differences=[n for n in sorted(inputs) if inputs[n]!=control[n]]
provenance=load(E/'provenance.json')
assert differences==sorted(provenance['paths']) and len(differences)==6
assert provenance['private_head']=='6e311851fa6a6e0c981b7e2f2c34d3bdc5d3d2f9'
assert provenance['base']=='2026ecfe6dd9df1a0a8616e3bc5c5561e8e0cf63'
assert all(sha(S/n)==h for n,h in inputs.items())
commands=load(E/'commands.json');assert [c['name'] for c in commands]==['build','runtime-tests']
for c in commands:
    log=E/(c['name']+'.log');assert c['rc']==0 and sha(log)==c['log_sha256']
    assert 'Compiling perry-runtime ' in log.read_text()
log=(E/'runtime-tests.log').read_text()
test_summaries=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;',log)
assert test_summaries[-1]==('4862','0','5')
assert all(row==('1','0','0') for row in test_summaries[:-1])
assert re.search(r'test .*collection_entry_discards_only_previously_idle_eden_pages_and_keeps_reuse_safe \.\.\. ok',log)
products=load(E/'products.json');assert set(products)=={'perry','libperry_runtime.a','libperry_stdlib.a'}
assert all(sha(E/n)==h for n,h in products.items())
proof=dict(private_commit=provenance['private_head'],base=provenance['base'],tracked_inputs=4999,differences=differences,actual_products=products,linux_runtime=dict(passed=4862,failed=0,ignored=5),commands=commands,input_manifest_sha256=sha(E/'source-inputs.json'),filtered_subprocess_test_summaries=test_summaries[:-1],scope='Private collection-entry overlay on rebased GC proposal; Linux mincore and full runtime pass. Application benefit unproven; unapplied.')
(E/'independent-build-verification.json').write_text(json.dumps(proof,indent=2)+'\n');print(json.dumps(proof))

import tarfile
files=[p for p in E.iterdir() if p.is_file() and p.name not in ['perry','libperry_runtime.a','libperry_stdlib.a']]+[pathlib.Path(__file__),B/'gc-entry-eden-v32-build-driver.exit',B/'gc-entry-eden-v32-build-driver.log']
manifest=B/'entry-eden-v32-build-evidence-files.json';manifest.write_text(json.dumps({str(p.relative_to(B)):sha(p) for p in files},indent=2)+'\n')
with tarfile.open(B/'entry-eden-v32-build-evidence.tar.gz','w:gz') as t:
 for p in files+[manifest]:t.add(p,arcname=str(p.relative_to(B)))
