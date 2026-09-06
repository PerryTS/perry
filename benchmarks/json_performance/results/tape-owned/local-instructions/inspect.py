from pathlib import Path
import json,hashlib,importlib.util,random,statistics,subprocess,sys,time,os
root=Path('/Users/amlug/projects/perry/codex-json-fastpaths/benchmarks/json_performance');art=Path('/Users/amlug/projects/perry/codex-json-fastpaths-artifacts');out=art/'tape-owned-local-instructions';out.mkdir()
sys.argv=['run.py'];spec=importlib.util.spec_from_file_location('json_runner',root/'run.py');r=importlib.util.module_from_spec(spec);spec.loader.exec_module(r)
engines=['integer-piece','tape-owned'];r.ENGINES={e:[str(art/(e+'-worker'))] for e in engines}
manifest={f['name']:f for f in r.FIXTURES};cases=[('small_record','stringify',500000),('tiny_object','stringify',2000000),('null','stringify',2000000),('escaped_1m','stringify',84),('wide_1m','stringify',128),('records_object_1m','stringify',128),('heterogeneous_1m','stringify',128),('records_array_1m','parse',95),('heterogeneous_1m','parse',78),('small_record','parse',500000),('numbers_1m','stringify',96),('object_1k','stringify',500000),('records_array_8m','parse',10),('numbers_1m','parse',88),('records_object_20m','parse',3)]
meta={'purpose':'Local instruction-count diagnostic only; no quiet CPU/RSS acceptance. Process-wide instructions include startup and warmup, unlike loop CPU. Identical fixed calls and worker.o across Perry arms.','utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'cpu':subprocess.check_output(['sysctl','-n','machdep.cpu.brand_string'],text=True).strip(),'load':os.getloadavg(),'cases':cases,'worker_sha256':{e:hashlib.sha256(Path(cmd[0]).read_bytes()).hexdigest() for e,cmd in r.ENGINES.items()}}
(out/'metadata.json').write_text(json.dumps(meta,indent=2)+'\n')
rng=random.Random(52691);rows=[]
for name,op,n in cases:
 f=manifest[name];assert hashlib.sha256((root/'.work/fixtures'/(name+'.json')).read_bytes()).hexdigest()==f['sha256']
 for rep in range(3):
  order=engines.copy();rng.shuffle(order)
  for e in order:
   row=r.one(e,f,op,n,5000 if f['bytes']<4096 else 2);row['rep']=rep;assert 'error' not in row and row.get('instructions',0)>0,row
   rows.append(row)
   with (out/'raw.jsonl').open('a') as file:file.write(json.dumps(row,separators=(',',':'))+'\n')
 print('CHECKED',name,flush=True)
summary=[]
for name,op,n in cases:
 counts={e:statistics.median(x['instructions'] for x in rows if x['fixture']==name and x['operation']==op and x['engine']==e) for e in engines}
 row={'fixture':name,'operation':op,'calls':n,'median_process_instructions':counts,'candidate_vs_reference':counts['tape-owned']/counts['integer-piece']};summary.append(row);print(row,flush=True)
(out/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
