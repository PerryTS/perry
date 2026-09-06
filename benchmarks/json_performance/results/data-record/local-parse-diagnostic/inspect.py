from pathlib import Path
import json,hashlib,importlib.util,random,statistics,subprocess,sys,time,os
root=Path('/Users/amlug/projects/perry/codex-json-fastpaths/benchmarks/json_performance');art=Path('/Users/amlug/projects/perry/codex-json-fastpaths-artifacts');out=art/'data-record-parse-diagnostic';out.mkdir()
sys.argv=['run.py'];spec=importlib.util.spec_from_file_location('json_runner',root/'run.py');r=importlib.util.module_from_spec(spec);spec.loader.exec_module(r)
engines=['unicode','tape-scan','data-record'];r.ENGINES={e:[str(art/(e+'-worker'))] for e in engines}
manifest={f['name']:f for f in r.FIXTURES};cases=[('numbers_1m',128),('heterogeneous_1m',128),('tiny_object',500000)]
meta={'purpose':'Local parse instruction/GC diagnostic, not qualified CPU/RSS measurements. Process-wide instructions include startup and warmup. GC diagnostic executions separately enable PERRY_GC_DIAG=1.', 'cases':cases,'cpu':subprocess.check_output(['sysctl','-n','machdep.cpu.brand_string'],text=True).strip(),'load':os.getloadavg(),'worker_sha256':{e:hashlib.sha256(Path(cmd[0]).read_bytes()).hexdigest() for e,cmd in r.ENGINES.items()}}
(out/'metadata.json').write_text(json.dumps(meta,indent=2)+'\n')
rng=random.Random(62387);rows=[]
for name,n in cases:
 f=manifest[name];assert hashlib.sha256((root/'.work/fixtures'/(name+'.json')).read_bytes()).hexdigest()==f['sha256']
 for rep in range(3):
  order=engines.copy();rng.shuffle(order)
  for e in order:
   row=r.one(e,f,'parse',n,5000 if f['bytes']<4096 else 2);row['rep']=rep;assert 'error' not in row and row.get('instructions',0)>0,row
   rows.append(row)
   with (out/'raw.jsonl').open('a') as file:file.write(json.dumps(row,separators=(',',':'))+'\n')
 print('CHECKED',name,flush=True)
summary=[]
for name,n in cases:
 counts={e:statistics.median(x['instructions'] for x in rows if x['fixture']==name and x['engine']==e) for e in engines}
 row={'fixture':name,'calls':n,'median_process_instructions':counts,'record_vs_unicode':counts['data-record']/counts['unicode'],'record_vs_tape':counts['data-record']/counts['tape-scan']};summary.append(row);print(row,flush=True)
(out/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
for e in engines:
 env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')};env['PERRY_GC_DIAG']='1'
 cmd=r.ENGINES[e]+[str(root/'.work/fixtures/numbers_1m.json'),'parse','128','2']
 p=subprocess.run(cmd,env=env,capture_output=True,text=True,timeout=90)
 (out/(e+'-gc.stdout')).write_text(p.stdout);(out/(e+'-gc.stderr')).write_text(p.stderr)
 assert p.returncode==0
 print('GC_DIAGNOSTIC',e,'exit',p.returncode,flush=True)
print('FINISHED',flush=True)
