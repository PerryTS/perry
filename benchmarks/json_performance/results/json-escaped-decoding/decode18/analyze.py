from pathlib import Path
import json,statistics
root=Path(__file__).resolve().parent/'results'
read=lambda p:[json.loads(l) for l in (root/p).read_text().splitlines()]
verify=read('focused/verify.jsonl');timing=read('focused/timing.jsonl');life=read('lifetimes/raw.jsonl')
assert len(verify)==54 and all(r['correct'] for r in verify)
assert len(timing)==378 and len(life)==147
assert all(r['exit_code']==0 and 'error' not in r and r['tape'] is None for r in timing)
assert len({r['executable_path'] for r in timing+life if r['engine'] not in ['node','bun']})==1
assert all(not r['trace'] for r in life)
assert all(r['output_verified'] for r in life if r['mode'] in ['latest','retain'])
assert json.loads((root/'window-custom.json').read_text())['quiet_gate_passed']
assert all(r['monitor_exit']==0 and not r['external'] for r in json.loads((root/'trace-observations.json').read_text()))
stats=lambda xs:dict(median=statistics.median(xs),range=[min(xs),max(xs)])
engines=['checkpoint','counter','inline','candidate','node','bun'];summary=[]
for fixture,op in sorted({(r['fixture'],r['operation']) for r in timing}):
 a=[r for r in timing if (r['fixture'],r['operation'])==(fixture,op)];assert len(a)==42
 row=dict(fixture=fixture,operation=op)
 for metric in ['cpu_us','peak_rss_mib','instructions']:
  v={e:stats([r['cpu_ms']*1000/r['iterations'] if metric=='cpu_us' else r['peak_rss']/1048576 if metric=='peak_rss_mib' else r[metric] for r in a if r['engine']==e]) for e in engines}
  ref,c=v['inline'],v['candidate'];v['delta_vs_inline']=c['median']-ref['median'];v['delta_percent_vs_inline']=100*(c['median']/ref['median']-1);v['delta_percent_vs_counter']=100*(c['median']/v['counter']['median']-1);v['ranges_separated']=ref['range'][1]<c['range'][0] or c['range'][1]<ref['range'][0];row[metric]=v
 summary.append(row)
(root/'focused/summary.json').write_text(json.dumps(summary,indent=2)+'\n')
lines=['# Scanner inlining control','','Seven fresh processes per engine and row. Inline is decode17; outlined is decode18. Their only source difference is the inlining attribute on parse_string_bytes. CPU is user + system time per call; RSS is process peak. All Perry arms use the same invocation path. This is a focused comparison, not an all-row acceptance.','','| Fixture | Operation | Previous µs | Inline µs | Outlined µs | Node µs | Bun µs | Outlined vs inline | Outlined RSS MiB |','|---|---|---:|---:|---:|---:|---:|---:|---:|']
for r in summary:
 c=r['cpu_us'];m=r['peak_rss_mib'];cpus=' | '.join(f"{c[e]['median']:.3f}" for e in engines[1:])
 lines.append(f"| {r['fixture']} | {r['operation']} | {cpus} | {c['delta_percent_vs_inline']:+.2f}% | {m['candidate']['median']:.3f} |")
(root/'focused/summary.md').write_text('\n'.join(lines)+'\n')
ls=[]
for fixture,op,mode,n in sorted({(r['fixture'],r['operation'],r['mode'],r['iterations']) for r in life}):
 a=[r for r in life if (r['fixture'],r['operation'],r['mode'],r['iterations'])==(fixture,op,mode,n)];assert len(a)==21
 v={e:{k:stats([r[k] for r in a if r['engine']==e]) for k in ['totalCpuUs','peak_rss','rssDrained']} for e in ['counter','inline','candidate']}
 ls.append(dict(fixture=fixture,operation=op,mode=mode,iterations=n,values=v,cpu_delta_percent_vs_inline=100*(v['candidate']['totalCpuUs']['median']/v['inline']['totalCpuUs']['median']-1),cpu_delta_percent_vs_counter=100*(v['candidate']['totalCpuUs']['median']/v['counter']['totalCpuUs']['median']-1),peak_delta_mib_vs_inline=(v['candidate']['peak_rss']['median']-v['inline']['peak_rss']['median'])/1048576))
(root/'lifetimes/summary.json').write_text(json.dumps(ls,indent=2)+'\n')
print('Verified 54 outputs, 378 timings, 147 lifetimes and the common invocation path.')
for r in summary:print(r['fixture'],r['operation'],round(r['cpu_us']['delta_percent_vs_inline'],3),round(r['cpu_us']['delta_percent_vs_counter'],3),r['cpu_us']['ranges_separated'],round(r['peak_rss_mib']['delta_vs_inline'],5))
for r in ls:print('LIFETIME',r['fixture'],r['operation'],r['mode'],r['iterations'],round(r['cpu_delta_percent_vs_inline'],3),round(r['cpu_delta_percent_vs_counter'],3),r['peak_delta_mib_vs_inline'])
