from pathlib import Path
import gzip,hashlib,json,os,re,subprocess
w=Path(__file__).resolve().parent;bench=w.parents[1];d=w/'archive';sha=lambda b:hashlib.sha256(b).hexdigest()
window=json.loads((d/'window.json').read_text());assert window['quiet_gate_passed'] and window['finished_utc'] and not window['competing_workloads_before'] and not window['competing_workloads_after'];assert json.loads((d/'controller-exit.json').read_text())['exit_code']==0
for p,h in json.loads((d/'stage-hashes.json').read_text()).items():assert sha((bench/p).read_bytes())==h,p
raw=gzip.decompress((d/'source.patch.gz').read_bytes());pm=json.loads((d/'source.patch.json').read_text());assert sha(raw)==pm['original_sha256'] and sha((d/'source.patch.gz').read_bytes())==pm['gzip_sha256'] and raw==(w/'source.patch').read_bytes()
clean={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')};rows=[]
for row in json.loads((d/'profiles.json').read_text()):
 c=row['case'];label=c['fixture']+'-'+c['operation']+'-'+c['arm'];assert row['worker_exit']==row['sampler_exit']==0 and row['stop_reason'] is None and row['diagnostic_only'];assert row['worker_sha256']==sha((w/(c['arm']+'-worker')).read_bytes())
 assert subprocess.check_output(['/opt/homebrew/bin/node','--version'],text=True).strip()=='v26.5.1'
 cmd=['/opt/homebrew/bin/node',str(w/'worker.js'),str(bench/'.work/fixtures'/(c['fixture']+'.json')),c['operation'],str(c['iterations']),str(c['warmup']),'verify']
 oracle=subprocess.run(cmd,env=clean,capture_output=True,check=True);(d/(label+'.node-oracle.stdout')).write_bytes(oracle.stdout);(d/(label+'.node-oracle.stderr')).write_bytes(oracle.stderr)
 actual=(d/(label+'.stdout')).read_bytes()
 def significant(b):return b.split(b'\nVERIFY ',1)[1]
 assert significant(actual)==significant(oracle.stdout)
 result=list(map(float,re.search(rb'^RESULT (.+)$',actual,re.M)[1].split()))
 expected=list(map(float,re.search(rb'^RESULT (.+)$',oracle.stdout,re.M)[1].split()))
 assert result[5]==expected[5] and result[6]==0
 text=(d/(label+'.sample.txt')).read_text();graph=text.split('Call graph:',1)[1].split('Total number in stack',1)[0];nodes=[];stack=[]
 for line in graph.splitlines():
  m=re.match(r'^([ +!:|]*)(\d+) (.+)$',line)
  if not m:continue
  depth=len(m[1]);count=int(m[2]);name=m[3]
  while stack and nodes[stack[-1]]['depth']>=depth:stack.pop()
  nodes.append(dict(depth=depth,count=count,name=name,parent=stack[-1] if stack else None));stack.append(len(nodes)-1)
 def ancestors(i):
  while nodes[i]['parent'] is not None:i=nodes[i]['parent'];yield i
 def work(n):return 'perry_fn_worker_ts__run' in n
 def phase(pred):
  chosen=[i for i,n in enumerate(nodes) if pred(n['name']) and (work(n['name']) or any(work(nodes[a]['name']) for a in ancestors(i))) and not any(pred(nodes[a]['name']) for a in ancestors(i))]
  return dict(samples=sum(nodes[i]['count'] for i in chosen),frames=[nodes[i] for i in chosen])
 phases={'workload':phase(work),'parse_api':phase(lambda n:'js_json_parse' in n),'string_scan':phase(lambda n:'parse_string_bytes' in n),'string_value':phase(lambda n:'parse_string_value' in n),'number_parse':phase(lambda n:'parse_number' in n),'object_parse':phase(lambda n:'parse_object' in n),'root_scope':phase(lambda n:'RuntimeHandleScope' in n or 'parse_root_' in n),'construction_batch':phase(lambda n:'ConstructionBatch' in n or 'construction_batch' in n),'allocation':phase(lambda n:any(x in n for x in ['gc_alloc','gc_bump','string_storage_alloc','object_alloc'])),'collection':phase(lambda n:any(x in n for x in ['gc_safepoint','gc_collect','gc_budgeted']))}

 total=phases['workload']['samples'];assert total>=500,(c,total)
 for p in phases.values():p['pct_of_workload_samples']=p['samples']/total*100
 for i,n in enumerate(nodes):
  if not any(work(nodes[a]['name']) for a in ancestors(i)):continue
  n['self_samples']=n['count']-sum(child['count'] for child in nodes if child['parent']==i);assert n['self_samples']>=0
 leaves=sorted([n for n in nodes if n.get('self_samples',0)>0],key=lambda n:n['self_samples'],reverse=True)[:35]
 rows.append(dict(case=c,workload_samples=total,phases=phases,top_self_sample_nodes=leaves,profile_sha256=sha(text.encode()),oracle_command=cmd,oracle_output_sha256=sha(oracle.stdout),complete_workload_checksum_and_keep_match_node=True,diagnostic_only=True))
 print(c['fixture'],total,'workload samples', {k:round(v['pct_of_workload_samples'],2) for k,v in phases.items()})
 for n in leaves[:15]:print(n['self_samples'],n['name'])
(w/'analysis.json').write_text(json.dumps(dict(source_commit='e69d1292110cf9abee24f8690c584652805a4167',reference_source_commit='3aac4d6335da54abeeed73df842decbbe6dd5d71',diagnostic_only=True,note='Six frozen R26/R39 wide-object parse/stringify and large-record-object parse diagnostics; pinned Node executes the complete identical loop and warmup, and checksum plus every VERIFY/LAST/KEEP byte match. CPU/RSS of sampled processes is not benchmark evidence. Inclusive attribution overlaps and cannot be added.',cases=rows),indent=2)+'\n')
