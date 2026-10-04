import pathlib,json,hashlib,re
B=pathlib.Path('/root/rss-header-20261002');R=B/'primary-gc-runtime-v24';E=R/'export'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def load(p):return json.loads(p.read_text())
assert (E/'build-driver.exit').read_text().strip()=='0';control=load(E/'control-source-inputs.json');products={};tests={};diff={}
for a in ['control','weak','elf']:
 s=R/('source-'+a);inputs=load(E/(a+'-source-inputs.json'));assert all(sha(s/n)==h for n,h in inputs.items());d=[n for n in sorted(set(control)|set(inputs)) if control.get(n)!=inputs.get(n)];diff[a]=d
 assert d==([] if a=='control' else ['crates/perry-runtime/src/gc/verify.rs'] if a=='weak' else ['crates/perry-runtime/src/gc/roots/runtime_handles.rs'])
 for n,h in load(E/(a+'-products.json')).items():assert sha(E/a/n)==h;products[a+'/'+n]=h
 for row in load(E/(a+'-commands.json')):
  assert row['rc']==0 and sha(E/'logs'/(a+'-'+row['name']+'.log'))==row['log_sha256']
  if row['name']=='build':assert 'Compiling perry-runtime ' in (E/'logs'/(a+'-build.log')).read_text()
 if a!='control':
  log=(E/'logs'/(a+'-runtime-tests.log')).read_text();assert re.search(r'test result: ok\. 4856 passed; 0 failed; 5 ignored;',log);tests[a]=4856
assert len({products[a+'/libperry_runtime.a'] for a in ['control','weak','elf']})==3
proof=dict(base=load(E/'provenance.json')['base'],source_input_counts={a:len(load(E/(a+'-source-inputs.json'))) for a in ['control','weak','elf']},source_diffs_vs_current_gc_control=diff,products=products,linux_runtime_tests=tests,scope='All tracked archive Rust/Cargo inputs and live exported product/log hashes verified independently. Private one-file probes; no app acceptance inferred.')
(E/'independent-build-verification.json').write_text(json.dumps(proof,indent=2)+'\n');print(json.dumps(proof,indent=2))
