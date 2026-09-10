from pathlib import Path
import gzip,hashlib,json,re
w=Path(__file__).resolve().parent;bench=w.parents[1];d=bench/'results'/('quiet-'+w.name+'-profiles');read=lambda n:json.loads((d/n).read_text());sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
def read_text(path):
 return path.read_text() if path.exists() else gzip.decompress(path.with_name(path.name+'.gz').read_bytes()).decode()
window=read('window.json');assert window['quiet_gate_passed'] and window.get('finished_utc');assert not window['competing_workloads_before'] and not window['competing_workloads_after'];assert 'results/'+d.name in window['command'];assert read('controller-exit.json')['exit_code']==0
staged=read('remote-profile-stage-hashes.json');assert staged==json.loads((w/'remote-profile-stage-hashes.json').read_text()) and len(staged)==18
for path,digest in staged.items():assert sha(bench/path)==digest,path
oracle={}
for line in (bench/'results'/('quiet-'+w.name+'-modes')/'verify.jsonl').read_text().splitlines():
 r=json.loads(line)
 if r['engine']=='oracle':oracle[r['fixture'],r['operation']]=r
rows=read('profiles.json');assert len(rows)==4;out=[]
for r in rows:
 c=r['case'];label=c['fixture']+'-'+c['operation']+'-'+c['mode'];assert r['worker_exit']==r['sampler_exit']==0 and r['stop_reason'] is None;assert r['worker_sha256']==sha(w/'main-worker');assert r['worker_command'][-1]=='verify';assert r['observed_peak_rss_bytes']<=2*1024**3
 stdout=read_text(d/(label+'.stdout'));values=list(map(float,re.search(r'^RESULT (.+)$',stdout,re.M)[1].split()));assert len(values)==7;expected=oracle[c['fixture'],c['operation']];assert values[5]==expected['checksum']/15*(c['iterations']+c['warmup']) and values[6]==0
 verified=stdout.split('\nVERIFY ',1)[1].rsplit('\nKEEP ',1)[0];assert hashlib.sha256(verified.encode()).hexdigest()==expected['verify_sha256']
 report=read_text(d/(label+'.sample.txt'));graph=report.split('Call graph:',1)[1].split('Total number in stack',1)[0];nodes=[];stack=[]
 for line in graph.splitlines():
  m=re.match(r'^([ +!:|]*)(\d+) (.+)$',line)
  if not m:continue
  depth=len(m[1]);name=m[3];count=int(m[2])
  while stack and nodes[stack[-1]]['depth']>=depth:stack.pop()
  parent=stack[-1] if stack else None;nodes.append({'depth':depth,'count':count,'name':name,'parent':parent});stack.append(len(nodes)-1)
 assert nodes and 'Thread_' in nodes[0]['name'];total=nodes[0]['count'];assert total>=500,total
 # Inclusive, non-overlapping subtrees for each explicitly named phase.
 def phase(predicate):
  selected=[]
  for i,node in enumerate(nodes):
   if not predicate(node['name']):continue
   parent=node['parent'];nested=False
   while parent is not None:
    if predicate(nodes[parent]['name']):nested=True;break
    parent=nodes[parent]['parent']
   if not nested:selected.append(i)
  return {'samples':sum(nodes[i]['count'] for i in selected),'frames':[nodes[i] for i in selected]}
 phases={'tape_build':phase(lambda n:'build_tape_into' in n),'lazy_full_materialization':phase(lambda n:'force_materialize_lazy' in n),'direct_array_parser':phase(lambda n:'DirectParser11parse_array ' in n),'collection':phase(lambda n:'gc_collect_full_mark_sweep_with_trigger' in n or 'gc_collect_minor_with_trigger_inner' in n)}
 for value in phases.values():assert value['samples']<=total;value['pct_of_main_thread_samples']=value['samples']/total*100
 out.append({'case':c,'samples':total,'profile_sha256':hashlib.sha256(report.encode()).hexdigest(),'full_output_matches_node':True,'phases':phases,'observed_peak_rss_mib':r['observed_peak_rss_bytes']/1048576,'note':'One short instrumented sample, not an isolated phase timer. Direct parser is nested under full materialization in lazy scans; those columns overlap and must not be added.'})
 print(label,total,{k:round(v['pct_of_main_thread_samples'],2) for k,v in phases.items()},flush=True)
(w/'profiles-analysis.json').write_text(json.dumps({'window':window,'diagnostic_only':True,'instrumented':True,'cases':out},indent=2)+'\n')
print('VERIFIED four worker outputs against Node, positive sampling coverage, all 18 hashes and quiet window.',flush=True)
