from pathlib import Path
import gzip,hashlib,json,os,re,subprocess
w=Path(__file__).resolve().parent;bench=w.parents[1];out=[]
clean={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
def read_profile(p):
 return p.read_text() if p.exists() else gzip.decompress(p.with_name(p.name+'.gz').read_bytes()).decode()
for suffix in ['access-profiles']:
 d=bench/'results'/('quiet-'+w.name+'-'+suffix);window=json.loads((d/'window.json').read_text());assert window['quiet_gate_passed'] and window['finished_utc'] and not window['competing_workloads_before'] and not window['competing_workloads_after'];assert json.loads((d/'controller-exit.json').read_text())['exit_code']==0
 stage_name='access-profile-stage-hashes.json'
 for p,h in json.loads((d/stage_name).read_text()).items():assert hashlib.sha256((bench/p).read_bytes()).hexdigest()==h,p
 patch=d/'source.patch';raw=patch.read_bytes() if patch.exists() else gzip.decompress(patch.with_suffix('.patch.gz').read_bytes());assert raw==(w/'source.patch').read_bytes()
 if not patch.exists():
  meta=json.loads(patch.with_suffix('.patch.json').read_text());assert meta['original_sha256']==hashlib.sha256(raw).hexdigest() and meta['gzip_sha256']==hashlib.sha256(patch.with_suffix('.patch.gz').read_bytes()).hexdigest()
 for row in json.loads((d/'profiles.json').read_text()):
  c=row['case'];label=c['fixture']+'-'+c['operation']+'-'+c['mode'];assert row['worker_exit']==row['sampler_exit']==0 and row['stop_reason'] is None;assert row['worker_sha256']==hashlib.sha256((w/(c['mode']+'-access-worker')).read_bytes()).hexdigest()
  cmd=['/opt/homebrew/bin/node',str(w/'harness/access-worker.js'),str(bench/'.work/fixtures'/(c['fixture']+'.json')),c['operation'],str(c['iterations']),str(c['warmup'])]
  oracle_path=d/(label+'.node-oracle.stdout');oracle_reused=oracle_path.exists()
  if not oracle_reused:oracle_path.write_bytes(subprocess.run(cmd,capture_output=True,env=clean,check=True).stdout)
  oracle_output=oracle_path.read_bytes()
  def result(raw):return list(map(float,re.search(r'^RESULT (.+)$',raw,re.M)[1].split()))
  actual=result((d/(label+'.stdout')).read_text());expected=result(oracle_output.decode());assert actual[5:]==expected[5:] and actual[6]==0
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
  phases={'workload':phase(work),'array_access':phase(lambda n:'js_array_get_f64 ' in n),'lazy_access':phase(lambda n:'9json_tape8lazy_get ' in n or 'cached_read8lazy_get ' in n or '15lazy_get_rooted ' in n), 'packed_index':phase(lambda n:'js_packed_arraylike_index_get ' in n), 'generic_index':phase(lambda n:'js_dyn_index_get ' in n), 'named_property_helpers':phase(lambda n:'js_object_get_field' in n), 'fmod':phase(lambda n:n.startswith('fmod ')),'root_scope_helpers':phase(lambda n:'RuntimeHandleScope' in n),'resolve_materialized':phase(lambda n:'resolve_materialized_array' in n)}
  total=phases['workload']['samples'];assert total>0
  assert total>=500,total
  for p in phases.values():p['pct_of_workload_samples']=p['samples']/total*100
  for i,n in enumerate(nodes):
   if not any(work(nodes[a]['name']) for a in ancestors(i)):continue
   n['self_samples']=n['count']-sum(child['count'] for child in nodes if child['parent']==i)
   assert n['self_samples']>=0,n
  leaves=sorted([n for n in nodes if n.get('self_samples',0)>0],key=lambda n:n['self_samples'],reverse=True)[:30]
  rec={'top_self_sample_nodes':leaves,'case':c,'window':suffix,'workload_samples':total,'coverage_qualified':total>=500,'oracle_command':cmd,'oracle_output_sha256':hashlib.sha256(oracle_output).hexdigest(),'oracle_output_reused_from_prior_verified_analysis':oracle_reused,'checksum_matches_node':True,'phases':phases,'profile_sha256':hashlib.sha256(text.encode()).hexdigest(),'note':'Instrumented diagnostic with at least 500 workload samples. Inclusive phases overlap and must not be added. CPU/RSS from sampled processes is not used as benchmark evidence. Inlined property reads remain in the generated workload frame; zero named-helper samples does not mean zero property-read cost.'};out.append(rec);print(c['fixture'],c['mode'],suffix,total,{k:round(v['pct_of_workload_samples'],2) for k,v in phases.items()},flush=True)
(w/'access-profiles-analysis.json').write_text(json.dumps({'diagnostic_only':True,'cases':out},indent=2)+'\n')
