from pathlib import Path
import json,hashlib,importlib.util,random,statistics,subprocess,sys,time,os
root=Path('/Users/amlug/projects/perry/codex-json-fastpaths/benchmarks/json_performance');art=Path('/Users/amlug/projects/perry/codex-json-fastpaths-artifacts');out=art/'bounded-preflight-local-instructions';out.mkdir()
sys.argv=['run.py'];spec=importlib.util.spec_from_file_location('json_runner',root/'run.py');r=importlib.util.module_from_spec(spec);spec.loader.exec_module(r)
engines=['primitive-object','bounded-preflight'];r.ENGINES={e:[str(art/(e+'-worker'))] for e in engines}
manifest={f['name']:f for f in r.FIXTURES};cases=[('wide_1m',128),('heterogeneous_1m',128),('small_record',500000),('records_object_1m',128),('escaped_1m',128),('tiny_object',2000000)]
meta={'purpose':'Local instruction-count diagnostic only; no quiet CPU/RSS acceptance. Process-wide instructions include startup and warmup, unlike loop CPU. Identical fixed calls and worker.o across Perry arms.','utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'cpu':subprocess.check_output(['sysctl','-n','machdep.cpu.brand_string'],text=True).strip(),'load':os.getloadavg(),'cases':cases,'worker_sha256':{e:hashlib.sha256(Path(cmd[0]).read_bytes()).hexdigest() for e,cmd in r.ENGINES.items()}}
(out/'metadata.json').write_text(json.dumps(meta,indent=2)+'\n')
rng=random.Random(52691);rows=[]
for name,n in cases:
 f=manifest[name];assert hashlib.sha256((root/'.work/fixtures'/(name+'.json')).read_bytes()).hexdigest()==f['sha256']
 for rep in range(3):
  order=engines.copy();rng.shuffle(order)
  for e in order:
   row=r.one(e,f,'stringify',n,5000 if f['bytes']<4096 else 2);row['rep']=rep;assert 'error' not in row and row.get('instructions',0)>0,row
   rows.append(row)
   with (out/'raw.jsonl').open('a') as file:file.write(json.dumps(row,separators=(',',':'))+'\n')
 print('CHECKED',name,flush=True)
summary=[]
for name,n in cases:
 counts={e:statistics.median(x['instructions'] for x in rows if x['fixture']==name and x['engine']==e) for e in engines}
 row={'fixture':name,'calls':n,'median_process_instructions':counts,'candidate_vs_reference':counts['bounded-preflight']/counts['primitive-object']};summary.append(row);print(row,flush=True)
(out/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
