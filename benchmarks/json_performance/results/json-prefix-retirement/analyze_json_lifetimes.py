from pathlib import Path
import json,statistics as st,sys
p=Path(sys.argv[1]);rows=[json.loads(x) for x in (p/'raw.jsonl').read_text().splitlines()]
assert len(rows)==108 and all(not r['trace'] for r in rows)
assert all(r['output_verified'] for r in rows if r['mode'] in ['retain','latest'])
result=[]
for f,op,mode,n in sorted(set((r['fixture'],r['operation'],r['mode'],r['iterations']) for r in rows)):
 group=[r for r in rows if (r['fixture'],r['operation'],r['mode'],r['iterations'])==(f,op,mode,n)];record=dict(fixture=f,operation=op,mode=mode,iterations=n)
 for key in ['loopCpuUs','drainCpuUs','totalCpuUs','peak_rss','rssDrained']:
  arms={}
  for e in ['parent','checkpoint','candidate']:
   values=[r[key] for r in group if r['engine']==e];assert len(values)==3;arms[e]={'median':st.median(values),'range':[min(values),max(values)]}
  arms['vs_checkpoint_percent']=(arms['candidate']['median']/arms['checkpoint']['median']-1)*100
  arms['vs_parent_percent']=(arms['candidate']['median']/arms['parent']['median']-1)*100
  record[key]=arms
 result.append(record)
(p/'summary.json').write_text(json.dumps(result,indent=2)+'\n')
lines=['# Lifetimes including final collection','','Three randomized triples per case. Negative changes mean less total CPU. Ranges are observed minima/maxima, not confidence intervals.','','| Fixture | Operation | Lifetime | Calls | Parent ms | Checkpoint ms | Candidate ms | vs checkpoint | vs parent |','|---|---|---|---:|---:|---:|---:|---:|---:|']
for r in result:
 t=r['totalCpuUs'];lines.append(f"| {r['fixture']} | {r['operation']} | {r['mode']} | {r['iterations']} | {t['parent']['median']/1000:.3f} | {t['checkpoint']['median']/1000:.3f} | {t['candidate']['median']/1000:.3f} | {t['vs_checkpoint_percent']:+.2f}% | {t['vs_parent_percent']:+.2f}% |")
(p/'table.md').write_text('\n'.join(lines)+'\n');print('Verified',len(rows),'trials and',len(result),'cases')
