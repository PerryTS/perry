from pathlib import Path
from statistics import median
import json,collections
L=Path('/root/lanes/perry-unpackcpu');groups=collections.defaultdict(list)
accepted=['micro-inst','micro-wall','micro-gc','upm-instructions','upm-wall','upm-gc','programs-instructions','programs-gc']
for name in accepted:
 p=L/'measure'/(name+'.jsonl')
 if not p.exists():continue
 for line in p.read_text().splitlines():
  r=json.loads(line);assert r['rc']==0 and r['parity'];groups[(r['kind'],r['name'],r['arm'])].append(r)
summary=[]
for key,rows in sorted(groups.items()):
 expected=7 if key[0]=='upm-wall' else 5
 assert len(rows)==expected and sorted(r['n'] for r in rows)==list(range(expected)),(key,len(rows))
 metrics={}
 for metric in ['instructions:u','cycles:u','wall','user','sys','rss_kb','fulls']:
  values=[r[metric] for r in rows if r.get(metric) is not None]
  if values:
   m=median(values);mad=median(abs(v-m) for v in values);metrics[metric]=dict(median=m,min=min(values),max=max(values),mad=mad,mad_pct=mad/m*100 if m else 0)
 summary.append(dict(kind=key[0],name=key[1],arm=key[2],n=len(rows),cpus=rows[0]['cpus'],metrics=metrics))
(L/'evidence/statistics.json').write_text(json.dumps(summary,indent=2))
for kind in ['micro-inst','upm-instructions','programs-instructions']:
 for name in sorted({r['name'] for r in summary if r['kind']==kind}):
  rs=[next(r for r in summary if r['kind']==kind and r['name']==name and r['arm']==arm) for arm in ['base','fix','node']]
  b,f,n=[r['metrics']['instructions:u']['median'] for r in rs]
  print(kind,name,f'{b/1e9:.6f}/{f/1e9:.6f}/{n/1e9:.6f}',f'{(f/b-1)*100:+.4f}%',flush=True)
print('accepted rows',sum(len(v) for v in groups.values()),flush=True)
