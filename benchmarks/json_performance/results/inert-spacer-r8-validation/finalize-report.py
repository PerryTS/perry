from pathlib import Path
import json, subprocess
w=Path(__file__).resolve().parent;bench=w.parents[1]
subprocess.run(['python3',str(w/'write-report.py')],check=True)
s=(w/'report-draft.md').read_text()
decision='''**Not qualified for landing.** Literal/computed zero-spacing calls gain about 8.9× in the initial run and 9.7× in the longer recheck, with no GC-policy change. The initial key-list slowdown is +0.79% with separated ranges; its longer recheck is +0.81% with overlapping ranges. Longer ordinary small-record stringify is +0.43% with separated ranges. That fails the requested no-regression bar, so R8 remains an archived experiment; it is not proposed for merge.

The initial +0.13% sequential-access signal has overlapping ranges in the longer recheck (+0.16% median), so it is not confirmed by that recheck. All initial and longer samples remain in the evidence. None of these observations establishes that the extra floating comparison or stack-frame size causes a slowdown: the key-list route does not execute that comparison.'''
s=s.replace('DECISION_PENDING',decision)
a=json.loads((w/'recheck-analysis.json').read_text())
lines=['## Longer recheck','','Same frozen binaries and workers; eleven interleaved fresh-process repetitions, five million access iterations and two million options iterations. Options warmup remains 5,000; access has no warmup. Counts differ from the initial run, so absolute results from different windows should not be treated as a same-workload A/B. All 704 additional checksums, full options outputs, vectors, medians, recorded hashes, and both quiet windows pass the independent analyzer.','']
for kind,phase in a['phases'].items():
 win=phase['window'];lines += ['### '+kind,'',f"Window {win['started_utc']} to {win['finished_utc']}; load {win['load_before'][0]:.3f} to {win['load_after'][0]:.3f}; quiet gate passed.",'','| Workload | Main CPU | R8 CPU | Node CPU | Bun CPU | R8 vs main | Ranges |','|---|---:|---:|---:|---:|---:|---|']
 for c in phase['cases']:
  e=c['engines'];values=[f"{e[n]['cpu_us']:.6f}" for n in ['baseline','perry','node','bun']];note='separated slowdown' if c['separated_regression'] else 'separated gain' if c['separated_improvement'] else 'overlap'
  lines += ['| '+c['fixture']+' / '+c['operation']+' | '+' | '.join(values)+f" | {c['delta_pct']:+.2f}% | {note} |"]
 lines += ['','| Workload | Main RSS | R8 RSS | Node RSS | Bun RSS |','|---|---:|---:|---:|---:|']
 for c in phase['cases']:
  e=c['engines'];values=[f"{e[n]['peak_rss_mib']:.2f}" for n in ['baseline','perry','node','bun']]
  lines += ['| '+c['fixture']+' / '+c['operation']+' | '+' | '.join(values)+' |']
 slug='quiet-'+w.name+'-recheck-'+kind
 lines += ['',f'[All timed samples](results/{slug}/timing.jsonl), [output verification](results/{slug}/verify.jsonl), [sample vectors](results/{slug}/summary.json), [quiet window](results/{slug}/window.json).','']
s=s.replace('## Implementation and build evidence','\n'.join(lines)+'\n## Implementation and build evidence')
s += '\nNext experiment: retain the original compact admission block and attempt the newly eligible spacers only after it fails, preserving original arguments on fallback. Check actual machine code before timing; no benefit is assumed. [Investigation notes](results/inert-spacer-r8-validation/next-dispatch-investigation.md).\n'
(bench/'INERT_SPACER_R8.md').write_text(s)
(w/'report-draft.md').write_text(s)
print('Wrote complete R8 report with all 34 initial and 16 recheck CPU/RSS rows.')
