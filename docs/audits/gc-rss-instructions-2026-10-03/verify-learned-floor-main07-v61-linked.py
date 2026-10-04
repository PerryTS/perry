import pathlib,json,hashlib,re,subprocess,tarfile
B=pathlib.Path('/root/rss-header-20261002');R=B/'primary-main07-v39';E=R/'export';V='gc-learned-floor-main07-v61-linked'
exports={'lazy':B/'primary-lazy-rss-main07-v51/export','learned':B/'primary-learned-floor-main07-v59/export'}
trees={'lazy':B/'primary-lazy-rss-main07-v51/source','learned':B/'primary-learned-floor-main07-v59/source'}
def sha(p):
 with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
assert (B/(V+'.exit')).read_text().strip()=='0' and (B/(V+'-driver.exit')).read_text().strip()=='0'
rows=json.loads((B/(V+'-runs.json')).read_text());products=json.loads((B/(V+'-products.json')).read_text());assert len(rows)==66 and len(products)==6
for p,h in products.items():assert sha(p)==h
for a in ['lazy','learned']:
 manifest=json.loads((exports[a]/'source-inputs.json').read_text());assert len(manifest)==5011
 assert all(sha(trees[a]/n)==h for n,h in manifest.items())
 expected=json.loads((exports[a]/'products.json').read_text());assert all(products[str(exports[a]/n)]==h for n,h in expected.items())
node='/opt/node-v26.5.1-linux-x64/bin/node';assert subprocess.check_output([node,'--version'],text=True).strip()=='v26.5.1'
cases=sorted({x['case'] for x in rows});assert len(cases)==11
files=set();oracles={}
for c in cases:
 builds=[x for x in rows if x['case']==c and x['mode']=='build'];assert {x['arm'] for x in builds}=={'lazy','learned'} and len(builds)==2
 src=pathlib.Path(builds[0]['cmd'][2]);assert all(x['source_sha256']==sha(src) for x in builds)
 oracle=subprocess.run([node,'--expose-gc',str(src)],text=True,capture_output=True,check=True);oracles[c]=oracle.stdout
 for ext in ['out','err']:files.add(B/'logs'/(V+'-node-'+c+'.'+ext))
 assert (B/'logs'/(V+'-node-'+c+'.out')).read_text()==oracle.stdout
 for build in builds:
  assert build['rc']==0 and build['reason'] is None and sha(build['cmd'][0])==products[str(exports[build['arm']]/'perry')]
  assert build['env']['PERRY_RUNTIME_DIR']==str(exports[build['arm']]) and build['env']['PERRY_WORKSPACE_ROOT']==str(trees[build['arm']])
  assert build['env']['PERRY_GC_INSTRUMENTS']=='1'
  binary=pathlib.Path(build['cmd'][-1]);digest=sha(binary)
  runs=[x for x in rows if x['case']==c and x['arm']==build['arm'] and x['mode']!='build'];assert {x['mode'] for x in runs}=={'default','moving'} and len(runs)==2
  for row in runs:
   assert row['rc']==0 and row['reason'] is None and row['correct'] and row['stdout']==oracle.stdout and row['binary_sha256']==digest
   assert row['cmd']==[str(binary)]
   if row['mode']=='moving':
    lines=(B/'logs'/(row['label']+'.err')).read_text().splitlines();cycles=[json.loads(x) for x in lines if x.startswith('{')]
    copied=sum(x.get('copying_nursery',{}).get('copied_objects',0) for x in cycles);protected=any(x.startswith('[gc-fromspace-protect]') and 'retired_set=' in x for x in lines)
    assert copied==row['copied_objects'] and copied>0 and row['protected']==protected and protected
for row in rows:
 for ext in ['out','err']:files.add(B/'logs'/(row['label']+'.'+ext))
 assert (B/'logs'/(row['label']+'.out')).read_text()==row['stdout']
derived=json.loads((B/(V+'-derived-source.json')).read_text());original=R/'source-gc/test-files/test_gap_class_expr_fresh_static_blocks_this.ts';src=B/(V+'-derived-sources')/original.name
assert sha(original)==derived['original_source_sha256'] and sha(src)==derived['derived_source_sha256']
text=original.read_text()
for statement in ['const B = makeExpr("B");','(globalThis as any).__IDENT = I;','const D = makeDecl("D");']:
 assert text.count(statement)==1;text=text.replace(statement,statement+'\ngc();')
assert src.read_text()=='declare function gc(): void;\n'+text
assert subprocess.check_output([node,'--expose-gc',str(original)],text=True)==oracles[original.stem]
proof=dict(base='07e50b14a3d23e8f914193ec4d5a679adbd7c37a',production_runtime='4f9121c09b642a007e592c1d91cbe86d5d122851',private_learned_floor='3fb6cd50ed',private_control='d4f491673b',tracked_source_counts={'lazy':5011,'learned':5011},linked_executions=44,actual_moving_protected=22,cases=cases,all_raw_sources_products_oracles_counters_verified=True,inputs={n:sha(B/(V+'-'+n+'.json')) for n in ['runs','products','derived-source']})
(B/(V+'-verification.json')).write_text(json.dumps(proof,indent=2)+'\n')
files.update([B/(V+'-'+n+'.json') for n in ['runs','products','derived-source','verification']]);files.update([B/(V+'.exit'),B/(V+'-driver.exit'),B/(V+'-driver.log'),pathlib.Path(__file__),src])
for ext in ['out','err']:files.add(B/'logs'/(V+'-node-original-'+original.stem+'.'+ext))
manifest=B/(V+'-evidence-files.json');manifest.write_text(json.dumps({str(p.relative_to(B)):sha(p) for p in sorted(files)},indent=2)+'\n')
with tarfile.open(B/(V+'-evidence.tar.gz'),'w:gz') as t:
 for p in sorted(files|{manifest}):t.add(p,arcname=str(p.relative_to(B)))
print(json.dumps(proof,indent=2))
