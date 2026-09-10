from pathlib import Path
import json
w=Path(__file__).resolve().parent;bench=w.parents[1]
a=json.loads((w/'analysis.json').read_text());b=json.loads((w/'build-provenance.json').read_text())
lines=['# Outlined JSON stringify entry (R7)','', 'DECISION_PENDING','',f"Source: `{b['source_commit']}` on `codex/json-stringify-entry-r7`, based on R5 and excluding R6. Baseline is frozen main `53df2c671fff33b1f8f624432372c2401db8b1d0` (0.5.1530), not a fresh measurement of the latest main. Five engines run interleaved on the M1 Mac mini (8 GiB), seven fresh-process trials per row, Node 26.5.1 and Bun 1.3.14.",'','CPU is microseconds per operation. Access rows count one loop iteration after parsing; focus and options rows count the complete operation. Options input is eagerly parsed before timing. Peak RSS is MiB for the whole process, including input, runtime, output, and allocator storage; it is not retained heap. Negative deltas are faster. Every sample, including outliers, is retained.','']
for phase,title in [('access','Access after parsing'),('focus','Parse, stringify, and consumption'),('options','Stringify fallback arguments')]:
 p=a['phases'][phase];window=p['window']
 lines += ['## '+title,'',f"Window: {window['started_utc']} to {window['finished_utc']}; one-minute load {window['load_before'][0]:.3f} to {window['load_after'][0]:.3f}. Quiet gate passed, no detected competing workloads at either boundary.",'','| Workload | Main | R5 | R7 | Node | Bun | R7 vs main | R7 vs R5 |','|---|---:|---:|---:|---:|---:|---:|---:|']
 for c in p['cases']:
  e=c['engines'];values=[f"{e[n]['cpu_us']:.6f}" for n in ['baseline','prior','perry','node','bun']]
  lines.append('| '+c['fixture']+' / '+c['operation']+' | '+' | '.join(values)+f" | {c['delta_pct']:+.2f}% | {c['prior_delta_pct']:+.2f}% |")
 lines += ['','Peak RSS:','','| Workload | Main | R5 | R7 | Node | Bun |','|---|---:|---:|---:|---:|---:|']
 for c in p['cases']:
  e=c['engines'];values=[f"{e[n]['peak_rss_mib']:.2f}" for n in ['baseline','prior','perry','node','bun']]
  lines.append('| '+c['fixture']+' / '+c['operation']+' | '+' | '.join(values)+' |')
 slug='quiet-'+w.name+'-'+phase
 lines += ['',f"[Raw samples](results/{slug}/timing.jsonl), [complete-output verification](results/{slug}/verify.jsonl), [CPU/RSS vectors and medians](results/{slug}/summary.json), [qualified window](results/{slug}/window.json).",'']
lines += ['## Implementation and evidence','','IMPLEMENTATION_PENDING','','## Validation and limits','','VALIDATION_PENDING','',f"All {a['trials']} timed checksums, CPU/RSS sample vectors, medians, source-patch hashes, and quiet windows were independently checked by [analyze.py](results/stringify-entry-r7-validation/analyze.py).",'','The full original 38-row matrix, rotating-source suite, short-call controls, and retained-memory suite were not rerun for this experiment. Earlier results remain in [merged-main measurements](MERGED_MAIN_53DF.md) and [R5](INVARIANT_FIELD_LOOP.md). The no-regressions objective remains open.','']
(w/'report-draft.md').write_text('\n'.join(lines))
print('Wrote',w/'report-draft.md')
