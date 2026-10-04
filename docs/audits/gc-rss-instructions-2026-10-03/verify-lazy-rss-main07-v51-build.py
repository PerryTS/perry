"""Independent source/product/full-suite verification for the private candidate."""
import pathlib,json,hashlib,re,tarfile
B=pathlib.Path('/root/rss-header-20261002');R=B/'primary-lazy-rss-main07-v51';E=R/'export'
def sha(p):
 with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def load(p):return json.loads(p.read_text())
assert (E/'complete.exit').read_text().strip()=='0'
p=load(E/'provenance.json');assert p['base']=='07e50b14a3d23e8f914193ec4d5a679adbd7c37a'
inputs=load(E/'source-inputs.json');control=load(B/'primary-main07-v39/export/gc-source-inputs.json')
assert len(inputs)==5011 and all(sha(R/'source'/n)==h for n,h in inputs.items())
assert sorted(n for n in inputs if control.get(n)!=inputs[n])==sorted(p['private_paths']) and len(p['private_paths'])==3
commands=load(E/'commands.json');assert [c['name'] for c in commands]==['build','runtime-tests']
for c in commands:
 assert c['rc']==0 and sha(E/(c['name']+'.log'))==c['log_sha256'] and 'Compiling perry-runtime ' in (E/(c['name']+'.log')).read_text()
 assert c['command']==(['cargo','build','--locked','--release','-p','perry','-p','perry-runtime-static','-p','perry-stdlib-static'] if c['name']=='build' else ['cargo','test','--locked','--release','-p','perry-runtime','--lib'])
text=(E/'runtime-tests.log').read_text();summaries=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;',text)
passed,failed,ignored=map(int,summaries[-1]);assert failed==0 and ignored==5 and all(s==('1','0','0') for s in summaries[:-1])
prior=load(B/'primary-main07-v39/export/independent-build-verification.json')['arms']['gc']['runtime']['passed']
assert passed==prior+2
assert re.search(rf'{passed} passed; 0 failed; 5 ignored; 0 measured; 0 filtered out;',text)
for subject in ['successful_copying_minor_does_not_query_fallback_rss','pinned_minor_fallback_still_samples_rss_and_preserves_root','hidden_key_cache_is_marked_by_its_scanner','a_declared_prototype_is_born_in_its_final_linked_shape']:
 assert re.search(r'test .*'+subject+r' \.\.\. ok',text)
products=load(E/'products.json');assert set(products)=={'perry','libperry_runtime.a','libperry_stdlib.a'} and all(sha(E/n)==h for n,h in products.items())
proof=dict(base=p['base'],private_commit=p['private_commit'],tracked_inputs=len(inputs),private_paths=p['private_paths'],commands=commands,products=products,runtime=dict(passed=passed,failed=failed,ignored=ignored),scope='Private lazy-RSS build and actual copying/pinned fallback tests. Application RSS and instruction benefit remains unproven.')
(E/'independent-build-verification.json').write_text(json.dumps(proof,indent=2)+'\n')
files=[x for x in E.iterdir() if x.is_file() and (x.suffix in ['.json','.log'] or x.name=='complete.exit')]+[pathlib.Path(__file__),B/'primary-lazy-rss-main07-v51-build.py']
m=B/'lazy-rss-main07-v51-build-evidence-files.json';m.write_text(json.dumps({str(x.relative_to(B)):sha(x) for x in files},indent=2)+'\n')
with tarfile.open(B/'lazy-rss-main07-v51-build-evidence.tar.gz','w:gz') as t:
 for x in files+[m]:t.add(x,arcname=str(x.relative_to(B)))
print(json.dumps(proof))
