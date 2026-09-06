from pathlib import Path
import json,hashlib,importlib.util,random,statistics,subprocess,sys,time,os
root=Path('/Users/amlug/projects/perry/codex-json-fastpaths/benchmarks/json_performance');art=Path('/Users/amlug/projects/perry/codex-json-fastpaths-artifacts');out=art/'short-array-local-instructions';out.mkdir()
sys.argv=['run.py'];spec=importlib.util.spec_from_file_location('json_runner',root/'run.py');r=importlib.util.module_from_spec(spec);spec.loader.exec_module(r)
engines=['inline-object','short-array'];r.ENGINES={e:[str(art/(e+'-worker'))] for e in engines}
manifest={f['name']:f for f in r.FIXTURES};cases=[('small_record','stringify',500000),('tiny_object','parse',2000000),('object_1k','stringify',500000),('long_string_1m','stringify',1100),('unicode_1m','stringify',1400),('escaped_1m','stringify',84),('wide_1m','stringify',216),('numbers_1m','parse',92),('empty_object','parse',2000000),('small_record','parse',500000)]
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
 row={'fixture':name,'operation':op,'calls':n,'median_process_instructions':counts,'candidate_vs_reference':counts['short-array']/counts['inline-object']};summary.append(row);print(row,flush=True)
(out/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
