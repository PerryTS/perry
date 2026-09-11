from pathlib import Path
import json
w=Path(__file__).resolve().parent;bench=w.parents[1]
read=lambda n:json.loads((w/n).read_text())
m=read('provenance.json')
lines=['# Isolated JSON scanning and bounded escape spans: R36','',(w/'measurement-verdict.md').read_text().strip(),'',
 '## Scope and reference','',
 'The inherited stringify work roots callback keys, reuses escape-free string provenance safely, clears that provenance on concatenation/appending, handles decoded wide keys through the normalizing builder, and avoids primitive toJSON key copies. R36 restores the original generic tape delimiter scanner, uses the surrogate-aware scanner only for borrowed value tokens, explicitly outlines the scanned string constructor, and copies ordinary escaped-string spans eight bytes at a time within existing 64-byte input/output windows. The decoder does not reserve or allocate additional scratch storage. GC policy, parse boundaries, root ordering, admission and thresholds remain unchanged.','',
 f"Measured source `{m['source_commit']}` versus frozen R26 `{m['main_build']['source_commit']}`, both workspace 0.5.1531. The harness calls the reference main/baseline; it is R26, not current main. Earlier implementation is represented by PRs #10050/#10052/#10064 with separate release metadata. This is not a merged-main measurement.",'',
 '## Validation','',(w/'validation-verdict.txt').read_text().strip(),'',
 'All 81 original reference behavior/options receipts and the 24 added reference fixture receipts are reused with exact source, fixture and frozen R26 artifact hash checks. All 81 original candidate executions are fresh. The expanded emitter regression has 20 fresh candidate executions and 20 verified reference receipts: the reference has fourteen output/callback failures, while the candidate passes all 20. Native/shadow checker comparison covers the emitter and primitive-key fixtures. Four candidate primitive-key executions match complete Node output and the verified reference receipts; the native checker finding described above remains visible. The original 18 checker verdicts are reused only after exact emitted-IR and checker-source equivalence; existing findings remain unsuppressed.','',
 '## Measurement method','',
 'Quiet M1/8 GiB host, Node 26.5.1, Bun 1.3.14; fresh interleaved processes. CPU is user+system per loop iteration and peak RSS covers the entire process. Terminal windows are archived first. Repeated-input parsing includes existing caches and lazy construction; rotating inputs and consumption cases provide separate controls. Overlapping observed ranges do not establish equivalence.','']
def table(title, phase, retained=False):
    rows = phase['cases']
    window = phase['window']
    lines.extend(['### ' + title, '', f"Window: {window['started_utc']} to {window['finished_utc']}.", '',
        '| Fixture / operation | R26 µs | R36 µs | Node µs | Bun µs | CPU change | Ranges |',
        '|---|---:|---:|---:|---:|---:|---|'])
    for row in rows:
        e = row['engines']
        delta = row['cpu_delta_pct'] if retained else row['delta_pct']
        regression = row['cpu_separated_regression'] if retained else row['separated_regression']
        gain = max(e['perry']['cpu_samples_us']) < min(e['baseline']['cpu_samples_us'])
        label = row['fixture'] + ' / ' + row.get('operation', row.get('mode', '')) + (f" / {row['count']} live" if retained else '')
        lines.append('| ' + label + ' | ' + ' | '.join(f"{e[a]['cpu_us']:.6f}" for a in ['baseline', 'perry', 'node', 'bun']) + f" | {delta:+.2f}% | {'regression' if regression else 'gain' if gain else 'overlap'} |")
    lines.extend(['', '| Fixture / operation | R26 MiB | R36 MiB | Node MiB | Bun MiB | Peak RSS change MiB |',
        '|---|---:|---:|---:|---:|---:|'])
    for row in rows:
        e = row['engines']
        label = row['fixture'] + ' / ' + row.get('operation', row.get('mode', '')) + (f" / {row['count']} live" if retained else '')
        delta = e['perry']['peak_rss_mib'] - e['baseline']['peak_rss_mib']
        lines.append('| ' + label + ' | ' + ' | '.join(f"{e[a]['peak_rss_mib']:.3f}" for a in ['baseline', 'perry', 'node', 'bun']) + f" | {delta:+.3f} |")
    if retained:
        lines.extend(['', '| Fixture / operation | R26 after MiB | R36 after MiB | Node after MiB | Bun after MiB | After-RSS change MiB |', '|---|---:|---:|---:|---:|---:|'])
        for row in rows:
            e = row['engines']
            label = row['fixture'] + ' / ' + row.get('operation', row.get('mode', '')) + f" / {row['count']} live"
            lines.append('| ' + label + ' | ' + ' | '.join(f"{e[a]['rss_after_mib']:.3f}" for a in ['baseline', 'perry', 'node', 'bun']) + f" | {row['rss_after_delta_mib']:+.3f} |")
    lines.append('')

for title,filename,phase in [
 ('Post-parse array access','access-analysis.json','access'),
 ('Independent access recheck (11 repetitions)','access-recheck-analysis.json','access'),
 ('Historical regression screen (11 repetitions)','regression-recheck-analysis.json','focus'),
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
lines.extend(['## Build fingerprints','','| Artifact | R26 SHA-256 | R36 SHA-256 |','|---|---|---|'])
for name in ['perry','libperry_runtime.a','libperry_stdlib.a']:
 lines.append(f"| {name} | `{m['main_build']['files'][name]['sha256']}` | `{m['candidate_build']['files'][name]['sha256']}` |")
lines.extend(['', '## Diagnostic follow-up', '', (w/'diagnostic-findings.md').read_text().strip(), '', 'The artifact index records both terminal windows, exact CPU/peak-RSS vectors, full output checks, validation receipts, source patch and linked-code diagnostics. Both remote controllers exited zero and archived their windows before subsequent remote actions. Median peak-RSS differences across the 21 comparisons range from -48 KiB to +128 KiB. These process peaks do not establish retained live-heap behavior. The candidate has no ready PR or merged-main measurement.', ''])
assert len(lines)<1900
(bench/'ISOLATED_JSON_SCAN_CHUNKS_R36.md').write_text('\n'.join(lines))
print('Wrote R36 report with',len(lines),'lines')
