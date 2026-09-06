from pathlib import Path
import importlib.util,json,random,sys,hashlib,time,statistics
root=Path.cwd();sys.argv=['run.py']
spec=importlib.util.spec_from_file_location('runner_retention',root/'run.py');r=importlib.util.module_from_spec(spec);spec.loader.exec_module(r)
r.ENGINES['baseline']=[str(root/'.work/baseline-worker')]
f=next(f for f in r.FIXTURES if f['name']=='empty_object')
out=root/'results/retained-empty';out.mkdir()
meta={'utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'purpose':'Additional empty-object retained-output check; same application object, input, default GC and counts for all four engines. RSS is whole-process. Final output verification occurs after current-RSS reading.', 'worker_sha256':{e:hashlib.sha256(Path(r.ENGINES[e][0]).read_bytes()).hexdigest() for e in ['baseline','perry']}}
(out/'metadata.json').write_text(json.dumps(meta,indent=2)+'\n')
rng=random.Random(11624);rows=[]
for count in [1,200000]:
 for rep in range(3):
  order=list(r.ENGINES);rng.shuffle(order)
  for engine in order:
   row=r.one(engine,f,'retain-parse',count,0,verify=True);row['rep']=rep
   assert 'error' not in row and row['exit_code']==0 and row['retained']==count,row
   assert row['verify_sha256']==hashlib.sha256(b'{}').hexdigest(),row
   rows.append(row)
   with (out/'raw.jsonl').open('a') as file:file.write(json.dumps(row,separators=(',',':'))+'\n')
summary=[]
for count in [1,200000]:
 for engine in r.ENGINES:
  samples=[x for x in rows if x['engine']==engine and x['iterations']==count]
  summary.append({'engine':engine,'retained':count,'rss_after_mib':statistics.median(x['rss_after'] for x in samples)/1048576,'peak_rss_mib':statistics.median(x['peak_rss'] for x in samples)/1048576})
(out/'summary.json').write_text(json.dumps(summary,indent=2)+'\n');print('Retained empty-object checks complete',len(rows),flush=True)
