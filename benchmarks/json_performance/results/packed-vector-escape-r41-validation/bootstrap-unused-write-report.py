from pathlib import Path
import json
w=Path(__file__).resolve().parent;bench=w.parents[1]
read=lambda n:json.loads((w/n).read_text())
m=read('provenance.json')
lines=['# Compact parse-template capture: R41','',(w/'measurement-verdict.md').read_text().strip(),'',
 '## Scope and reference','',
 'The inherited stringify work roots callback keys, reuses escape-free string provenance safely, clears that provenance on concatenation/appending, handles decoded wide keys through the normalizing builder, and avoids primitive toJSON key copies. The inherited parser separates small inputs from the dominant-token length proof, handles valid-ED continuation bytes and copies ordinary decoded spans within bounded scratch windows. The inherited native-buffer writer retains UTF-8 validation, exact output bounds, geometric growth and WTF-8 fallback; the builder uses a bulk ASCII check. R41 changes only small-object template capture: validated local slots are initialized on demand, and an existing cached plan is updated in place. Failed partial capture leaves the previous plan intact. No managed intermediate allocation or GC entry is added. The decoder does not reserve or allocate additional scratch storage. GC policy, parse boundaries, root ordering, admission and thresholds remain unchanged.','',
 f"Measured source `{m['source_commit']}` versus frozen R26 `{m['main_build']['source_commit']}`, both workspace 0.5.1531. The harness calls the reference main/baseline; it is R26, not current main. Earlier implementation is represented by PRs #10050/#10052/#10064 with separate release metadata. This is not a merged-main measurement.",'',
 '## Validation','',(w/'validation-verdict.txt').read_text().strip(),'',
 'All 125 reference executions are reused with exact source, fixture and frozen R26 artifact hashes; all 125 candidate executions are fresh. The reference has 14 failures in the 20 original emitter checks and eight moving-stress crashes in the 20 large-emitter checks. The candidate passes both fixtures. The primitive-key and large-emitter native findings remain explicit, with identical complete reports and IR in both arms; shadow checks pass. Ten original-suite checker commands executed freshly for both arms over 18 IR files. The full set of comparisons covers 30 IR files and preserves every known finding. The original reuse refusal was due to the generated shape-census metadata change; fresh checks supersede it without changing checker code, budgets or allowlists.','',
 '## Measurement method','',
 'Quiet M1/8 GiB host, Node 26.5.1, Bun 1.3.14; fresh interleaved processes. CPU is user+system per loop iteration and peak RSS covers the entire process. Terminal windows are archived first. Repeated-input parsing includes existing caches and lazy construction; rotating inputs and consumption cases provide separate controls. Overlapping observed ranges do not establish equivalence.','']
def table(title, phase, retained=False):
    rows = phase['cases']
    window = phase['window']
    lines.extend(['### ' + title, '', f"Window: {window['started_utc']} to {window['finished_utc']}.", '',
        '| Fixture / operation | R26 µs | R41 µs | Node µs | Bun µs | CPU change | Ranges |',
        '|---|---:|---:|---:|---:|---:|---|'])
    for row in rows:
        e = row['engines']
        delta = row['cpu_delta_pct'] if retained else row['delta_pct']
        regression = row['cpu_separated_regression'] if retained else row['separated_regression']
        gain = max(e['perry']['cpu_samples_us']) < min(e['baseline']['cpu_samples_us'])
        label = row['fixture'] + ' / ' + row.get('operation', row.get('mode', '')) + (f" / {row['count']} live" if retained else '')
        lines.append('| ' + label + ' | ' + ' | '.join(f"{e[a]['cpu_us']:.6f}" for a in ['baseline', 'perry', 'node', 'bun']) + f" | {delta:+.2f}% | {'regression' if regression else 'gain' if gain else 'overlap'} |")
    lines.extend(['', '| Fixture / operation | R26 MiB | R41 MiB | Node MiB | Bun MiB | Peak RSS change MiB |',
        '|---|---:|---:|---:|---:|---:|'])
    for row in rows:
        e = row['engines']
        label = row['fixture'] + ' / ' + row.get('operation', row.get('mode', '')) + (f" / {row['count']} live" if retained else '')
        delta = e['perry']['peak_rss_mib'] - e['baseline']['peak_rss_mib']
        lines.append('| ' + label + ' | ' + ' | '.join(f"{e[a]['peak_rss_mib']:.3f}" for a in ['baseline', 'perry', 'node', 'bun']) + f" | {delta:+.3f} |")
    if retained:
        lines.extend(['', '| Fixture / operation | R26 after MiB | R41 after MiB | Node after MiB | Bun after MiB | After-RSS change MiB |', '|---|---:|---:|---:|---:|---:|'])
        for row in rows:
            e = row['engines']
            label = row['fixture'] + ' / ' + row.get('operation', row.get('mode', '')) + f" / {row['count']} live"
            lines.append('| ' + label + ' | ' + ' | '.join(f"{e[a]['rss_after_mib']:.3f}" for a in ['baseline', 'perry', 'node', 'bun']) + f" | {row['rss_after_delta_mib']:+.3f} |")
    lines.append('')

for title,filename,phase in [
 ('Post-parse array access','access-analysis.json','access'),
 ('Independent access recheck (11 repetitions)','access-recheck-analysis.json','access'),
 ('Independent recheck of full-screen regressions (11 repetitions)','regression-recheck-analysis.json','focus'),
 ('Original 38 parse/stringify plus 12 consumption cases','full-analysis.json','full'),
 ('Independent large-string stringify recheck (11 repetitions)','stringify-recheck-analysis.json','focus'),
 ('Stringify option controls','options-analysis.json','options'),
 ('Megabyte pretty printing and replacers','large-options-analysis.json','options'),
 ('Independent small option recheck','small-options-recheck-options-analysis.json','options'),
 ('Changing-object plain/zero controls','changing-options-analysis.json','options')]:
 if (w/filename).exists():table(title,read(filename)['phases'][phase])
if (w/'rotating-analysis.json').exists():table('Eight rotating inputs per fixture',read('rotating-analysis.json'))
if (w/'korean-rotating-analysis.json').exists():table('Valid Korean input controls',read('korean-rotating-analysis.json'))
if (w/'small-rotating-recheck-analysis.json').exists():table('Independent small-record recheck (11 repetitions)',read('small-rotating-recheck-analysis.json'))
if (w/'retained-analysis.json').exists():table('Retained outputs',read('retained-analysis.json'),True)
lines.extend(['## Build fingerprints','','| Artifact | R26 SHA-256 | R41 SHA-256 |','|---|---|---|'])
for name in ['perry','libperry_runtime.a','libperry_stdlib.a']:
 lines.append(f"| {name} | `{m['main_build']['files'][name]['sha256']}` | `{m['candidate_build']['files'][name]['sha256']}` |")
lines.extend(['', '## Diagnostic follow-up', '', (w/'diagnostic-findings.md').read_text().strip(), '', 'The artifact index records qualification for every benchmark window and preserves every benchmark terminal receipt, exact CPU/RSS vector, complete output check, validation log, source patch and linked-code diagnostic. The diagnostic envelope loss is described above. Every benchmark window was archived before subsequent remote operations. Process peak RSS does not by itself establish retained live-heap behavior. This is experimental-source evidence; no merged-main claim is made.', ''])
assert len(lines)<1900
(bench/'PACKED_VECTOR_ESCAPE_R41.md').write_text('\n'.join(lines))
print('Wrote R41 report with',len(lines),'lines')
