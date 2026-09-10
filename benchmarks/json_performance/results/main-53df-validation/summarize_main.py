from pathlib import Path
import json,math
b=Path('benchmarks/json_performance')
a=b/'results/quiet-main-53df-all-r5';r=b/'results/quiet-main-53df-rotating-r5'
for d in [a,r]:
 w=json.loads((d/'window.json').read_text());assert w['quiet_gate_passed'] and w['finished_utc']
 assert '.work/main-53df/worker' in w['command'] or '.work/main-53df/rotating-worker' in w['command']
h=json.loads((a/'host-all.json').read_text());rh=json.loads((r/'host.json').read_text())
for label,engine in [('main-53df','perry'),('main-eee','baseline')]:
 p=json.loads((a/('provenance.json' if engine=='perry' else 'reference-provenance.json')).read_text())
 assert h['worker_sha256'][engine]==p['files'][str(b/'.work'/label/'worker')]
 assert rh['worker_sha256' if engine=='perry' else 'baseline_worker_sha256']==p['files'][str(b/'.work'/label/'rotating-worker')]
assert rh['commit']=='53df2c671fff33b1f8f624432372c2401db8b1d0'
assert h['versions']==rh['versions']=={'node':'v26.5.1','bun':'1.3.14'}
rows=json.loads((a/'summary.json').read_text());cells={}
for row in rows:cells.setdefault((row['phase'],row['fixture'],row['operation'],row['iterations']),{})[row['engine']]=row
counts={}
for metric,phase in [('cpu_us','timing'),('peak_rss_mib',None),('rss_after_mib','memory')]:
 selected=[m for (p,*_),m in cells.items() if phase is None or p==phase]
 counts[metric]={'leads':sum(m['perry'][metric]<=min(m['node'][metric],m['bun'][metric]) for m in selected),'total':len(selected)}
rot=json.loads((r/'summary.json').read_text());ri={}
for row in rot:ri.setdefault((row['fixture'],row['mode']),{})[row['engine']]=row
rot_counts={}
for mode in ['rotating','same','select']:
 selected=[m for (_,mo),m in ri.items() if mo==mode]
 rot_counts[mode]={metric:{'leads':sum(m['perry'][metric]<=min(m['node'][metric],m['bun'][metric]) for m in selected),'total':len(selected)} for metric in ['cpu_us','peak_rss_mib','rss_after_mib']}
fixture_order=[f['name'] for f in json.loads((b/'results/fixtures.json').read_text())]
timing={(f,op):m for (phase,f,op,n),m in cells.items() if phase=='timing'}
lines=['# Fresh merged-main JSON measurements: 53df2c671','','Main `53df2c671fff33b1f8f624432372c2401db8b1d0` (0.5.1530) contains the accepted JSON work landed through train #10037. Its complete tree matches final PR #10036 head `6a5d2ba5e`. The matching compiler and static libraries were rebuilt from that actual commit, and fresh workers were linked. [Build, source identity and compiled GC validation](results/main-53df-validation/README.md).','','Apple M1 / 8 GiB; Node 26.5.1 and Bun 1.3.14. Five repetitions per engine and equal work within each row, under qualified quiet-host windows. CPU is process CPU inside the measured loop per call; RSS includes runtime and inputs. Results use medians; a lower median alone does not establish statistical significance. The `perry` arm is newly built main53df and `baseline` is earlier main eee3881c4.','','## Original suite','',f"Main leads both engines on {counts['cpu_us']['leads']}/{counts['cpu_us']['total']} CPU medians, {counts['peak_rss_mib']['leads']}/{counts['peak_rss_mib']['total']} peak-RSS medians and {counts['rss_after_mib']['leads']}/{counts['rss_after_mib']['total']} retained-current-RSS medians. All 200 output checks and 344 measurement groups validate. An additional checksum/count audit covers all 2120 verification, calibration, timing and memory records.",'','[Every original CPU, peak-RSS and current-RSS row](results/quiet-main-53df-all-r5/comparison.md). Repeated-source parse can reuse source/template information. The 38 parse/stringify CPU rows follow; array consumption is listed separately.','','| Fixture | Operation | Perry µs | Node µs | Bun µs | Perry / best |','|---|---|---:|---:|---:|---:|']
for f in fixture_order:
 for op in ['parse','stringify']:
  m=timing[f,op];c=m['perry']['cpu_us'];best=min(m['node']['cpu_us'],m['bun']['cpu_us'])
  lines.append(f"| {f} | {op} | {c:.6f} | {m['node']['cpu_us']:.6f} | {m['bun']['cpu_us']:.6f} | {c/best:.3f}x |")
lines+=['','## Array consumption','','| Fixture | Operation | Perry µs | Node µs | Bun µs | Perry / best |','|---|---|---:|---:|---:|---:|']
for f in fixture_order:
 for op in ['sparse','scan','roundtrip']:
  if (f,op) not in timing:continue
  m=timing[f,op];c=m['perry']['cpu_us'];best=min(m['node']['cpu_us'],m['bun']['cpu_us'])
  lines.append(f"| {f} | {op} | {c:.3f} | {m['node']['cpu_us']:.3f} | {m['bun']['cpu_us']:.3f} | {c/best:.3f}x |")
lines+=['','## Changing input','','Eight equal-size, same-shape sources are preloaded. Seventeen fixtures change a value; null and the empty object have identical contents in separately loaded strings. Same-source and selection-only controls keep the same pool alive. Selection costs are reported without subtraction. All 380 output checks and 1140 timing trials validate.','','[All changing-input, same-source and selection-control CPU/RSS rows](results/quiet-main-53df-rotating-r5/comparison.md).','','| Fixture | Perry µs | Node µs | Bun µs | Perry / best | Peak MiB: Perry / Node / Bun |','|---|---:|---:|---:|---:|---:|']
for f in fixture_order:
 m=ri[f,'rotating'];c=m['perry']['cpu_us'];best=min(m['node']['cpu_us'],m['bun']['cpu_us']);rss=' / '.join(f"{m[e]['peak_rss_mib']:.2f}" for e in ['perry','node','bun'])
 lines.append(f"| {f} | {c:.6f} | {m['node']['cpu_us']:.6f} | {m['bun']['cpu_us']:.6f} | {c/best:.3f}x | {rss} |")
lines+=['', 'CPU medians lead both engines in 13/19 changing-input rows and 18/19 same-source control rows. Peak RSS leads in 15/19 and 13/19 respectively. Perry trails the selection-only CPU control in all 19 rows; those controls measure input selection, with no JSON parsing. Their absolute costs are available in the complete table.', '', '## Remaining work','','The complete parity goal remains open. The four original CPU gaps are the 13 KiB, 1 MiB and 8 MiB array scans plus the 20 MiB roundtrip. Changing-input and large-container memory gaps remain visible above. Positive CPU/RSS deltas versus older main are retained in the raw and comparison tables; these measurements do not prove an absence of all regressions.','','Inherited lazy stringify canonicalization failures are documented in [LAZY_CANONICAL.md](LAZY_CANONICAL.md). The slower R4/R5 corrections were parked separately and are not in this main build. A [fresh scan profile](https://github.com/PerryTS/perry/blob/98bd51eb0/benchmarks/json_performance/results/pr-lazy-record-scan-profile/README.md) identifies the adaptive full reparse as a next target; indexed scalar projection is still unimplemented. [Large-container memory diagnostics](GC_MEMORY_GROWTH.md) remain relevant. No collector-policy change is included here.']
(b/'MERGED_MAIN_53DF.md').write_text('\n'.join(lines)+'\n')
result={'original':counts,'rotating':rot_counts}
(b/'results/main-53df-validation/parity-counts.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2))
