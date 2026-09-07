from pathlib import Path
import json,statistics,csv
root=Path(__file__).resolve().parent/'results'
read=lambda p:[json.loads(l) for l in (root/p).read_text().splitlines()]
verify=read('recheck/verify.jsonl');timing=read('recheck/timing.jsonl');memory=read('recheck/memory.jsonl');life=read('lifetimes/raw.jsonl')
assert len(verify)==152 and all(r['correct'] for r in verify)
assert len(timing)==380 and len(memory)==216 and len(life)==108
assert all(r['exit_code']==0 and 'error' not in r and r['tape'] is None for r in timing+memory)
assert len({r['executable_path'] for r in timing+memory+life})==1
stats=lambda xs:dict(median=statistics.median(xs),range=[min(xs),max(xs)])
engines=['checkpoint','candidate'];summary=[]
for f,op in sorted({(r['fixture'],r['operation']) for r in timing}):
 a=[r for r in timing if (r['fixture'],r['operation'])==(f,op)]
 assert len(a)==10
 row=dict(fixture=f,operation=op)
 for k in ['cpu_us','peak_rss_mib','instructions']:
  values={e:stats([r['cpu_ms']*1000/r['iterations'] if k=='cpu_us' else r['peak_rss']/1048576 if k=='peak_rss_mib' else r[k] for r in a if r['engine']==e]) for e in engines}
  ref,c=values['checkpoint'],values['candidate'];values['delta']=c['median']-ref['median'];values['delta_percent']=100*(c['median']/ref['median']-1);values['ranges_separated']=ref['range'][1]<c['range'][0] or c['range'][1]<ref['range'][0];row[k]=values
 summary.append(row)
(root/'recheck/summary.json').write_text(json.dumps(summary,indent=2)+'\n')
lines=['# All 38 rows with an identical executable path','','Five fresh-process trials per arm and row. CPU includes user and system time per call; RSS is process peak. Observed ranges are not confidence intervals. This comparison corrects the invocation-path difference between the Perry arms. Node/Bun are output oracles in this window; their timing figures are in the separate four-engine run.','','| Fixture | Operation | Reference µs | Candidate µs | CPU change | Ranges separated | Peak RSS change MiB |','|---|---|---:|---:|---:|---|---:|']
for r in summary:
 c=r['cpu_us'];lines.append(f"| {r['fixture']} | {r['operation']} | {c['checkpoint']['median']:.3f} | {c['candidate']['median']:.3f} | {c['delta_percent']:+.3f}% | {c['ranges_separated']} | {r['peak_rss_mib']['delta']:+.4f} |")
(root/'recheck/all-38.md').write_text('\n'.join(lines)+'\n')
ms=[]
for f,op,n in sorted({(r['fixture'],r['operation'],r['iterations']) for r in memory}):
 a=[r for r in memory if (r['fixture'],r['operation'],r['iterations'])==(f,op,n)];assert len(a)==6
 v={e:{k:stats([r[k]/1048576 for r in a if r['engine']==e]) for k in ['peak_rss','rss_after']} for e in engines};ms.append(dict(fixture=f,operation=op,iterations=n,values=v))
(root/'recheck/memory-summary.json').write_text(json.dumps(ms,indent=2)+'\n')
ls=[]
for f,op,mode,n in sorted({(r['fixture'],r['operation'],r['mode'],r['iterations']) for r in life}):
 a=[r for r in life if (r['fixture'],r['operation'],r['mode'],r['iterations'])==(f,op,mode,n)];assert len(a)==6
 v={e:{k:stats([r[k] for r in a if r['engine']==e]) for k in ['totalCpuUs','peak_rss','rssDrained']} for e in engines}
 ls.append(dict(fixture=f,operation=op,mode=mode,iterations=n,values=v,cpu_delta_percent=100*(v['candidate']['totalCpuUs']['median']/v['checkpoint']['totalCpuUs']['median']-1),peak_rss_delta_mib=(v['candidate']['peak_rss']['median']-v['checkpoint']['peak_rss']['median'])/1048576))
(root/'lifetimes/summary.json').write_text(json.dumps(ls,indent=2)+'\n')
print('Verified 152 outputs, 380 timings, 216 memory cases, 108 lifetimes and common invocation path')
print('\n'.join(lines))
for r in ls:print('LIFETIME',r['fixture'],r['operation'],r['mode'],r['iterations'],round(r['cpu_delta_percent'],3),r['peak_rss_delta_mib'])
