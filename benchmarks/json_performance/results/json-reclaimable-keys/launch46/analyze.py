from pathlib import Path
import json,statistics
root=Path(__file__).resolve().parent/'results'
read=lambda p:[json.loads(l) for l in (root/p).read_text().splitlines()]
verify=read('recheck/verify.jsonl');timing=read('recheck/timing.jsonl');memory=read('recheck/memory.jsonl');life=read('lifetimes/raw.jsonl')
assert len(verify)==266 and all(r['correct'] for r in verify)
assert len(timing)==1330 and len(memory)==600 and len(life)==150
assert all(r['exit_code']==0 and 'error' not in r and r['tape'] is None for r in timing+memory)
assert len({r['argv0'] for r in timing+memory+life if r['engine'] not in ['node','bun']})==1
assert all(not r['trace'] for r in life)
assert all(r['output_verified'] for r in life if r['mode'] in ['latest','retain'])
assert json.loads((root/'window-custom.json').read_text())['quiet_gate_passed']
assert all(r['monitor_exit']==0 and not r['external'] and not r['xprotect_busy'] for r in json.loads((root/'trace-observations.json').read_text()))
stats=lambda xs:dict(median=statistics.median(xs),range=[min(xs),max(xs)])
selected='candidate'
engines=['fused','previous','copy','branch','candidate','node','bun'];summary=[]
for fixture,op in sorted({(r['fixture'],r['operation']) for r in timing}):
 a=[r for r in timing if (r['fixture'],r['operation'])==(fixture,op)];assert len(a)==35
 row=dict(fixture=fixture,operation=op)
 for metric in ['cpu_us','peak_rss_mib','instructions']:
  v={e:stats([r['cpu_ms']*1000/r['iterations'] if metric=='cpu_us' else r['peak_rss']/1048576 if metric=='peak_rss_mib' else r[metric] for r in a if r['engine']==e]) for e in engines}
  ref,c=v['previous'],v[selected];v['delta']=c['median']-ref['median'];v['delta_percent']=100*(c['median']/ref['median']-1);v['ranges_separated']=ref['range'][1]<c['range'][0] or c['range'][1]<ref['range'][0];row[metric]=v
 summary.append(row)
(root/'recheck/summary.json').write_text(json.dumps(summary,indent=2)+'\n')
lines=['# All 38 parse/stringify rows','','Five fresh-process trials per engine and row, identical work counts. CPU includes user and system time per call; RSS is process peak. All Perry builds use the same argv[0] via an immutable-file execv launcher. The previous candidate is selected depth22; Fused35 is 18d8e529f; Copy41 is 2bb79a173. Observed ranges and instructions are in summary.json. Median standings do not establish no regression.','','| Fixture | Operation | Fused35 µs | Previous µs | Copy41 µs | Branch44 µs | Keys45 µs | Node µs | Bun µs | Change vs previous | Selected RSS MiB | Node RSS MiB | Bun RSS MiB |','|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|']
for r in summary:
 c=r['cpu_us'];m=r['peak_rss_mib'];cpus=' | '.join(f"{c[e]['median']:.3f}" for e in engines);rss=' | '.join(f"{m[e]['median']:.3f}" for e in [selected,'node','bun'])
 lines.append(f"| {r['fixture']} | {r['operation']} | {cpus} | {c['delta_percent']:+.2f}% | {rss} |")
(root/'recheck/all-38.md').write_text('\n'.join(lines)+'\n')
ms=[]
for fixture,op,n in sorted({(r['fixture'],r['operation'],r['iterations']) for r in memory}):
 a=[r for r in memory if (r['fixture'],r['operation'],r['iterations'])==(fixture,op,n)];assert len(a)==15
 v={e:{k:stats([r[k]/1048576 for r in a if r['engine']==e]) for k in ['peak_rss','rss_after']} for e in engines[:5]}
 ms.append(dict(fixture=fixture,operation=op,iterations=n,values=v,peak_delta_mib=v[selected]['peak_rss']['median']-v['previous']['peak_rss']['median'],rss_after_delta_mib=v[selected]['rss_after']['median']-v['previous']['rss_after']['median']))
(root/'recheck/memory-summary.json').write_text(json.dumps(ms,indent=2)+'\n')
ls=[]
for fixture,op,mode,n in sorted({(r['fixture'],r['operation'],r['mode'],r['iterations']) for r in life}):
 a=[r for r in life if (r['fixture'],r['operation'],r['mode'],r['iterations'])==(fixture,op,mode,n)];assert len(a)==15
 v={e:{k:stats([r[k] for r in a if r['engine']==e]) for k in ['totalCpuUs','peak_rss','rssDrained']} for e in engines[:5]}
 ls.append(dict(fixture=fixture,operation=op,mode=mode,iterations=n,values=v,cpu_delta_percent=100*(v[selected]['totalCpuUs']['median']/v['previous']['totalCpuUs']['median']-1),peak_delta_mib=(v[selected]['peak_rss']['median']-v['previous']['peak_rss']['median'])/1048576))
(root/'lifetimes/summary.json').write_text(json.dumps(ls,indent=2)+'\n')
print('Verified 266 outputs, 1330 timings, 600 memory trials, 150 lifetimes and the common argv[0].')
for r in summary:print(r['fixture'],r['operation'],round(r['cpu_us']['delta_percent'],3),r['cpu_us']['ranges_separated'],round(r['peak_rss_mib']['delta'],5))
for r in ls:print('LIFETIME',r['fixture'],r['operation'],r['mode'],r['iterations'],round(r['cpu_delta_percent'],3),r['peak_delta_mib'])
