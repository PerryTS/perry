#!/usr/bin/env python3
"""Generate the review report from retained raw measurements."""
import argparse,json,re,statistics
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('evidence',type=Path);p.add_argument('output',type=Path);a=p.parse_args()
data=a.evidence
ab=json.loads((data/'ab-final.json').read_text());pauses=json.loads((data/'pause-final.json').read_text())
names=['hello','tscwork','zodwork','qs_parse','qs_stringify','commander','fastify','effect','property-prototype','shapes-options','schema-record','root-survival','buffer_heavy','worker_heavy','class_getter','object_array','class_virtual','object_spread']
def rows(source,name,arm,iterations=None):
 return [r for r in source if r['name']==name and r['arm']==arm and r['iterations']==iterations]
def med(source,name,arm,key,iterations=None):
 rs=rows(source,name,arm,iterations);assert len(rs)==5,(name,arm,len(rs));return statistics.median(r[key] for r in rs)
def pair(source,name,key,iterations=None):return [med(source,name,arm,key,iterations) for arm in ['obs-main','obs-head']]
def change(values):return 100*(values[1]/values[0]-1)
def ints(values):return ' → '.join(f'{x:,.0f}' for x in values)
out=['Keylists lane, 2026-10-11. Base f98073e991d1c6e2d5ba29adee2dc7c41649df7e. **Draft; not ready to land.** Effect, schema-record, worker and buffer regress beyond the small-regression allowance. The native root gate also reproduces an existing finding on main. No versions changed.',
 'Tables deleted: **none**. O5 deletion probes show both caches pay, so they remain. Canonical trie and ShapeTable remain shape authorities. Physically deleted `scan_canonical_keys_roots_mut` and `scan_class_keys_roots_mut`, their call sites, and their rekey-census entries. Canonical/class death walks run only after full traces. Static shape-cache minors return immediately; its full scanner still supplies strong roots.',
 'Canonical/class backings are old from birth and carry the existing shared-layout movement bit. Both old defrag and idle compaction use the mover that excludes that bit. The bit does not pin liveness; full traces still reclaim dead weak backings. Remembered slots keep young key strings and attributes correct. No emitted operation/site changes: **0 bytes per site before/after delta**. Pure shipping `.text` shrinks 1,664–1,728 bytes per program.',
 '**Unfinished removal:** ShapeTable rekey and young logs still service independently owned mutable tombstone/Array-subclass lists and L3 prototype words. Removing that residual support now breaks moving-GC correctness. Transition-cache key words can also still move until L2. The census enumerates retained holders; deleted entries were removed rather than suppressed.',
 'A/B: qb6, CPUs 0–23,32–55, ASLR off, THP disabled and verified (`AnonHugePages=0`), diagnostics off, n=5 interleaved, no measurement lock. Identical private observation code records every collector completion, including worker threads and synchronous fulls; two atomic adds per collection, no per-operation work. Both arms use the same saved frontend objects because codegen is unchanged. Pure twins link the original normal runtime graphs and supply `.text`.',
 '| Program | Instructions main → head | Δ | Peak RSS KiB main → head | Full/minor main → head | `.text` bytes main → head |',
 '|---|---:|---:|---:|---:|---:|']
for n in names:
 count=200000 if n.startswith('class_') or n in ['object_array','object_spread'] else None
 ins=pair(ab,n,'instructions',count);rss=pair(ab,n,'rss_kb',count);text=pair(ab,n,'text',count)
 counts=['%g/%g'%(med(ab,n,arm,'full',count),med(ab,n,arm,'minor',count)) for arm in ['obs-main','obs-head']]
 out.append(f'| {n} | {ints(ins)} | {change(ins):+.3f}% | {ints(rss)} | '+ ' → '.join(counts)+f' | {ints(text)} |')
out += ['', 'Worker full counts range 40–42; both arm medians are 41. Buffer and worker now pass main, so both are included and gated. The previously reported failures #12091/#12092 did not reproduce.',
 'Class-micro metric uses the mandated 50k/200k slope; total-instruction rows above use 200k. No class regression is hidden in startup cost.',
 '| Micro | Instructions/iteration main | Head | Δ |','|---|---:|---:|---:|']
for n in names[-4:]:
 v=[(med(ab,n,arm,'instructions',200000)-med(ab,n,arm,'instructions',50000))/150000 for arm in ['obs-main','obs-head']]
 out.append(f'| {n} | {v[0]:.4f} | {v[1]:.4f} | {change(v):+.5f}% |')
out += ['', 'GC pause is the sum of active collector durations, including fulls and all worker collectors; worker sums are not process-wide stop-the-world duration. Diagnostics off, n=5 interleaved under `MEASURE.lock`, CPUs 56–63, ASLR off. **Sibling CPUs 24–31 reached 100% utilization during these runs. These are noisy observations, not a reliable claim of an 8–24 ms saving per program.**',
 '| Program | Main GC ms | Head GC ms | Head − main ms |','|---|---:|---:|---:|']
for n in names:
 count=200000 if n in names[-4:] else None
 v=[x/1000 for x in pair(pauses,n,'gc_pause_us',count)]
 out.append(f'| {n} | {v[0]:.3f} | {v[1]:.3f} | {v[1]-v[0]:+.3f} |')
out += ['', 'Diagnostic attribution only: one locked run per arm with `PERRY_GC_DIAG=1`. The following totals cover copying-minor per-prune reports and scanner-profile reports; they are not complete GC pause. No diagnostic-on instructions/RSS are used for acceptance.',
 '| Program | Canonical minor prune ms main → head | Shape scanner ms main → head |','|---|---:|---:|']
for n in names[:14]:
 v=[]
 for arm in ['obs-main','obs-head']:
  s=(data/arm/(n+'.trial0.nNone.diag.err')).read_text()
  prune=sum(int(x) for line in s.splitlines() if line.startswith('[gc-copy-minor] ran') for x in re.findall(r'"CANONICAL_KEYS \(canonical keys trie\)":(\d+)',line))
  scan=sum(int(x) for x in re.findall(r'\[gc-scanner-profile\]\s+crate::object::shapes::scan_shape_table_rekey_mut us=(\d+)',s))
  v.append((prune/1000,scan/1000))
 out.append(f'| {n} | {v[0][0]:.3f} → {v[1][0]:.3f} | {v[0][1]:.3f} → {v[1][1]:.3f} |')
out += ['', 'O5 probes use separate sources and targets; no shipped runtime switch. Lookup/publication removal is enough to reject deletion when an important workload loses substantially. Probe reference predates the final empty-cache custody fix (320 runtime text bytes); final A/B above includes that fix. Probe rows include both shipping-arm text sizes, RSS, and collection counts.',
 '| Removed cache | Program | Instructions reference → probe | Δ | RSS KiB reference → probe | Full/minor reference → probe | `.text` bytes reference → probe |','|---|---|---:|---:|---:|---:|---:|']
for label,file,arm in [('transition','no-transition-ab-raw.json','no-transition'),('shape','no-shape-ab-raw.json','no-shape')]:
 source=json.loads((data/file).read_text())
 for n in ['zodwork','qs_parse','qs_stringify',*names[-4:]]:
  count=200000 if n in names[-4:] else None
  ins=[med(source,n,x,'instructions',count) for x in ['head',arm]]
  rss=[med(source,n,x,'rss_kb',count) for x in ['head',arm]]
  text=[med(source,n,x,'text',count) for x in ['head',arm]]
  counts=['%g/%g'%(med(source,n,x,'full',count),med(source,n,x,'minor',count)) for x in ['head',arm]]
  out.append(f'| {label} | {n} | {ints(ins)} | {change(ins):+.3f}% | {ints(rss)} | '+ ' → '.join(counts)+f' | {ints(text)} |')
out += ['', 'The transition hit avoids repeated canonical-trie probes and shape/facts construction: deletion costs Zod +88.2%, qs_parse +45.3%, object_spread +319.8% instructions. Static-shape hits avoid canonicalization/descriptor work: deleting the shape cache costs Zod +4.3%. Both pay.',
 'Performance finding: old birth removes nursery key-list work but shifts collection timing and promotion (Effect minors 8→6; schema 14→13; Fastify 29→37), and adds old-allocation/page-index and remembered-slot work. Instruction samples show added valid-pointer construction, hash-map insertion, and dirty-slot scanning. Exit trie backing counts/bytes are identical for Effect and schema, so their peak RSS growth is not an unbounded trie leak. The complete excess instruction cost has not been isolated enough to satisfy the perf policy. **Do not ship this draft.** Fix-forward work should isolate pinned old storage from movable cohorts and review trigger/promotion accounting, then repeat the affected A/Bs; I2 atoms and I1 prototypes also remain dependencies for complete minor-scanner removal.',
 'Correctness: both observed and pure arms match Node on 12 programs, buffer/worker, and all four class micros (18/18 each). All 18 candidate workloads pass moving-GC stress with force-evacuation, evacuation verification, protected from-space and from-space scanning. Allocating workloads exercise hundreds/thousands of minors. All I3 stationary-backings/moving-children, full-death, old-allocation slow-path, and pin-removal controls pass.',
 'Full release runtime suite: head 5,694 pass / 7 fail / 5 ignored; untouched main 5,696 pass / the same 7 fail / 5 ignored. Existing failures: two array descriptor-rebind padding checks; perex exact-length root; dense constructor capture replay; stored-undefined own front; typed-prefix packed append; typed-feedback array-literal arm. Test fixtures now preserve retained old object indices/remembering and use owned mutable lists where a relocation witness is required; dead receiver storage is not dereferenced.',
 'Stock root-dominance self-test, all five `--audit-*` checks, fresh-archive symbol verification, root-holder gate and its 92 mutation controls, rekey census and its 12 controls, and pin-site checks pass. Root holders: 1,499 declarations, 147 scanners; rekey census: 42 sites, 27 prunes, no declared gaps. Native corpus: 312/312 sources, 356 modules, 9,809 functions, 100,450 statepoints, 70,416 live bundles, 318,829 relocates; 40/40 seeded violations caught, no immovable exemptions. **Native corpus gate remains red:** one stale finding in `array_record_stack_custody`, reproduced by the original checker and untouched main. Its zero budget remains intact.',
 'The indexed test harness uses the same hazard/window/protection predicates and unchanged budgets. It indexes candidate instructions and dominator ancestry to avoid a 254k-instruction fixture repeatedly scanning its whole body. Equivalence validation runs reference and indexed implementations on every stock self-test. The original checker reproduces the reported real-corpus finding. An additional 11830 per-file seed experiment misses one numeric-only candidate on both arms; the full-corpus 40-site mutation gate passes. Address-class inventory and shape-descriptor census also reproduce existing baseline drift; neither gate was weakened.',
 'Raw evidence: `scratchpad/codex-small/keylists-evidence/` contains all n=5 rows, diagnostic logs, output/stress statuses, unit-suite logs, profiles, source audits and baseline controls. `touched-files.txt` lists every repository file changed. Build targets are removed after evidence and bundle verification. Bundle: `/Users/amlug/projects/perry/secret-tests/scratchpad/codex-small/keylists.bundle`.']
a.output.write_text('\n\n'.join(out[:5])+'\n\n'+'\n'.join(out[5:])+'\n')
