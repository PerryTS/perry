"""Independent tracked-source, actual product and test-log attestation."""
import pathlib,json,hashlib,re
B=pathlib.Path('/root/rss-header-20261002');R=B/'primary-idle-eden-v30';E=R/'export';S=R/'source'
def sha(p):
    with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def load(p):return json.loads(p.read_text())
assert (B/'gc-private-idle-eden-v30-build-driver.exit').read_text().strip()=='0'
assert (E/'complete.exit').read_text().strip()=='0'
control=load(B/'primary-gc-runtime-v24/export/control-source-inputs.json');inputs=load(E/'source-inputs.json')
assert len(inputs)==len(control)==4994 and inputs.keys()==control.keys()
differences=[n for n in sorted(inputs) if inputs[n]!=control[n]]
assert differences==['crates/perry-runtime/src/arena/block.rs','crates/perry-runtime/src/arena/reset.rs']
assert all(sha(S/n)==h for n,h in inputs.items())
commands=load(E/'commands.json');assert [c['name'] for c in commands]==['build','runtime-tests']
for c in commands:
    log=E/(c['name']+'.log');assert c['rc']==0 and sha(log)==c['log_sha256']
    assert 'Compiling perry-runtime ' in log.read_text()
log=(E/'runtime-tests.log').read_text()
test_summaries=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;',log)
assert test_summaries[-1]==('4856','0','5')
assert all(row==('1','0','0') for row in test_summaries[:-1])
products=load(E/'products.json');assert set(products)=={'perry','libperry_runtime.a','libperry_stdlib.a'}
assert all(sha(E/n)==h for n,h in products.items())
proof=dict(private_commit='3843fcb54a',base='c17090892e2a749bb4a3f2a0022e0f8274f0320c',tracked_inputs=4994,differences=differences,actual_products=products,linux_runtime=dict(passed=4856,failed=0,ignored=5),commands=commands,input_manifest_sha256=sha(E/'source-inputs.json'),filtered_subprocess_test_summaries=test_summaries[:-1],original_verifier_failure='Rejected extra filtered subprocess summaries despite the full suite passing; raw logs unchanged.',scope='Private two-file overlay on unchanged GC proposal; builds/tests only, application benefit unproven.')
(E/'independent-build-verification.json').write_text(json.dumps(proof,indent=2)+'\n');print(json.dumps(proof))
