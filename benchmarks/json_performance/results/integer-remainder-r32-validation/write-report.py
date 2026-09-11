from pathlib import Path
import json
w=Path(__file__).resolve().parent;bench=w.parents[1]
read=lambda n:json.loads((w/n).read_text())
m=read('provenance.json')
lines=['# Exact integer remainder for JSON access: R32','',(w/'measurement-verdict.md').read_text().strip(),'',
'## Scope and reference','',
'When both dynamic-remainder operands are already numeric doubles, exact nonnegative-u32 dividends and positive-u32 divisors use integer remainder. Cast round trips and a dividend sign check preserve fractions, NaN/infinity and negative zero on the existing floating-point path. Tagged numbers, objects/coercion and BigInt retain their existing paths. No allocation, GC policy, threshold, parse-boundary, parser, serializer or codegen changes. This improves workload index arithmetic such as i % records.length; it is not a new JSON parsing algorithm.','',
f"Measured source `{m['source_commit']}` against frozen R26 `{m['main_build']['source_commit']}` at workspace 0.5.1531. The harness names R26 main/baseline; it is not current main. Release metadata is prepared separately on top of PR #10064; no metadata or merged-main production measurement is claimed.",'',
'## Validation','',(w/'validation-verdict.txt').read_text().strip(),'',
'## Method and limits','',
'Quiet M1 / 8 GiB host, Node 26.5.1 and Bun 1.3.14, interleaved fresh processes. CPU is user+system per loop iteration. Peak RSS is process-wide, including startup and fixture setup, and is not retained-live-heap size. Each terminal remote window was archived before any subsequent remote operation. Full output/checksum, worker/input hashes, every sample vector and median were verified. Overlapping observed ranges do not establish equality. Repeat-input rows include existing caches/lazy construction; rotating and consuming rows are separate controls.','']
def table(title, phase, retained=False):
    rows = phase['cases']
    window = phase['window']
    lines.extend(['### ' + title, '', f"Window: {window['started_utc']} to {window['finished_utc']}.", '',
        '| Fixture / operation | R26 µs | R32 µs | Node µs | Bun µs | CPU change | Ranges |',
        '|---|---:|---:|---:|---:|---:|---|'])
    for row in rows:
        e = row['engines']
        delta = row['cpu_delta_pct'] if retained else row['delta_pct']
        regression = row['cpu_separated_regression'] if retained else row['separated_regression']
        gain = max(e['perry']['cpu_samples_us']) < min(e['baseline']['cpu_samples_us'])
        label = row['fixture'] + ' / ' + row.get('operation', row.get('mode', '')) + (f" / {row['count']} live" if retained else '')
        lines.append('| ' + label + ' | ' + ' | '.join(f"{e[a]['cpu_us']:.6f}" for a in ['baseline', 'perry', 'node', 'bun']) + f" | {delta:+.2f}% | {'regression' if regression else 'gain' if gain else 'overlap'} |")
    lines.extend(['', '| Fixture / operation | R26 MiB | R32 MiB | Node MiB | Bun MiB | Peak RSS change MiB |',
        '|---|---:|---:|---:|---:|---:|'])
    for row in rows:
        e = row['engines']
        label = row['fixture'] + ' / ' + row.get('operation', row.get('mode', '')) + (f" / {row['count']} live" if retained else '')
        delta = e['perry']['peak_rss_mib'] - e['baseline']['peak_rss_mib']
        lines.append('| ' + label + ' | ' + ' | '.join(f"{e[a]['peak_rss_mib']:.3f}" for a in ['baseline', 'perry', 'node', 'bun']) + f" | {delta:+.3f} |")
    if retained:
        lines.extend(['', '| Fixture / operation | R26 after MiB | R32 after MiB | Node after MiB | Bun after MiB | After-RSS change MiB |', '|---|---:|---:|---:|---:|---:|'])
        for row in rows:
            e = row['engines']
            label = row['fixture'] + ' / ' + row.get('operation', row.get('mode', '')) + f" / {row['count']} live"
            lines.append('| ' + label + ' | ' + ' | '.join(f"{e[a]['rss_after_mib']:.3f}" for a in ['baseline', 'perry', 'node', 'bun']) + f" | {row['rss_after_delta_mib']:+.3f} |")
    lines.append('')

for title,filename,phase in [
('Array access','access-analysis.json','access'),
('Independent access recheck (11 repetitions)','access-recheck-analysis.json','access'),
('Original 38 parse/stringify plus 12 consumption rows','full-analysis.json','full'),
('Stringify options','options-analysis.json','options')]:
 if (w/filename).exists():table(title,read(filename)['phases'][phase])
if (w/'rotating-analysis.json').exists():table('Eight rotating inputs per fixture',read('rotating-analysis.json'))
if (w/'escaped-rotating-recheck-analysis.json').exists():table('Independent escaped-input recheck (11 repetitions)',read('escaped-rotating-recheck-analysis.json'))
lines.extend(['## Build fingerprints','','| Artifact | R26 SHA-256 | R32 SHA-256 |','|---|---|---|'])
for name in ['perry','libperry_runtime.a','libperry_stdlib.a']:
 lines.append(f"| {name} | `{m['main_build']['files'][name]['sha256']}` | `{m['candidate_build']['files'][name]['sha256']}` |")
lines.extend(['','The unit controller initially stopped at its explicit hold before any production build began. After local lint review, the clean frozen source completed the normal three-package production build. The reference/candidate remainder fixture exposes one identical unsuppressed shadow root-store-order finding; the initial comparator failure and its exact-fingerprint classification remain in the archive. No global allowlist was changed. Local debugger snapshots used synthetic inputs and are diagnostic only, not measurement results.',''])
assert len(lines)<1900
(bench/'INTEGER_REMAINDER_R32.md').write_text('\n'.join(lines))
print('Wrote R32 report with',len(lines),'lines')
