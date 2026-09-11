from pathlib import Path
import json
w=Path(__file__).resolve().parent;bench=w.parents[1]
read=lambda n:json.loads((w/n).read_text())
m=read('provenance.json')
lines=['# Reuse source UTF-16 length for large JSON tokens: R27','',(w/'measurement-verdict.md').read_text().strip(),'',
 '## Scope and reference','',
 'A large unescaped string token can derive its UTF-16 length from the exact rooted input string when at most 256 surrounding bytes are ASCII. Payload pointer and byte length must both match. A conservative final-byte check rejects sequences that could cross the token boundary under the existing bounded WTF-8 counter. Escaped tokens and every failed proof retain their original counter/allocator. The known-length path uses the identical allocation body, flags, padding, copy and malloc-output accounting. No GC policy, parse-boundary, cache admission or threshold change.','',
 f"Measured source `{m['source_commit']}` versus frozen R26 `{m['main_build']['source_commit']}`, both workspace 0.5.1531. The harness calls the reference main/baseline; it is R26, not current main. Earlier implementation is in ready PRs #10050/#10052/#10064 with separate release metadata. This is not a merged-main measurement.",'',
 '## Validation','',(w/'validation-verdict.txt').read_text().strip(),'',
 'The original 81 reference behavior/options receipts are reused from the exact R26 build; candidate executions are fresh. Nine added source-token cases run freshly on both arms. The original 18 checker verdicts are reused only after exact emitted-IR and checker-source equivalence. Added token native/shadow checker runs are fresh on both arms. Existing findings remain unsuppressed.','',
 '## Measurement method','',
 'Quiet M1/8 GiB host, Node 26.5.1, Bun 1.3.14; fresh interleaved processes. CPU is user+system per loop iteration and peak RSS covers the entire process. Terminal windows are archived first. Repeated-input parsing includes existing caches and lazy construction; rotating inputs and consumption cases provide separate controls. Overlapping observed ranges do not establish equivalence.','']
def table(title, phase, retained=False):
    rows = phase['cases']
    window = phase['window']
    lines.extend(['### ' + title, '', f"Window: {window['started_utc']} to {window['finished_utc']}.", '',
        '| Fixture / operation | R26 µs | R27 µs | Node µs | Bun µs | CPU change | Ranges |',
        '|---|---:|---:|---:|---:|---:|---|'])
    for row in rows:
        e = row['engines']
        delta = row['cpu_delta_pct'] if retained else row['delta_pct']
        regression = row['cpu_separated_regression'] if retained else row['separated_regression']
        gain = max(e['perry']['cpu_samples_us']) < min(e['baseline']['cpu_samples_us'])
        label = row['fixture'] + ' / ' + row.get('operation', row.get('mode', '')) + (f" / {row['count']} live" if retained else '')
        lines.append('| ' + label + ' | ' + ' | '.join(f"{e[a]['cpu_us']:.6f}" for a in ['baseline', 'perry', 'node', 'bun']) + f" | {delta:+.2f}% | {'regression' if regression else 'gain' if gain else 'overlap'} |")
    lines.extend(['', '| Fixture / operation | R26 MiB | R27 MiB | Node MiB | Bun MiB | Peak RSS change MiB |',
        '|---|---:|---:|---:|---:|---:|'])
    for row in rows:
        e = row['engines']
        label = row['fixture'] + ' / ' + row.get('operation', row.get('mode', '')) + (f" / {row['count']} live" if retained else '')
        delta = e['perry']['peak_rss_mib'] - e['baseline']['peak_rss_mib']
        lines.append('| ' + label + ' | ' + ' | '.join(f"{e[a]['peak_rss_mib']:.3f}" for a in ['baseline', 'perry', 'node', 'bun']) + f" | {delta:+.3f} |")
    if retained:
        lines.extend(['', '| Fixture / operation | R26 after MiB | R27 after MiB | Node after MiB | Bun after MiB | After-RSS change MiB |', '|---|---:|---:|---:|---:|---:|'])
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
if (w/'small-rotating-recheck-analysis.json').exists():table('Independent small-record recheck (11 repetitions)',read('small-rotating-recheck-analysis.json'))
if (w/'retained-analysis.json').exists():table('Retained outputs',read('retained-analysis.json'),True)
lines.extend(['## Build fingerprints','','| Artifact | R26 SHA-256 | R27 SHA-256 |','|---|---|---|'])
for name in ['perry','libperry_runtime.a','libperry_stdlib.a']:
 lines.append(f"| {name} | `{m['main_build']['files'][name]['sha256']}` | `{m['candidate_build']['files'][name]['sha256']}` |")
lines.extend(['','The artifact index records archived raw windows, exact sample vectors, validation receipts, source patches and failed attempts. Independent R26 stringify profiles and future scanner/emitter proposals, if included, are diagnostic/proposal evidence and are not R27 performance results.',''])
assert len(lines)<1900
(bench/'SOURCE_TOKEN_LENGTH_R27.md').write_text('\n'.join(lines))
print('Wrote R27 report with',len(lines),'lines')
