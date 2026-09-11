from pathlib import Path
import json
w=Path(__file__).resolve().parent;bench=w.parents[1]
read=lambda n:json.loads((w/n).read_text())
m=read('provenance.json')
lines=['# Direct reads of owned materialized JSON arrays: R26','',(w/'measurement-verdict.md').read_text().strip(),'',
 '## Scope and reference','',
 'The outlined lazy-array read helper now reads the exact materialized array edge owned and traced by its live lazy header. A normal GC array header, non-forwarded state and the existing length/capacity/sanity bounds permit the direct read. Growth forwarding and inconsistent headers retain the general resolver; descriptors, holes and cold construction retain the rooted accessor. Both fast and resolved paths refresh the cached length. No generic array resolver, GC policy, threshold, parse-boundary or lazy-admission change is included.','',
 f"Measured source `{m['source_commit']}` versus exact frozen R25 `{m['main_build']['source_commit']}`, both workspace 0.5.1531. The harness calls the reference main/baseline; it is R25, not current main. Earlier R24/R25 implementation bytes are in ready PRs #10050/#10052 with separate release metadata. This experiment is not a merged-main measurement.",'',
 '## Validation','',
 (w/'validation-verdict.txt').read_text().strip(),'',
 'All 81 reference behavior/options receipts are reused from the exact R25 build. Candidate executions are fresh. Original compiler input paths are held fixed; native IR normalizes only the ModuleID path comment, and shadow IR must match byte-for-byte. The original 18 checker verdicts are reused only after exact emitted-IR and checker-source equivalence; the candidate zero-spacing, changing-record and retained-zero subjects run fresh checker commands, while their reference verdicts are reused from R25. Existing findings remain unsuppressed.','',
 '## Measurement method','',
 'Quiet M1/8 GiB host, Node 26.5.1, Bun 1.3.14; fresh interleaved processes. CPU is user+system per loop iteration, whole-process peak RSS is reported separately. Every terminal window is archived first, including failures. Access loops measure post-parse reads; repeated-input parse includes existing caches/lazy construction and is complemented by consumption and rotating-input cases. Retained outputs remain live. One-operation CPU rows have limited timer resolution. Overlapping ranges do not establish equivalence.','']
def table(title, phase, retained=False):
    rows = phase['cases']
    window = phase['window']
    lines.extend(['### ' + title, '', f"Window: {window['started_utc']} to {window['finished_utc']}.", '',
        '| Fixture / operation | R25 µs | R26 µs | Node µs | Bun µs | CPU change | Ranges |',
        '|---|---:|---:|---:|---:|---:|---|'])
    for row in rows:
        e = row['engines']
        delta = row['cpu_delta_pct'] if retained else row['delta_pct']
        regression = row['cpu_separated_regression'] if retained else row['separated_regression']
        gain = max(e['perry']['cpu_samples_us']) < min(e['baseline']['cpu_samples_us'])
        label = row['fixture'] + ' / ' + row.get('operation', row.get('mode', '')) + (f" / {row['count']} live" if retained else '')
        lines.append('| ' + label + ' | ' + ' | '.join(f"{e[a]['cpu_us']:.6f}" for a in ['baseline', 'perry', 'node', 'bun']) + f" | {delta:+.2f}% | {'regression' if regression else 'gain' if gain else 'overlap'} |")
    lines.extend(['', '| Fixture / operation | R25 MiB | R26 MiB | Node MiB | Bun MiB | Peak RSS change MiB |',
        '|---|---:|---:|---:|---:|---:|'])
    for row in rows:
        e = row['engines']
        label = row['fixture'] + ' / ' + row.get('operation', row.get('mode', '')) + (f" / {row['count']} live" if retained else '')
        delta = e['perry']['peak_rss_mib'] - e['baseline']['peak_rss_mib']
        lines.append('| ' + label + ' | ' + ' | '.join(f"{e[a]['peak_rss_mib']:.3f}" for a in ['baseline', 'perry', 'node', 'bun']) + f" | {delta:+.3f} |")
    if retained:
        lines.extend(['', '| Fixture / operation | R25 after MiB | R26 after MiB | Node after MiB | Bun after MiB | After-RSS change MiB |', '|---|---:|---:|---:|---:|---:|'])
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
 ('Stringify option controls','options-analysis.json','options')]:
 if (w/filename).exists():table(title,read(filename)['phases'][phase])
if (w/'rotating-analysis.json').exists():table('Eight rotating inputs per fixture',read('rotating-analysis.json'))
if (w/'retained-analysis.json').exists():table('Retained outputs',read('retained-analysis.json'),True)
lines.extend(['## Build fingerprints','','| Artifact | R25 SHA-256 | R26 SHA-256 |','|---|---|---|'])
for name in ['perry','libperry_runtime.a','libperry_stdlib.a']:
 lines.append(f"| {name} | `{m['main_build']['files'][name]['sha256']}` | `{m['candidate_build']['files'][name]['sha256']}` |")
lines.extend(['','The artifact index records archived raw windows, exact sample vectors, validation receipts, source patches and failed attempts. Independent R25 parsing profiles and a future UTF-16-length model, if included in the validation archive, are explicitly diagnostic/proposal evidence and are not R26 performance results.',''])
assert len(lines)<1900
(bench/'TRUSTED_MATERIALIZED_READ_R26.md').write_text('\n'.join(lines))
print('Wrote R26 report with',len(lines),'lines')
