from pathlib import Path
import hashlib,json,random,statistics,sys,time,os
import importlib.util
root=Path(__file__).resolve().parents[2]
spec=importlib.util.spec_from_file_location('rotating_runner',root/'run_rotating.py')
r=importlib.util.module_from_spec(spec);spec.loader.exec_module(r)
out=Path(sys.argv[1]);out.mkdir(parents=True,exist_ok=False)
engines={'perry':[str(root/'.work/source-length-r4/rotating-worker')],
         'baseline':[str(root/'.work/decoder-r2/rotating-worker')],
         'prior':[str(root/'.work/source-length-r3/rotating-worker')]}
node=['/opt/homebrew/bin/node',str(root/'rotating-worker.js')]
meta=dict(started_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),host=os.uname().nodename,
          workers={e:hashlib.sha256(Path(c[0]).read_bytes()).hexdigest() for e,c in engines.items()},
          runner_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
          run_rotating_sha256=hashlib.sha256((root/'run_rotating.py').read_bytes()).hexdigest(),
          cases=['small_record','object_1k'],iterations=3000000,warmup=5000,repetitions=9,
          node_version=__import__('subprocess').check_output([node[0],'--version'],text=True).strip())
rng=random.Random(10034);summaries=[]
def save(phase,row):
 with (out/(phase+'.jsonl')).open('a') as f:f.write(json.dumps(row)+'\n')
for fixture in meta['cases']:
 prefix=root/'.work/rotating'/fixture
 reference=r.run_one(node,prefix,'parse',7,8,'rotating',verify=True)
 assert not reference.get('error'),reference
 reference.update(engine='node',fixture=fixture);save('verify',reference)
 for e,cmd in engines.items():
  row=r.run_one(cmd,prefix,'parse',7,8,'rotating',verify=True)
  row['correct']=not row.get('error') and all(row[k]==reference[k] for k in ['checksum','verify_sha256','last_sha256','verified_members'])
  row.update(engine=e,fixture=fixture);save('verify',row)
  assert row['correct'],row
 rows=[]
 for rep in range(meta['repetitions']):
  order=list(engines);rng.shuffle(order)
  for e in order:
   row=r.run_one(engines[e],prefix,'parse',meta['iterations'],meta['warmup'],'rotating')
   assert not row.get('error') and row['checksum']==meta['iterations']+meta['warmup'] and row['retained']==0,row
   row.update(engine=e,fixture=fixture,rep=rep);save('timing',row);rows.append(row)
 for e in engines:
  sample=[row for row in rows if row['engine']==e]
  cpu=[(row['user_us']+row['system_us'])/row['iterations'] for row in sample]
  rss=[row['peak_rss']/1048576 for row in sample]
  summaries.append(dict(fixture=fixture,engine=e,cpu_us=statistics.median(cpu),cpu_samples_us=cpu,peak_rss_mib=statistics.median(rss),peak_rss_samples_mib=rss))
 print('PASS',fixture,flush=True)
meta['finished_utc']=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime())
(out/'metadata.json').write_text(json.dumps(meta,indent=2)+'\n')
(out/'summary.json').write_text(json.dumps(summaries,indent=2)+'\n')
