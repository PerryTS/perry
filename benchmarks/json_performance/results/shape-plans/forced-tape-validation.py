from pathlib import Path
import os,subprocess,json,re,hashlib
art=Path('/Users/amlug/projects/perry/codex-json-fastpaths-artifacts')
root=Path('/Users/amlug/projects/perry/codex-json-fastpaths');name='test_gap_json_lazy_growth_alias'
obj=art/(name+'.o');binary=art/('shape-plans-'+name)
with (art/'shape-plans-lazy-fixture-link.log').open('w') as log:
 subprocess.run(['cc',str(obj),str(art/'shape-plans-runtime/libperry_runtime.a'),'-lc','-Wl,-dead_strip','-Wl,-no_exported_symbols','-o',str(binary)],stdout=log,stderr=subprocess.STDOUT,check=True)
env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
node=subprocess.run(['/opt/homebrew/bin/node','--experimental-strip-types',str(root/'test-files'/(name+'.ts'))],env=env,capture_output=True,text=True,timeout=120)
assert node.returncode==0
rows=[]
for seed in [None,17,9013]:
 current=dict(env,PERRY_JSON_TAPE='1')
 if seed is not None:
  current.update(PERRY_GC_SCHEDULE_SEED=str(seed),PERRY_GC_SCHEDULE_RATE='0.1',PERRY_GC_PROTECT_FROMSPACE='1',PERRY_GC_VERIFY_EVACUATION='1')
 p=subprocess.run([str(binary)],env=current,capture_output=True,text=True,timeout=180)
 counts={k:int(v) for k,v in re.findall(r'(scheduled_collections|copying_minors|moved_objects|loop_polls)=(\d+)',p.stderr)}
 row={'fixture':name,'seed':seed,'tape':'forced','object_sha256':hashlib.sha256(obj.read_bytes()).hexdigest(),'exit':p.returncode,'stdout':p.stdout,'stderr':p.stderr,'node_stdout':node.stdout,'matches_node':p.returncode==0 and p.stdout==node.stdout,'counters':counts,'live_subject':seed is None or all(counts.get(k,0)>0 for k in ['scheduled_collections','copying_minors','moved_objects','loop_polls'])}
 rows.append(row);(art/'shape-plans-forced-tape-validation.json').write_text(json.dumps(rows,indent=2)+'\n')
 print(seed,row['matches_node'],row['live_subject'],counts,flush=True)
 assert row['matches_node'] and row['live_subject'],row
