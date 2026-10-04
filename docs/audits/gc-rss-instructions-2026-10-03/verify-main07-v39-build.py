"""Independent current checkpoint build/source/full-suite attestation."""
import pathlib,json,hashlib,re,tarfile
B=pathlib.Path('/root/rss-header-20261002');R=B/'primary-main07-v39';E=R/'export'
def sha(p):
 with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def load(p):return json.loads(p.read_text())
assert (B/'gc-main07-v39-build-driver.exit').read_text().strip()=='0' and (E/'complete.exit').read_text().strip()=='0'
p=load(E/'provenance.json');assert p['base']=='07e50b14a3d23e8f914193ec4d5a679adbd7c37a'
manifest={a:load(E/(a+'-source-inputs.json')) for a in ['base','gc','entry']}
diffs=[n for n in sorted(set(manifest['base'])|set(manifest['gc'])) if manifest['base'].get(n)!=manifest['gc'].get(n)]
assert diffs==sorted(p['files']) and len(diffs)==22 and all(n.startswith('crates/perry-runtime/src/') for n in diffs)
private=[n for n in sorted(manifest['entry']) if manifest['entry'][n]!=manifest['gc'][n]];assert private==sorted(p['private_paths']) and len(private)==6
arms={}
for a,count in [('base',5003),('gc',5010),('entry',5010)]:
 inputs=manifest[a];assert len(inputs)==count and all(sha(R/('source-'+a)/n)==h for n,h in inputs.items())
 if a!='entry':assert sha(E/(a+'-source-inputs.json'))==p['manifest_hashes'][a]
 commands=load(E/(a+'-commands.json'));assert [c['name'] for c in commands]==['build','runtime-tests']
 for c in commands:
  text=(E/a/(c['name']+'.log')).read_text();assert c['rc']==0 and sha(E/a/(c['name']+'.log'))==c['log_sha256'] and 'Compiling perry-runtime ' in text
  assert c['command']==(['cargo','build','--locked','--release','-p','perry','-p','perry-runtime-static','-p','perry-stdlib-static'] if c['name']=='build' else ['cargo','test','--locked','--release','-p','perry-runtime','--lib'])
 text=(E/a/'runtime-tests.log').read_text();summaries=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;',text)
 passed,failed,ignored=map(int,summaries[-1]);assert passed>=4840 and failed==0 and ignored==5 and all(s==('1','0','0') for s in summaries[:-1])
 assert re.search(rf'{passed} passed; 0 failed; 5 ignored; 0 measured; 0 filtered out;',text)
 for subject in ['hidden_key_cache_is_marked_by_its_scanner','hidden_key_cache_is_rewritten_by_its_scanner','hidden_key_scanner_is_registered_and_survives_gc','a_declared_prototype_is_born_in_its_final_linked_shape','a_subclass_prototype_links_its_parents']:
  assert re.search(r'test .*'+subject+r' \.\.\. ok',text)
 if a=='entry':assert re.search(r'test .*collection_entry_discards_only_previously_idle_eden_pages_and_keeps_reuse_safe \.\.\. ok',text)
 products=load(E/(a+'-products.json'));assert set(products)=={'perry','libperry_runtime.a','libperry_stdlib.a'} and all(sha(E/a/n)==h for n,h in products.items())
 arms[a]=dict(tracked_inputs=count,products=products,commands=commands,runtime=dict(passed=passed,failed=failed,ignored=ignored),filtered_subprocess_summaries=summaries[:-1])
assert arms['gc']['runtime']['passed']==arms['base']['runtime']['passed']+20
assert arms['entry']['runtime']['passed']==arms['gc']['runtime']['passed']+1
result=dict(base=p['base'],runtime=p['runtime_head'],authored_runtime_paths=diffs,private_paths=private,arms=arms,scope='Fresh main07 source/actual product/full unfiltered runtime verification; application acceptance and private adoption unproven.')
(E/'independent-build-verification.json').write_text(json.dumps(result,indent=2)+'\n')
files=[x for x in E.rglob('*') if x.is_file() and (x.suffix in ['.json','.log'] or x.name=='complete.exit')]+[B/'gc-main07-v39-build-driver.exit',B/'gc-main07-v39-build-driver.log',pathlib.Path(__file__),B/'primary-main07-v39-build.py']
m=B/'main07-v39-build-evidence-files.json';m.write_text(json.dumps({str(x.relative_to(B)):sha(x) for x in files},indent=2)+'\n')
with tarfile.open(B/'main07-v39-build-evidence.tar.gz','w:gz') as t:
 for x in files+[m]:t.add(x,arcname=str(x.relative_to(B)))
print(json.dumps(result))
