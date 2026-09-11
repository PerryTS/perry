from pathlib import Path
import hashlib, json

w = Path(__file__).resolve().parent
bench = w.parents[1]
read = lambda name: json.loads((w / name).read_text())
meta = read('provenance.json')
lines = ['# Canonical zero-spacing stringify: R25', '', (w / 'measurement-verdict.md').read_text().strip(), '',
    '## Scope and reference', '',
    'The canonical primitive/string/record/flat emitters now accept numeric +0, -0 and tagged INT32 zero as the spacer when the replacer is null or undefined. The separate lazy raw-source-copy predicate remains unchanged. Replacers, observable spacer coercions, descriptors and toJSON keep the existing semantic fallbacks. No production GC policy, threshold or parse-boundary change is included.', '',
    f"Candidate source: `{meta['source_commit']}`. Reference: measured R24 `{meta['main_build']['source_commit']}`. Both use workspace version 0.5.1531 and the same production build command. The harness calls the reference `main`/`baseline`; it is R24, **not current main**. PR #10050 contains the same R24 implementation/test bytes with separate release metadata; this experiment does not claim a merged-main measurement.", '',
    'The four original benchmark object files and the two added control objects match between arms. Original R24 behavior/options results are reused with exact hashes and provenance. The new zero-spacing fixture and both new controls run against both frozen builds. Native and shadow IR comparisons normalize only the native ModuleID path comment.', '',
    '## Validation and known limitations', '',
    '295 serial release JSON runtime tests pass on the final measured source. The initial new cache-hit witness failed after only four cold calls; the established six-field fixture and eight output-verified calls cover cache admission before requiring reuse. The failed run and source are preserved. Formatting was corrected before the final build. Script lint passes 73 of 74 executed checks, retaining the existing public benchmark freshness failure; file-size checking passes. Compile-tier and CI-only lint checks are not represented as passes.', '',
    'Each arm has 46 original behavior comparisons, 14 original option comparisons, nine zero-spacing semantic/GC comparisons, four changing-record comparisons and eight retained-zero comparisons. Every scheduled subject reports positive moved-object and protected-from-space counters. The new semantic fixture checks getters, own/inherited toJSON, mutations after caching, boxed-spacer coercion, replacers and live retained outputs.', '',
    'The new zero-spacing fixture and retained-zero worker pass native and shadow root checks. The changing-record worker passes shadow checks but retains four unsuppressed R24 native unrooted/global property-write findings. The original fixture set retains its 16 unsuppressed native findings. Matching IR/fingerprints establish an unchanged baseline, not a clean native safety verdict. The original 18 IR files reuse their previously executed checker verdicts only after exact emitted-IR and checker-source equivalence; the three added subjects run fresh checks in both arms.', '',
    'The existing lazy-spacer crashes/noncanonical outputs, lazy-getter failures and fractional-spacing difference remain explicit baseline outcomes. They are not fixed or counted as conformance passes. The zero-spacing fast path does not widen the lazy source-copy shortcut.', '',
    '## Measurement method', '',
    'Quiet M1/8 GiB host, Node 26.5.1 and Bun 1.3.14. Fresh processes and interleaved engine order; fixed workloads and seven repetitions unless a recheck says otherwise. Every terminal window is archived before any subsequent remote operation, including failed attempts. CPU is user+system time per measured loop iteration; RSS is whole-process peak memory. Changing-record loops include the per-iteration mutation. Retained loops include retaining results. One-operation retained CPU measurements are cold and have limited timer resolution; use the larger-count rows for throughput comparisons. Repeated-input parse timings include existing input caches and lazy construction. Consumption loops measure additional access work separately. Overlapping sample ranges do not establish equivalence.', '']

def table(title, phase, retained=False):
    rows = phase['cases']
    window = phase['window']
    lines.extend(['### ' + title, '', f"Window: {window['started_utc']} to {window['finished_utc']}.", '',
        '| Fixture / operation | R24 µs | R25 µs | Node µs | Bun µs | CPU change | Ranges |',
        '|---|---:|---:|---:|---:|---:|---|'])
    for row in rows:
        e = row['engines']
        delta = row['cpu_delta_pct'] if retained else row['delta_pct']
        regression = row['cpu_separated_regression'] if retained else row['separated_regression']
        gain = max(e['perry']['cpu_samples_us']) < min(e['baseline']['cpu_samples_us'])
        label = row['fixture'] + ' / ' + row.get('operation', row.get('mode', '')) + (f" / {row['count']} live" if retained else '')
        lines.append('| ' + label + ' | ' + ' | '.join(f"{e[a]['cpu_us']:.6f}" for a in ['baseline', 'perry', 'node', 'bun']) + f" | {delta:+.2f}% | {'regression' if regression else 'gain' if gain else 'overlap'} |")
    lines.extend(['', '| Fixture / operation | R24 MiB | R25 MiB | Node MiB | Bun MiB | Peak RSS change MiB |',
        '|---|---:|---:|---:|---:|---:|'])
    for row in rows:
        e = row['engines']
        label = row['fixture'] + ' / ' + row.get('operation', row.get('mode', '')) + (f" / {row['count']} live" if retained else '')
        delta = e['perry']['peak_rss_mib'] - e['baseline']['peak_rss_mib']
        lines.append('| ' + label + ' | ' + ' | '.join(f"{e[a]['peak_rss_mib']:.3f}" for a in ['baseline', 'perry', 'node', 'bun']) + f" | {delta:+.3f} |")
    if retained:
        lines.extend(['', '| Fixture / operation | R24 after MiB | R25 after MiB | Node after MiB | Bun after MiB | After-RSS change MiB |', '|---|---:|---:|---:|---:|---:|'])
        for row in rows:
            e = row['engines']
            label = row['fixture'] + ' / ' + row.get('operation', row.get('mode', '')) + f" / {row['count']} live"
            lines.append('| ' + label + ' | ' + ' | '.join(f"{e[a]['rss_after_mib']:.3f}" for a in ['baseline', 'perry', 'node', 'bun']) + f" | {row['rss_after_delta_mib']:+.3f} |")
    lines.append('')

for title, filename, phase in [
    ('Original stringify-option cases', 'options-analysis.json', 'options'),
    ('Changing record before every stringify', 'changing-options-analysis.json', 'options'),
    ('One-megabyte strings and Unicode', 'large-options-analysis.json', 'options'),
    ('Independent ASCII plain recheck (11 repetitions)', 'large-recheck-options-analysis.json', 'options'),
    ('Original 38 plus 12 consumption cases', 'full-analysis.json', 'full'),
]:
    table(title, read(filename)['phases'][phase])
if (w / 'rotating-analysis.json').exists():
    table('Eight rotating inputs per fixture', read('rotating-analysis.json'))
if (w / 'small-rotating-recheck-analysis.json').exists():
    table('Independent small-record rotating recheck (11 repetitions)', read('small-rotating-recheck-analysis.json'))
table('Retained zero-spacing outputs', read('zero-retained-analysis.json'), True)
for filename, title in [('retained-analysis.json', 'Retained plain outputs')]:
    if (w / filename).exists():
        table(title, read(filename), True)
lines.extend(['## Build fingerprints', '', '| Artifact | R24 SHA-256 | R25 SHA-256 |', '|---|---|---|'])
for name in ['perry', 'libperry_runtime.a', 'libperry_stdlib.a']:
    lines.append(f"| {name} | `{meta['main_build']['files'][name]['sha256']}` | `{meta['candidate_build']['files'][name]['sha256']}` |")
lines.extend(['', 'The artifact index records the exact archived measurements, validation receipts, source patches and failed attempts. Frozen binaries are identified above; benchmark source paths and build/link commands are preserved in their provenance records.', ''])
assert len(lines) < 1900
target = bench / 'ZERO_SPACING_R25.md'
target.write_text('\n'.join(lines))
print('Wrote', target, 'with', len(lines), 'lines.')
