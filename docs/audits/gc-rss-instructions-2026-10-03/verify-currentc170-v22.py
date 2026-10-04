import pathlib,json,hashlib,re
B=pathlib.Path(__file__).resolve().parent;Q=B/'qb6-currentc170-build-v22';W=pathlib.Path('/Users/amlug/projects/perry/rss-gc-runtime-20261003')
def sha(p):return hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
def load(p):return json.loads(p.read_text())
i=load(Q/'inputs.json');p=load(B/'provenance-gc-runtime-currentc170-v22.json');assert i['provenance']==p
assert len(p['files'])==22
for n,h in p['files'].items():assert sha(W/n)==h
for n,h in i['archives'].items():assert sha(B/n)==h
for n in ['build-driver.exit','base-build.exit','gc-build.exit','base-complete.exit','gc-complete.exit','linked.exit','qb6-currentc170-real-builds.exit','qb6-currentc170-auto-builds.exit']:assert (Q/n).read_text().strip()=='0'
products={};logs={}
for a in ['base','gc']:
 for n,h in load(Q/(a+'-products.json')).items():assert sha(Q/('currentc170-'+a)/n)==h;products[a+'/'+n]=h
 for r in load(Q/(a+'-commands.json')):
  n=a+'-'+r['label']+'.log';assert r['exit_code']==0 and sha(Q/'logs'/n)==r['log_sha256'];logs[n]=r['log_sha256']
for n,num in [('runtime',4856),('ffi',75),('events',15)]:
 s=(Q/'logs'/('gc-'+n+'-tests.log')).read_text();assert re.search(r'test result: ok\. '+str(num)+r' passed; 0 failed;',s)
rows=load(Q/'gc-qb6-currentc170-v22-linked-runs.json');runs=[r for r in rows if r['mode']!='build'];builds=[r for r in rows if r['mode']=='build'];assert len(runs)==44 and len(builds)==22
assert len({(r['case'],r['arm'],r['mode']) for r in runs})==44
for r in rows:
 assert r['rc']==0 and r['reason'] is None
 if r['mode']=='build':
  src=W/'test-files'/(r['case']+'.ts')
  if r['case']=='test_gap_class_expr_fresh_static_blocks_this':src=Q/'derived-sources'/src.name
  assert sha(src)==r['source_sha256']
 else:
  assert r['correct'] and sha(Q/'bin'/pathlib.Path(r['cmd'][0]).name)==r['binary_sha256']
  oracle=(Q/'logs'/('gc-qb6-currentc170-v22-linked-node-'+r['case']+'.out')).read_text();assert r['stdout']==oracle
  if r['mode']=='moving':
   s=(Q/'logs'/(r['label']+'.err')).read_text();cycles=[json.loads(l) for l in s.splitlines() if l.startswith('{')];copied=sum(c.get('copying_nursery',{}).get('copied_objects',0) for c in cycles)
   assert copied==r['copied_objects'] and copied>0 and r['protected'] and any(l.startswith('[gc-fromspace-protect]') and 'retired_set=' in l for l in s.splitlines())
completion=load(Q/'gc-auto-qb6-currentc170-v22-completion.json');assert completion['build_records']==completion['successful']==30 and completion['failed_sources']==[]
result=dict(base=p['base'],source_head=p['head'],host='qb6',inputs_sha256=sha(Q/'inputs.json'),products=products,logs=logs,tests=dict(runtime=4856,ffi=75,events=15),linked=dict(executions=44,default=22,moving=22,all_copied_and_protected=True,manifest_sha256=sha(Q/'gc-qb6-currentc170-v22-linked-runs.json')),auto_build_coverage=completion,performance_acceptance=False)
(B/'qb6-currentc170-build-v22-verification.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
