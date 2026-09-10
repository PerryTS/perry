from pathlib import Path
from collections import defaultdict
import gzip,hashlib,json,statistics
work=Path(__file__).resolve().parent;bench=work.parents[1]
outputs={};total=0
for kind in ['access','focus','options']:
 d=bench/'results'/('quiet-'+work.name+'-'+kind)
 timings=[json.loads(x) for x in (d/'timing.jsonl').read_text().splitlines()]
 verifies=[json.loads(x) for x in (d/'verify.jsonl').read_text().splitlines()]
 summary=json.loads((d/'summary.json').read_text());window=json.loads((d/'window.json').read_text())
 assert window['quiet_gate_passed'] and not window['competing_workloads_before'] and not window['competing_workloads_after']
 patch=d/'source.patch.gz';raw=gzip.decompress(patch.read_bytes());m=json.loads((d/'source.patch.json').read_text())
 assert hashlib.sha256(raw).hexdigest()==m['original_sha256'] and hashlib.sha256(patch.read_bytes()).hexdigest()==m['gzip_sha256']
 expected={(v['fixture'],v['operation']):v for v in verifies if v['engine']=='node'}
 for v in verifies:
  e=expected[v['fixture'],v['operation']]
  assert all(v[k]==e[k] for k in (['checksum','retained'] if kind=='access' else ['verify_sha256','checksum','retained'])),(kind,v,e)
 groups=defaultdict(list)
 for r in timings:
  e=expected[r['fixture'],r['operation']]
  wanted=e['checksum']/(e['iterations']+e['warmup'])*(r['iterations']+r['warmup'])
  assert r['checksum']==wanted and r['retained']==0,(kind,r,e)
  groups[r['fixture'],r['operation'],r['engine']].append(r)
 for s in summary:
  rows=groups[s['fixture'],s['operation'],s['engine']]
  assert len(rows)==7
  if kind!='access':assert s['repetitions']==7
  cpu=[(r['user_us']+r['system_us'])/r['iterations'] for r in rows]
  rss=[r['peak_rss']/1048576 for r in rows]
  assert cpu==s['cpu_samples_us'] and rss==s['peak_rss_samples_mib']
  assert statistics.median(cpu)==s['cpu_us'] and statistics.median(rss)==s['peak_rss_mib']
 by_case=defaultdict(dict)
 for s in summary:by_case[s['fixture'],s['operation']][s['engine']]=s
 assert len(by_case)=={'access':12,'focus':15,'options':5}[kind]
 assert all(set(e)=={'baseline','prior','perry','node','bun'} for e in by_case.values())
 comparisons=[]
 for (fixture,operation),engines in by_case.items():
  a=engines['perry'];b=engines['baseline'];nodes=[engines[e] for e in ['node','bun']]
  comparisons.append(dict(fixture=fixture,operation=operation,engines=engines,delta_pct=(a['cpu_us']/b['cpu_us']-1)*100,
   prior_delta_pct=(a['cpu_us']/engines['prior']['cpu_us']-1)*100,
   separated_regression=min(a['cpu_samples_us'])>max(b['cpu_samples_us']),
   separated_improvement=max(a['cpu_samples_us'])<min(b['cpu_samples_us']),
   beats_both_peers=a['cpu_us']<min(x['cpu_us'] for x in nodes),
   all_samples_beat_both_peers=max(a['cpu_samples_us'])<min(min(x['cpu_samples_us']) for x in nodes)))
 outputs[kind]={'window':window,'timed_trials':len(timings),'cases':comparisons};total+=len(timings)
assert total==1120,total
(work/'analysis.json').write_text(json.dumps({'trials':total,'phases':outputs},indent=2)+'\n')
print('VERIFIED',total,'complete timed checksums and every sample vector/median; all three quiet windows and patches')
