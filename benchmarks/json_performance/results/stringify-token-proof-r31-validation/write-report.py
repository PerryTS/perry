from pathlib import Path
import json
w=Path(__file__).resolve().parent;bench=w.parents[1]
read=lambda n:json.loads((w/n).read_text())
m=read('provenance.json')
lines=['# Root callback keys and maintain string escape proofs: R31','',(w/'measurement-verdict.md').read_text().strip(),'',
 '## Scope and reference','',
 'The object replacer walk keeps one rewritable key handle across getters, toJSON and replacer callbacks. It reloads key bytes only after callbacks and scopes replacer-pointer use at each call. The same slot is reused for each property. Pretty printing and replacer scalar output now use the existing heap-string provenance and inline-short-string emitters. Concatenation and in-place appends clear escape-free provenance when strings gain arbitrary bytes, while preserving lone-surrogate metadata. Escaped strings retain their existing writer. Borrowed non-ASCII JSON strings must also pass UTF-8 validation before receiving the escape-free flag; valid strings use the existing vector UTF-16 counter, while raw surrogate tokens use the existing normalizing builder. ASCII construction, GC policy, parse boundaries, cache admission and thresholds are unchanged.','',
 f"Measured source `{m['source_commit']}` versus frozen R26 `{m['main_build']['source_commit']}`, both workspace 0.5.1531. The harness calls the reference main/baseline; it is R26, not current main. Earlier implementation is represented by PRs #10050/#10052/#10064 with separate release metadata. This is not a merged-main measurement.",'',
 '## Validation','',(w/'validation-verdict.txt').read_text().strip(),'',
 'All 81 original reference behavior/options receipts are reused from the exact R26 build. All 81 original candidate executions are fresh. The new emitter regression adds 20 fresh executions per arm: the reference has fourteen output/callback failures, while the candidate must pass all 20. Fresh native/shadow checker runs cover the new fixture. The original 18 checker verdicts are reused only after exact emitted-IR and checker-source equivalence; existing findings remain unsuppressed.','',
 '## Measurement method','',
 'Quiet M1/8 GiB host, Node 26.5.1, Bun 1.3.14; fresh interleaved processes. CPU is user+system per loop iteration and peak RSS covers the entire process. Terminal windows are archived first. Repeated-input parsing includes existing caches and lazy construction; rotating inputs and consumption cases provide separate controls. Overlapping observed ranges do not establish equivalence.','']
def table(title, phase, retained=False):
    rows = phase['cases']
    window = phase['window']
    lines.extend(['### ' + title, '', f"Window: {window['started_utc']} to {window['finished_utc']}.", '',
        '| Fixture / operation | R26 µs | R31 µs | Node µs | Bun µs | CPU change | Ranges |',
        '|---|---:|---:|---:|---:|---:|---|'])
    for row in rows:
        e = row['engines']
        delta = row['cpu_delta_pct'] if retained else row['delta_pct']
        regression = row['cpu_separated_regression'] if retained else row['separated_regression']
        gain = max(e['perry']['cpu_samples_us']) < min(e['baseline']['cpu_samples_us'])
        label = row['fixture'] + ' / ' + row.get('operation', row.get('mode', '')) + (f" / {row['count']} live" if retained else '')
        lines.append('| ' + label + ' | ' + ' | '.join(f"{e[a]['cpu_us']:.6f}" for a in ['baseline', 'perry', 'node', 'bun']) + f" | {delta:+.2f}% | {'regression' if regression else 'gain' if gain else 'overlap'} |")
    lines.extend(['', '| Fixture / operation | R26 MiB | R31 MiB | Node MiB | Bun MiB | Peak RSS change MiB |',
        '|---|---:|---:|---:|---:|---:|'])
    for row in rows:
        e = row['engines']
        label = row['fixture'] + ' / ' + row.get('operation', row.get('mode', '')) + (f" / {row['count']} live" if retained else '')
        delta = e['perry']['peak_rss_mib'] - e['baseline']['peak_rss_mib']
        lines.append('| ' + label + ' | ' + ' | '.join(f"{e[a]['peak_rss_mib']:.3f}" for a in ['baseline', 'perry', 'node', 'bun']) + f" | {delta:+.3f} |")
    if retained:
        lines.extend(['', '| Fixture / operation | R26 after MiB | R31 after MiB | Node after MiB | Bun after MiB | After-RSS change MiB |', '|---|---:|---:|---:|---:|---:|'])
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
if (w/'small-rotating-recheck-analysis.json').exists():table('Independent small-record recheck (11 repetitions)',read('small-rotating-recheck-analysis.json'))
if (w/'retained-analysis.json').exists():table('Retained outputs',read('retained-analysis.json'),True)
lines.extend(['## Build fingerprints','','| Artifact | R26 SHA-256 | R31 SHA-256 |','|---|---|---|'])
for name in ['perry','libperry_runtime.a','libperry_stdlib.a']:
 lines.append(f"| {name} | `{m['main_build']['files'][name]['sha256']}` | `{m['candidate_build']['files'][name]['sha256']}` |")
lines.extend(['','The artifact index records archived raw windows, exact sample vectors, validation receipts, source patches and failed attempts. The small-options recheck remote measurement completed and was archived first, but its local controller then failed copying a misnamed local script. Exit1, the failure log, exact controller and reconstruction proof are preserved; the analyzer accepts only the archived remote completion proof and validates every sample. No remeasurement or fabricated exit0 was used. The candidate compiler and both static archives were built in a separate clean worktree; the copy receipt identifies the exact source and hashes. Prior independent profiles and future parser proposals are not R31 timing results.',''])
assert len(lines)<1900
(bench/'STRINGIFY_TOKEN_PROOF_R31.md').write_text('\n'.join(lines))
print('Wrote R31 report with',len(lines),'lines')
