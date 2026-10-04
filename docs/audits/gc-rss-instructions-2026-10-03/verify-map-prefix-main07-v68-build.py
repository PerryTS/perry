"""Independent source/product/full-suite verification for the private candidate."""
import pathlib,json,hashlib,re,tarfile
B=pathlib.Path('/root/rss-header-20261002');R=B/'primary-map-prefix-main07-v68';E=R/'export'
def sha(p):
 with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def load(p):return json.loads(p.read_text())
assert (E/'complete.exit').read_text().strip()=='0'
p=load(E/'provenance.json');assert p['base']=='07e50b14a3d23e8f914193ec4d5a679adbd7c37a'
inputs=load(E/'source-inputs.json');control=load(B/'primary-entry-once-main07-v45/export/source-inputs.json')
assert len(inputs)==5012 and all(sha(R/'source'/n)==h for n,h in inputs.items())
assert sorted(n for n in inputs if control.get(n)!=inputs[n])==sorted(p['private_paths']) and len(p['private_paths'])==3
commands=load(E/'commands.json');assert [c['name'] for c in commands]==['build','runtime-tests']
for c in commands:
 assert c['rc']==0 and sha(E/(c['name']+'.log'))==c['log_sha256'] and 'Compiling perry-runtime ' in (E/(c['name']+'.log')).read_text()
 assert c['command']==(['cargo','build','--locked','--release','-p','perry','-p','perry-runtime-static','-p','perry-stdlib-static'] if c['name']=='build' else ['cargo','test','--locked','--release','-p','perry-runtime','--lib'])
text=(E/'runtime-tests.log').read_text();summaries=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;',text)
passed,failed,ignored=map(int,summaries[-1]);assert failed==0 and ignored==5 and all(s==('1','0','0') for s in summaries[:-1])
prior=load(B/'primary-entry-once-main07-v45/export/independent-build-verification.json')['runtime']['passed']
assert passed==prior+5
assert re.search(rf'{passed} passed; 0 failed; 5 ignored; 0 measured; 0 filtered out;',text)
for subject in ['early_native_root_does_not_decode_unrelated_suffix', 'unsorted_late_duplicate_offset_keeps_every_native_root', 'folded_functions_keep_all_roots_from_both_sections', 'equally_near_offsets_preserve_existing_stream_order_tie', 'malformed_selected_prefix_still_fails_closed', 'hidden_key_cache_is_marked_by_its_scanner', 'a_declared_prototype_is_born_in_its_final_linked_shape']:
 assert re.search(r'test .*'+subject+r' .*ok',text)
products=load(E/'products.json');assert set(products)=={'perry','libperry_runtime.a','libperry_stdlib.a'} and all(sha(E/n)==h for n,h in products.items())
proof=dict(base=p['base'],private_commit=p['private_commit'],tracked_inputs=len(inputs),private_paths=p['private_paths'],commands=commands,products=products,runtime=dict(passed=passed,failed=failed,ignored=ignored),scope='Private native-map prefix build and full runtime tests, including early suffix avoidance, unsorted duplicate roots, folded functions, tie preservation, and malformed required-prefix rejection. Application benefit remains unproven.')
(E/'independent-build-verification.json').write_text(json.dumps(proof,indent=2)+'\n')
files=[x for x in E.iterdir() if x.is_file() and (x.suffix in ['.json','.log'] or x.name=='complete.exit')]+[pathlib.Path(__file__),B/'primary-map-prefix-main07-v68-build.py']
m=B/'map-prefix-main07-v68-build-evidence-files.json';m.write_text(json.dumps({str(x.relative_to(B)):sha(x) for x in files},indent=2)+'\n')
with tarfile.open(B/'map-prefix-main07-v68-build-evidence.tar.gz','w:gz') as t:
 for x in files+[m]:t.add(x,arcname=str(x.relative_to(B)))
print(json.dumps(proof))
