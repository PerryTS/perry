from pathlib import Path
import gzip,hashlib,json,os,re,subprocess
w=Path(__file__).resolve().parent;bench=w.parents[1];out=[]
clean={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
def read_profile(p):
 return p.read_text() if p.exists() else gzip.decompress(p.with_name(p.name+'.gz').read_bytes()).decode()
for suffix in ['main-profiles','main-profiles-recheck']:
 d=bench/'results'/('quiet-'+w.name+'-'+suffix);window=json.loads((d/'window.json').read_text());assert window['quiet_gate_passed'] and window['finished_utc'] and not window['competing_workloads_before'] and not window['competing_workloads_after'];assert json.loads((d/'controller-exit.json').read_text())['exit_code']==0
 stage_name='main-profile-recheck-stage-hashes.json' if suffix.endswith('recheck') else 'main-profile-stage-hashes.json'
 for p,h in json.loads((d/stage_name).read_text()).items():assert hashlib.sha256((bench/p).read_bytes()).hexdigest()==h,p
 for row in json.loads((d/'profiles.json').read_text()):
  c=row['case'];label=c['fixture']+'-'+c['operation']+'-'+c['mode'];assert row['worker_exit']==row['sampler_exit']==0 and row['stop_reason'] is None;assert row['worker_sha256']==hashlib.sha256((w/'main-access-worker').read_bytes()).hexdigest()
  cmd=['/opt/homebrew/bin/node',str(w/'harness/access-worker.js'),str(bench/'.work/fixtures'/(c['fixture']+'.json')),c['operation'],str(c['iterations']),str(c['warmup'])]
  oracle=subprocess.run(cmd,capture_output=True,env=clean,check=True);(d/(label+'.node-oracle.stdout')).write_bytes(oracle.stdout)
  def result(raw):return list(map(float,re.search(r'^RESULT (.+)$',raw,re.M)[1].split()))
  actual=result((d/(label+'.stdout')).read_text());expected=result(oracle.stdout.decode());assert actual[5:]==expected[5:] and actual[6]==0
  text=read_profile(d/(label+'.sample.txt'));graph=text.split('Call graph:',1)[1].split('Total number in stack',1)[0];nodes=[];stack=[]
  for line in graph.splitlines():
   m=re.match(r'^([ +!:|]*)(\d+) (.+)$',line)
   if not m:continue
   depth=len(m[1]);count=int(m[2]);name=m[3]
   while stack and nodes[stack[-1]]['depth']>=depth:stack.pop()
   nodes.append({'depth':depth,'count':count,'name':name,'parent':stack[-1] if stack else None});stack.append(len(nodes)-1)
  def ancestors(i):
   while nodes[i]['parent'] is not None:
    i=nodes[i]['parent'];yield i
  def work(n):return 'perry_fn_access_worker_ts__run' in n
  def phase(pred):
   selected=[i for i,n in enumerate(nodes) if pred(n['name']) and (work(n['name']) or any(work(nodes[a]['name']) for a in ancestors(i))) and not any(pred(nodes[a]['name']) for a in ancestors(i))]
   return {'samples':sum(nodes[i]['count'] for i in selected),'frames':[nodes[i] for i in selected]}
  phases={'workload':phase(work),'array_access':phase(lambda n:'js_array_get_f64 ' in n),'lazy_access':phase(lambda n:'9json_tape8lazy_get ' in n),'root_scope_helpers':phase(lambda n:'RuntimeHandleScope' in n),'resolve_materialized':phase(lambda n:'resolve_materialized_array' in n)}
  total=phases['workload']['samples'];assert total>0
  if suffix.endswith('recheck') or c['fixture']=='records_array_1m':assert total>=500,total
  for p in phases.values():p['pct_of_workload_samples']=p['samples']/total*100
  rec={'case':c,'window':suffix,'workload_samples':total,'coverage_qualified':total>=500,'oracle_command':cmd,'checksum_matches_node':True,'phases':phases,'profile_sha256':hashlib.sha256(text.encode()).hexdigest(),'note':'Instrumented diagnostic. Inclusive phases overlap and must not be added. Initial 16k sample includes startup and has insufficient workload coverage.'};out.append(rec);print(c['fixture'],suffix,total,{k:round(v['pct_of_workload_samples'],2) for k,v in phases.items()},flush=True)
(w/'main-profiles-analysis.json').write_text(json.dumps({'diagnostic_only':True,'cases':out},indent=2)+'\n')
