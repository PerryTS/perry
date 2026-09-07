from pathlib import Path
import json,subprocess,os,re,hashlib
art=Path('/Users/amlug/projects/perry/codex-json-fastpaths-artifacts');arm='record-bytes';source=Path('/Users/amlug/projects/perry/codex-json-fastpaths/test-files/test_gap_json_nested_records.ts');binary=art/(arm+'-extended-fixture');obj=art/'test_gap_json_nested_records.o'
with (art/(arm+'-extended-link.log')).open('w') as log:subprocess.run(['cc',str(obj),str(art/(arm+'-runtime/libperry_runtime.a')),'-lc','-Wl,-dead_strip','-Wl,-no_exported_symbols','-o',str(binary)],stdout=log,stderr=subprocess.STDOUT,check=True)
reference=json.loads((art/'nested-records-baseline-fixture.json').read_text());assert reference['node']['exit']==reference['reference']['exit']==0
expected=reference['reference']['stdout'];node=reference['node']['stdout'];rows=[]
for seed in [None,17,9013]:
 env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
 if seed is not None:env.update(PERRY_GC_SCHEDULE_SEED=str(seed),PERRY_GC_SCHEDULE_RATE='0.1',PERRY_GC_PROTECT_FROMSPACE='1',PERRY_GC_VERIFY_EVACUATION='1')
 p=subprocess.run([str(binary)],env=env,capture_output=True,text=True,timeout=180)
 counts={k:int(v) for k,v in re.findall(r'(scheduled_collections|copying_minors|moved_objects|loop_polls)=(\d+)',p.stderr)}
 a,b=p.stdout.splitlines(),node.splitlines();diff=[{'candidate':x,'node':y} for x,y in zip(a,b) if x!=y]
 row={'seed':seed,'exit':p.returncode,'stdout':p.stdout,'stderr':p.stderr,'counters':counts,'matches_reference':p.stdout==expected,'matches_node':p.stdout==node,'node_differences':diff,'known_preexisting_mismatch_only':len(a)==len(b) and len(diff)==1 and diff[0]['candidate'].startswith('global-prototype ') and diff[0]['node'].startswith('global-prototype '),'live_subject':seed is None or all(counts.get(k,0)>0 for k in ['scheduled_collections','copying_minors','moved_objects','loop_polls']),'object_sha256':hashlib.sha256(obj.read_bytes()).hexdigest()}
 rows.append(row);(art/(arm+'-extended-validation.json')).write_text(json.dumps(rows,indent=2)+'\n');print('EXTENDED',seed,p.returncode,row['matches_reference'],row['matches_node'],counts,flush=True)
 assert p.returncode==0 and row['live_subject'] and (row['matches_node'] or (row['matches_reference'] and row['known_preexisting_mismatch_only'])),row
