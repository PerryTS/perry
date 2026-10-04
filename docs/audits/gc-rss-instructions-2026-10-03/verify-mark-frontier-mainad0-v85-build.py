"""Independent source/product/full-suite verification for the private candidate."""
import pathlib,json,hashlib,re,tarfile
B=pathlib.Path('/root/rss-header-20261002');R=B/'primary-mark-frontier-mainad0-v85';E=R/'export'
def sha(p):
 with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def load(p):return json.loads(p.read_text())
assert (E/'complete.exit').read_text().strip()=='0'
p=load(E/'provenance.json');assert p['base']=='ad0a2617bf0b9708032cf89c86575e9bd69fb074'
inputs=load(E/'source-inputs.json');control=load(B/'primary-mainad0-once-v74/export/source-inputs.json')
assert len(inputs)==5015 and all(sha(R/'source'/n)==h for n,h in inputs.items())
assert sorted(n for n in inputs if control.get(n)!=inputs[n])==sorted(p['private_paths'])
commands=load(E/'commands.json');assert [c['name'] for c in commands]==['build','runtime-tests']
for c in commands:
 assert c['rc']==0 and sha(E/(c['name']+'.log'))==c['log_sha256'] and 'Compiling perry-runtime ' in (E/(c['name']+'.log')).read_text()
 assert c['command']==(['cargo','build','--locked','--release','-p','perry','-p','perry-runtime-static','-p','perry-stdlib-static'] if c['name']=='build' else ['cargo','test','--locked','--release','-p','perry-runtime','--lib'])
text=(E/'runtime-tests.log').read_text();summaries=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;',text)
passed,failed,ignored=map(int,summaries[-1]);assert failed==0 and ignored==5 and all(s==('1','0','0') for s in summaries[:-1])
prior=load(B/'primary-mainad0-once-v74/export/independent-build-verification.json')['runtime']['passed']
assert passed==prior+2
assert re.search(rf'{passed} passed; 0 failed; 5 ignored; 0 measured; 0 filtered out;',text)
for subject in ['cyclic_chain_keeps_only_pending_headers_across_budgeted_steps', 'shared_binary_tree_preserves_all_live_objects_with_a_small_frontier']:
 assert re.search(r'test .*'+subject+r' .*ok',text)
products=load(E/'products.json');assert set(products)=={'perry','libperry_runtime.a','libperry_stdlib.a'} and all(sha(E/n)==h for n,h in products.items())
proof=dict(base=p['base'],private_commit=p['private_commit'],tracked_inputs=len(inputs),private_paths=p['private_paths'],commands=commands,products=products,runtime=dict(passed=passed,failed=failed,ignored=ignored),scope='Private pending mark-frontier experiment on pinned mainad0, full unfiltered Linux runtime and fresh products. Application and moving checks still required; not adopted.')
(E/'independent-build-verification.json').write_text(json.dumps(proof,indent=2)+'\n')
files=[x for x in E.iterdir() if x.is_file() and (x.suffix in ['.json','.log'] or x.name=='complete.exit')]+[pathlib.Path(__file__),B/'primary-mark-frontier-mainad0-v85-build.py']
m=B/'mark-frontier-mainad0-v85-build-evidence-files.json';m.write_text(json.dumps({str(x.relative_to(B)):sha(x) for x in files},indent=2)+'\n')
with tarfile.open(B/'mark-frontier-mainad0-v85-build-evidence.tar.gz','w:gz') as t:
 for x in files+[m]:t.add(x,arcname=str(x.relative_to(B)))
print(json.dumps(proof))
