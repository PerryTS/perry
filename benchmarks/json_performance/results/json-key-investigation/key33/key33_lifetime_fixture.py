from pathlib import Path
import os,subprocess,json,re,hashlib
art=Path('/Users/amlug/projects/perry/codex-json-fastpaths-artifacts')
root=Path('/Users/amlug/projects/perry/codex-json-gc-deferral')
source=root/'test-files/test_gap_json_gc_deferral.ts'
binary=art/'key33-lifetime-fixture';obj=art/'test_gap_json_gc_deferral.o'
with (art/'key33-lifetime-fixture-link.log').open('w') as log:
 subprocess.run(['cc',str(obj),str(art/'key33-runtime/libperry_runtime.a'),'-lc','-Wl,-dead_strip','-Wl,-no_exported_symbols','-o',str(binary)],check=True,stdout=log,stderr=subprocess.STDOUT)
env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
node=subprocess.run(['/opt/homebrew/bin/node','--experimental-strip-types',str(source)],env=env,capture_output=True,text=True,timeout=120)
assert node.returncode==0
configs=[{}, {'PERRY_GC_HEAP_LIMIT':'32'}, {'PERRY_JSON_TAPE':'0'}, {'PERRY_GEN_GC':'0'}, {'PERRY_GC_MOVING_LOOP_POLLS':'0'}]
for seed in [17,9013]:
 configs.append(dict(PERRY_GC_SCHEDULE_SEED=str(seed),PERRY_GC_SCHEDULE_RATE='0.1',PERRY_GC_PROTECT_FROMSPACE='1',PERRY_GC_VERIFY_EVACUATION='1'))
 configs.append(dict(PERRY_GC_HEAP_LIMIT='32',PERRY_JSON_TAPE='0',PERRY_GC_SCHEDULE_SEED=str(seed),PERRY_GC_SCHEDULE_RATE='0.1',PERRY_GC_PROTECT_FROMSPACE='1',PERRY_GC_VERIFY_EVACUATION='1'))
rows=[]
for config in configs:
 p=subprocess.run([str(binary)],env=dict(env,**config),capture_output=True,text=True,timeout=180)
 counts={k:int(v) for k,v in re.findall(r'(scheduled_collections|copying_minors|moved_objects|loop_polls)=(\d+)',p.stderr)}
 row=dict(config=config,exit=p.returncode,stdout=p.stdout,stderr=p.stderr,node_stdout=node.stdout,matches_node=p.returncode==0 and p.stdout==node.stdout,counters=counts,source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),object_sha256=hashlib.sha256(obj.read_bytes()).hexdigest())
 if 'PERRY_GC_SCHEDULE_SEED' in config:row['live_subject']=all(counts.get(k,0)>0 for k in ['scheduled_collections','copying_minors','moved_objects','loop_polls'])
 rows.append(row);(art/'key33-lifetime-fixture-validation.json').write_text(json.dumps(rows,indent=2)+'\n')
 print(config,row['matches_node'],counts,flush=True)
 assert row['matches_node'] and row.get('live_subject',True),row
