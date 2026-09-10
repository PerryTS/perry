from pathlib import Path
from collections import defaultdict
import hashlib,json,statistics
w=Path(__file__).resolve().parent;bench=w.parents[1];d=bench/'results'/('quiet-'+w.name+'-modes');read=lambda n:json.loads((d/n).read_text());sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
window=read('window.json');assert window['quiet_gate_passed'] and window['finished_utc'];assert not window['competing_workloads_before'] and not window['competing_workloads_after'];assert 'results/'+d.name in window['command'];assert read('controller-exit.json')['exit_code']==0
host=read('host.json');assert host['diagnostic_only'] and host['source_commit']=='1a9c0de6cb790d2467b0ca22a660870025179b37';assert host['node_version']=='v26.5.1' and host['bun_version']=='1.3.14';assert host['worker_sha256']==sha(w/'main-worker')
assert host['commands']['main_auto'][-1]==host['commands']['main_direct'][-1]
assert host['commands']['main_auto'][:3]==['/usr/bin/env','-u','PERRY_JSON_TAPE']
assert host['commands']['main_direct'][:2]==['/usr/bin/env','PERRY_JSON_TAPE=0']
remote='/Users/perry/json-codex-yHdsko/benchmarks/json_performance/'
for path,digest in host['sources'].items():assert path.startswith(remote) and sha(bench/path.removeprefix(remote))==digest,path
staged=json.loads((w/'remote-stage-hashes.json').read_text());assert read('remote-stage-hashes.json')==staged
for path,digest in staged.items():assert sha(bench/path)==digest,path
assert len(staged)==16
for path,digest in read('fixture-hashes.json').items():assert digest==staged[path]
rows=lambda n:[json.loads(x) for x in (d/n).read_text().splitlines()]
verify,timing,summary=rows('verify.jsonl'),rows('timing.jsonl'),read('summary.json');assert len(verify)==50 and len(timing)==280 and len(summary)==40
expected={(r['fixture'],r['operation']):r for r in verify if r['engine']=='oracle'};assert len(expected)==10
for r in verify:
 e=expected[r['fixture'],r['operation']];assert all(r[k]==e[k] for k in ['checksum','retained','verify_sha256'])
groups=defaultdict(list)
for r in timing:
 e=expected[r['fixture'],r['operation']];assert r['checksum']==e['checksum']/15*(r['iterations']+r['warmup']) and r['retained']==0;groups[r['fixture'],r['operation'],r['engine']].append(r)
cases=defaultdict(dict)
for s in summary:
 chosen=groups[s['fixture'],s['operation'],s['engine']];assert [r['rep'] for r in chosen]==list(range(7));assert all(r['iterations']==s['iterations'] and r['warmup']==s['warmup'] for r in chosen)
 cpu=[(r['user_us']+r['system_us'])/r['iterations'] for r in chosen];rss=[r['peak_rss']/1048576 for r in chosen]
 assert cpu==s['cpu_samples_us'] and statistics.median(cpu)==s['cpu_us'];assert rss==s['peak_rss_samples_mib'] and statistics.median(rss)==s['peak_rss_mib'];cases[s['fixture'],s['operation']][s['engine']]=s
out=[]
for (fixture,op),engines in cases.items():
 assert set(engines)=={'main_auto','main_direct','node','bun'};a,b=engines['main_auto'],engines['main_direct']
 row={'fixture':fixture,'operation':op,'engines':engines,'direct_vs_auto_cpu_delta_pct':(b['cpu_us']/a['cpu_us']-1)*100,'direct_vs_auto_peak_rss_delta_mib':b['peak_rss_mib']-a['peak_rss_mib'],'direct_slower_pairs':sum(x>y for x,y in zip(b['cpu_samples_us'],a['cpu_samples_us'])),'direct_separated_slower':min(b['cpu_samples_us'])>max(a['cpu_samples_us']),'direct_separated_faster':max(b['cpu_samples_us'])<min(a['cpu_samples_us'])};out.append(row)
 print(fixture,op,'direct CPU',f"{row['direct_vs_auto_cpu_delta_pct']:+.2f}%",'RSS',f"{row['direct_vs_auto_peak_rss_delta_mib']:+.3f} MiB",flush=True)
(w/'modes-analysis.json').write_text(json.dumps({'window':window,'timed_trials':280,'verification_trials':50,'diagnostic_only':True,'note':'Same unchanged main executable in both modes. Route selection changes construction and memory lifetime together; this is not isolated phase cost or a production speedup.','cases':out},indent=2)+'\n')
print('VERIFIED all mode outputs, timings, sample vectors, hashes and quiet window.',flush=True)
