from pathlib import Path
import json,statistics,sys
out=Path(sys.argv[1]);summary=[]
for f in sorted(out.glob('*.jsonl')):
 rows=[json.loads(s) for s in f.read_text().splitlines()]
 assert len(rows)==14 and all('error' not in r and r['exit_code']==0 for r in rows),f
 cpus={e:[r['cpu_ms']*1000/r['iterations'] for r in rows if r['engine']==e] for e in ['baseline','perry']}
 assert all(len(v)==7 for v in cpus.values()),f
 b,c=statistics.median(cpus['baseline']),statistics.median(cpus['perry'])
 br,cr=[min(cpus['baseline']),max(cpus['baseline'])],[min(cpus['perry']),max(cpus['perry'])]
 row={'fixture':rows[0]['fixture'],'operation':rows[0]['operation'],'reference':rows[0]['reference'],'iterations':rows[0]['iterations'],'baseline_cpu_us':b,'candidate_cpu_us':c,'delta_percent':100*(c/b-1),'baseline_range_us':br,'candidate_range_us':cr,'ranges_separated':br[1]<cr[0] or cr[1]<br[0]}
 for metric in ['peak_rss','rss_after']:
  values={e:[r[metric]/1048576 for r in rows if r['engine']==e] for e in ['baseline','perry']}
  bm,cm=statistics.median(values['baseline']),statistics.median(values['perry'])
  brm,crm=[min(values['baseline']),max(values['baseline'])],[min(values['perry']),max(values['perry'])]
  row[metric+'_mib']={'baseline':bm,'candidate':cm,'delta':cm-bm,'delta_percent':100*(cm/bm-1),'baseline_range':brm,'candidate_range':crm,'ranges_separated':brm[1]<crm[0] or crm[1]<brm[0]}
 summary.append(row)
 print(row['reference'],row['fixture'],row['operation'],round(row['delta_percent'],2),row['ranges_separated'])
assert len(summary)==56,len(summary)
(out/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
