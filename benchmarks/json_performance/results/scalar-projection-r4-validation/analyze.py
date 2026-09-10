from pathlib import Path
import collections, hashlib, json, statistics, sys
work=Path(__file__).resolve().parent
bench=work.parents[1]
all_rows=[]
for kind in (sys.argv[1:] or ['access','focus']):
 dest=bench/'results'/('quiet-'+work.name+'-'+kind)
 window=json.loads((dest/'window.json').read_text());assert window['quiet_gate_passed']
 timing=[json.loads(l) for l in (dest/'timing.jsonl').read_text().splitlines()]
 verify=[json.loads(l) for l in (dest/'verify.jsonl').read_text().splitlines()]
 summary=json.loads((dest/'summary.json').read_text())
 expected={(r['fixture'],r['operation']):r for r in verify if r['engine']=='node'}
 if kind=='focus':
  for r in verify:
   ref=expected[r['fixture'],r['operation']]
   for k in ['checksum','verify_sha256','retained']: assert r[k]==ref[k],(kind,r,k)
 groups=collections.defaultdict(list)
 for r in timing:
  ref=expected[r['fixture'],r['operation']]
  check=ref['checksum'] if kind=='access' else ref['checksum']/(ref['iterations']+ref['warmup'])*(r['iterations']+r['warmup'])
  assert r['checksum']==check and r['retained']==0, r
  groups[r['fixture'],r['operation'],r['engine']].append(r)
 for s in summary:
  group=groups[s['fixture'],s['operation'],s['engine']]
  assert len(group)==7 and sorted(r['rep'] for r in group)==list(range(7))
  cpu=[(r['user_us']+r['system_us'])/r['iterations'] for r in group]
  peaks=[r['peak_rss']/1048576 for r in group]
  assert cpu==s['cpu_samples_us'] and peaks==s['peak_rss_samples_mib']
  assert statistics.median(cpu)==s['cpu_us'] and statistics.median(peaks)==s['peak_rss_mib']
 rows=[]
 for fixture,operation in dict.fromkeys((s['fixture'],s['operation']) for s in summary):
  arms={s['engine']:s for s in summary if s['fixture']==fixture and s['operation']==operation}
  a,b=arms['perry'],arms['baseline']
  separated=max(a['cpu_samples_us'])<min(b['cpu_samples_us']) or min(a['cpu_samples_us'])>max(b['cpu_samples_us'])
  cpu={e:s['cpu_us'] for e,s in arms.items()};rss={e:s['peak_rss_mib'] for e,s in arms.items()}
  row={'kind':kind,'fixture':fixture,'operation':operation,'cpu_us':cpu,'peak_rss_mib':rss,'cpu_delta_percent':100*(cpu['perry']/cpu['baseline']-1),'cpu_separated_vs_main':separated,'rss_delta_mib':rss['perry']-rss['baseline'],'ratio_vs_fastest_peer':cpu['perry']/min(cpu['node'],cpu['bun'])}
  rows.append(row)
  print(f"{kind:6s} {fixture:20s} {operation:10s} "+'/'.join(f'{cpu[e]:.6f}' for e in ['baseline','perry','node','bun'])+f" {row['cpu_delta_percent']:+.2f}% {'SEPARATED' if separated else 'overlap'} rss "+'/'.join(f'{rss[e]:.2f}' for e in ['baseline','perry','node','bun']))
 evidence={'quiet_window':window,'verified_timing_rows':len(timing),'verified_oracles':len(verify),'rows':rows,'input_hashes':{n:hashlib.sha256((dest/n).read_bytes()).hexdigest() for n in ['timing.jsonl','verify.jsonl','summary.json','window.json']}}
 (work/(kind+'-analysis.json')).write_text(json.dumps(evidence,indent=2)+'\n');all_rows+=rows
print('Separated CPU regressions:',[(r['kind'],r['fixture'],r['operation'],r['cpu_delta_percent']) for r in all_rows if r['cpu_delta_percent']>0 and r['cpu_separated_vs_main']])
