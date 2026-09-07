from pathlib import Path
import json,statistics,sys
root=Path(sys.argv[1]);out=root/'recheck'
def rows(name):return [json.loads(l) for l in (out/(name+'.jsonl')).read_text().splitlines()]
verify=rows('verify');timing=rows('timing');memory=rows('memory')
assert len(verify)==190 and all(r['correct'] for r in verify)
assert len(timing)==570 and len(memory)==324
assert all(r['exit_code']==0 and 'error' not in r for r in timing+memory)
def stats(xs):return {'median':statistics.median(xs),'range':[min(xs),max(xs)]}
def compare(a,b):
 return {'delta':b['median']-a['median'],'delta_percent':(b['median']/a['median']-1)*100 if a['median'] else None,'ranges_separated':a['range'][1]<b['range'][0] or b['range'][1]<a['range'][0]}
summaries=[]
for f,op in sorted({(r['fixture'],r['operation']) for r in timing}):
 selected=[r for r in timing if (r['fixture'],r['operation'])==(f,op)]
 assert len(selected)==15
 result={'fixture':f,'operation':op,'iterations':selected[0]['iterations']}
 for key in ['cpu_us','peak_rss_mib','rss_after_mib','instructions','cycles']:
  metric={}
  for engine in ['parent','checkpoint','candidate']:
   xs=[r['cpu_ms']*1000/r['iterations'] if key=='cpu_us' else r[key.removesuffix('_mib')]/1048576 if key.endswith('_mib') else r[key] for r in selected if r['engine']==engine]
   assert len(xs)==5
   metric[engine]=stats(xs)
  metric['vs_checkpoint']=compare(metric['checkpoint'],metric['candidate']);metric['vs_parent']=compare(metric['parent'],metric['candidate'])
  result[key]=metric
 summaries.append(result)
assert len(summaries)==38
(out/'summary.json').write_text(json.dumps(summaries,indent=2)+'\n')
lines=['# Five repeated comparisons: all 38 CPU rows','','CPU includes user and system time across the loop. Ranges are observed minima and maxima, not statistical confidence intervals. Parent is construction batch; checkpoint is bounded deferral.','','| Fixture | Operation | Parent µs | Checkpoint µs | Candidate µs | vs checkpoint | Separated | Peak RSS change MiB |','|---|---|---:|---:|---:|---:|---|---:|']
for r in summaries:
 c=r['cpu_us'];d=c['vs_checkpoint'];m=r['peak_rss_mib']['vs_checkpoint']
 lines.append(f"| {r['fixture']} | {r['operation']} | {c['parent']['median']:.3f} | {c['checkpoint']['median']:.3f} | {c['candidate']['median']:.3f} | {d['delta_percent']:+.2f}% | {d['ranges_separated']} | {m['delta']:+.3f} |")
(out/'table.md').write_text('\n'.join(lines)+'\n')
ms=[]
for f,op,n in sorted({(r['fixture'],r['operation'],r['iterations']) for r in memory}):
 a=[r for r in memory if (r['fixture'],r['operation'],r['iterations'])==(f,op,n)]
 assert len(a)==9
 record={'fixture':f,'operation':op,'retained':n}
 for key in ['peak_rss','rss_after']:
  metric={engine:stats([r[key]/1048576 for r in a if r['engine']==engine]) for engine in ['parent','checkpoint','candidate']}
  metric['vs_checkpoint']=compare(metric['checkpoint'],metric['candidate']);metric['vs_parent']=compare(metric['parent'],metric['candidate']);record[key+'_mib']=metric
 ms.append(record)
(out/'memory-summary.json').write_text(json.dumps(ms,indent=2)+'\n')
print('Verified 190 outputs, 570 CPU and 324 retained-memory trials')
for r in summaries:
 if r['operation']=='stringify' or r['fixture']=='wide_1m':
  print(r['fixture'],r['operation'],round(r['cpu_us']['vs_checkpoint']['delta_percent'],2),r['cpu_us']['vs_checkpoint']['ranges_separated'])
