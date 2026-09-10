from pathlib import Path
import json,statistics,math,shutil
root=Path('benchmarks/json_performance'); d=root/'results/quiet-tape-depth-r2-main-focus-r9-r27'
w=json.loads((d/'window.json').read_text()); assert w['quiet_gate_passed'] and w['finished_utc']
s=json.loads((d/'summary.json').read_text()); host=json.loads((d/'host.json').read_text())
verify=[json.loads(x) for x in (d/'verify.jsonl').read_text().splitlines()]
assert len(verify)==4*len(host['cases']) and all(r.get('correct',r['engine']=='node') for r in verify)
trials=[json.loads(x) for x in (d/'timing.jsonl').read_text().splitlines()]
assert len(trials)==3*sum(c[4] for c in host['cases']) and all(not r.get('error') and all(math.isfinite(r[k]) for k in ['wall_ms','user_us','system_us','rss_after','peak_rss']) for r in trials)
index={(r['fixture'],r['operation'],r['iterations'],r['engine']):r for r in s}
records=[]
for f,op,count,warm,reps in host['cases']:
 c=index[f,op,count,'perry']; main=index[f,op,count,'baseline']; prior=index[f,op,count,'prior']
 assert all(r['repetitions']==reps for r in [c,main,prior])
 assert all(math.isfinite(n) and n>0 for r in [c,main,prior] for n in r['cpu_samples_us'])
 c_samples=c['cpu_samples_us']; b_samples=main['cpu_samples_us']
 pairs=[100*(a/b-1) for a,b in zip(c_samples,b_samples)]
 row={'fixture':f,'operation':op,'repetitions':reps,'candidate_us':c['cpu_us'],'main_us':main['cpu_us'],'prior_us':prior['cpu_us'],'candidate_vs_main_pct':100*(c['cpu_us']/main['cpu_us']-1),'candidate_vs_prior_pct':100*(c['cpu_us']/prior['cpu_us']-1),'paired_slower_count':sum(x>0 for x in pairs),'paired_median_pct':statistics.median(pairs),'separated_slower':min(c_samples)>max(b_samples),'separated_faster':max(c_samples)<min(b_samples),'median_peak_rss_delta_kib':1024*(c['peak_rss_mib']-main['peak_rss_mib'])}
 records.append(row)
 print(f,op,round(row['candidate_vs_main_pct'],3),round(row['candidate_vs_prior_pct'],3),'paired slower',row['paired_slower_count'],'RSS KiB',row['median_peak_rss_delta_kib'])
(d/'reference-screen.json').write_text(json.dumps(records,indent=2)+'\n')
prior=json.loads((root/'.work/tape-depth-r1/provenance.json').read_text())
assert prior['files']['benchmarks/json_performance/.work/tape-depth-r1/worker']==host['workers']['prior']
shutil.copy2(root/'.work/tape-depth-r1/provenance.json',d/'prior-provenance.json')
lines=['# Tape-depth R2 focused replay','','Nine repetitions per case, with 27 for Unicode stringify, and three randomized Perry arms: tape-depth R2, freshly built merged main eee3881c4, and tape-depth R1. Node is the output oracle. All 48 output checks and 378 timing trials passed, under the archived quiet-host window. CPU is process CPU per call, with all samples retained. Differences are descriptive; overlapping ranges do not prove equality.','','| Fixture | Operation | Candidate us | Main us | R1 us | vs main | vs R1 | Slower pairs | Peak delta KiB |','|---|---|---:|---:|---:|---:|---:|---:|---:|']
for r in records:lines.append(f"| {r['fixture']} | {r['operation']} | {r['candidate_us']:.6f} | {r['main_us']:.6f} | {r['prior_us']:.6f} | {r['candidate_vs_main_pct']:+.3f}% | {r['candidate_vs_prior_pct']:+.3f}% | {r['paired_slower_count']}/{r['repetitions']} | {r['median_peak_rss_delta_kib']:+.0f} |")
(d/'README.md').write_text('\n'.join(lines)+'\n')
