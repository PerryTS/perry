from pathlib import Path
import json,statistics as st,collections,hashlib
L=Path('/root/lanes/perry-coldextract');O=L/'summary';O.mkdir(exist_ok=True)
rows=[]
for p in (L/'measure').glob('*.jsonl'):
 for line in p.read_text().splitlines():
  row=json.loads(line);row['series']=p.stem;rows.append(row)
keys=('instructions:u','cycles:u','wall','user','sys','rss_kb','fulls')
stats={}
for series in sorted(set(r['series'] for r in rows)):
 stats[series]={}
 for name in dict.fromkeys(r['name'] for r in rows if r['series']==series):
  stats[series][name]={}
  for arm in ['base','fix','node']:
   group=[r for r in rows if (r['series'],r['name'],r['arm'])==(series,name,arm)]
   if not group:continue
   assert all(r['rc']==0 and r['parity'] for r in group),(series,name,arm)
   d={}
   for key in keys:
    vals=[r[key] for r in group if r.get(key) is not None]
    if vals:
     med=st.median(vals);d[key]=dict(n=len(vals),median=med,min=min(vals),max=max(vals),mad=st.median(abs(v-med) for v in vals))
   stats[series][name][arm]=d
(O/'statistics.json').write_text(json.dumps(stats,indent=2))
for kind in ['micro','programs','upm']:
 lines=['| Task | Instructions G B / F / N | Δ F/B | Cycles G B / F / N | Wall s B / F / N | User s B / F / N | RSS MiB B / F / N | Full GC B / F / N |','|---|---|---:|---|---|---|---|---|']
 names=list(stats.get(kind+'-inst',{}))
 def get(name,arm,key,series):return stats.get(series,{}).get(name,{}).get(arm,{}).get(key,{}).get('median')
 for name in names:
  cells=[]
  gc_series='locked-'+kind+'-gc' if kind!='programs' else kind+'-gc'
  rss_series=kind+'-cycles' if kind!='programs' else kind+'-inst'
  for key,scale,series,digits in [('instructions:u',1e9,kind+'-inst',6),('cycles:u',1e9,kind+'-cycles',4),('wall',1,kind+'-cycles' if kind!='programs' else kind+'-inst',4),('user',1,kind+'-cycles' if kind!='programs' else kind+'-inst',4),('rss_kb',1024,rss_series,1),('fulls',1,gc_series,0)]:
   vals=[get(name,a,key,series) for a in ['base','fix','node']];cells.append(' / '.join('—' if v is None else f'{v/scale:.{digits}f}' for v in vals))
  b=get(name,'base','instructions:u',kind+'-inst');f=get(name,'fix','instructions:u',kind+'-inst');delta='—' if b is None or f is None else f'{100*(f/b-1):+.3f}%'
  lines.append('| '+name+' | '+cells[0]+' | '+delta+' | '+' | '.join(cells[1:])+' |')
 (O/(kind+'.md')).write_text('\n'.join(lines)+'\n')
 print('\n'.join(lines))
# Every noise floor is the larger within-arm relative MAD, alongside full range.
noise=[]
for series in ['micro-inst','programs-inst','upm-inst','micro-cycles','upm-cycles']:
 for name,arms in stats.get(series,{}).items():
  for key in ['instructions:u','cycles:u','wall','rss_kb']:
   ds=[a[key] for arm,a in arms.items() if arm in ['base','fix'] and key in a]
   if not ds:continue
   noise.append(dict(series=series,name=name,metric=key,mad_max=max(d['mad'] for d in ds),relative_mad_percent_max=max(100*d['mad']/d['median'] if d['median'] else 0 for d in ds),ranges={a:[v[key]['min'],v[key]['max']] for a,v in arms.items() if key in v}))
(O/'noise.json').write_text(json.dumps(noise,indent=2))
