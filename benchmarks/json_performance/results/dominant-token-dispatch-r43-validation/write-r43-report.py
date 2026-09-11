from pathlib import Path
import json
w = Path(__file__).resolve().parent
bench = w.parents[1]
read = lambda p: json.loads(p.read_text())
focus = read(w/'regression-recheck-analysis.json')['phases']['focus']
rotating = read(w/'rotating-analysis.json')
korean = read(w/'korean-rotating-analysis.json')
phases = [('Targeted repeated-input screen', focus, 528, 60, 0), ('Fresh-input and repeated-input controls', rotating, 420, 100, 60), ('Korean input controls', korean, 168, 40, 24)]
allcases = [c for _, p, _, _, _ in phases for c in p['cases']]
regressions = [c for c in allcases if c['separated_regression']]
assert len(allcases) == 33
rss = [c['engines']['perry']['peak_rss_mib']-c['engines']['baseline']['peak_rss_mib'] for c in allcases]
lines = ['# Bounded dominant-token parser dispatch: R43', '',
    f'The large-token specialization now uses a bounded prefix eligibility check. Across the three qualified windows, {len(regressions)} of 33 comparisons retain separated CPU regressions against frozen R26. R43 is not promoted and does not establish the no-regression objective.', '',
    'The experiment includes 1,116 timed trials, 200 complete-output verification records and 84 calibration trials. The targeted screen uses eleven interleaved R26/Perry/Node/Bun repetitions per case; fresh-input and Korean controls use seven. Node is pinned to 26.5.1 and Bun to 1.3.14. CPU is user plus system time per loop iteration; peak RSS covers the complete process, including startup.', '',
    f'Median peak-RSS changes versus R26 range from {min(rss)*1024:+.0f} to {max(rss)*1024:+.0f} KiB. Raw per-repetition vectors and complete-output checks are included in the indexed evidence.', '',
    '## Implementation and validation', '',
    'Inputs up to 256 bytes retain the ordinary parser; 257–4,096 bytes retain the length-aware parser. Larger inputs use the length-aware specialization only if the first 512 bytes can contain the start of a dominant unescaped string token. Such a token must open within the first 256 bytes and remain open at byte 512. This necessary-condition check can admit false positives; both parsers still validate the entire document. The check uses the existing quote/backslash scanner, stays outlined for large inputs, and allocates no managed memory. The R42 outlined object-parser boundary remains in place.', '',
    'Tests extend the source-length boundaries through 4,095/4,096/4,097/4,098/8,192 bytes, sweep valid dominant-token prefix/suffix positions, compare full outputs for wide objects, short-record arrays and Unicode roots, and reject malformed trailing syntax through both dispatch paths.', '',
    (w/'validation-verdict.txt').read_text().strip(), '',
    'Known native-root findings, lazy-descriptor/getter/fractional-spacing findings and frozen-reference emitter failures remain explicit. Passing candidate checks do not erase those findings or establish full conformance.', '']
for label, phase, timed, verify, calibration in phases:
    window = phase['window']
    assert window['quiet_gate_passed'] and window['finished_utc']
    lines += ['## '+label, '', f"Window: {window['started_utc']} to {window['finished_utc']}. {timed} timed, {verify} verification and {calibration} calibration records.", '',
        '| Fixture / operation | R26 µs | R43 µs | Node µs | Bun µs | Change vs R26 | Screen |',
        '|---|---:|---:|---:|---:|---:|---|']
    for c in phase['cases']:
        e=c['engines'];op=c.get('operation', c.get('mode'))
        status='Regression' if c['separated_regression'] else 'Gain' if c['separated_improvement'] else 'Overlap'
        lines.append('| '+c['fixture']+' / '+op+' | '+' | '.join(f'{e[a]["cpu_us"]:.6f}' for a in ['baseline','perry','node','bun'])+f' | {c["delta_pct"]:+.2f}% | {status} |')
    lines += ['', '| Fixture / operation | R26 MiB | R43 MiB | Node MiB | Bun MiB |', '|---|---:|---:|---:|---:|']
    for c in phase['cases']:
        e=c['engines'];op=c.get('operation', c.get('mode'))
        lines.append('| '+c['fixture']+' / '+op+' | '+' | '.join(f'{e[a]["peak_rss_mib"]:.3f}' for a in ['baseline','perry','node','bun'])+' |')
    lines.append('')
lines += ['“Separated” means the smallest candidate sample exceeds the largest reference sample, or vice versa for an improvement. It is a descriptive screen, not a significance test; overlapping ranges do not prove equivalence. Fresh-input controls rotate eight separately allocated inputs with differing contents; they are separate workloads from repeatedly parsing one unchanged source.', '',
    '## Immediate parent observations', '',
    'R42 and R43 below are separate quiet windows, not interleaved parent/candidate pairs. Both use eleven repetitions and the same twelve declared cases. These historical R42 vectors add no R43 trial counts.', '',
    '| Fixture / operation | R42 µs | R43 µs | Difference |','|---|---:|---:|---:|']
parent=read(w/'historical-r42-reference/regression-recheck-analysis.json')['phases']['focus']
prior={(c['fixture'],c['operation']):c for c in parent['cases']}
for c in focus['cases']:
    a=prior[c['fixture'],c['operation']]['engines']['perry']['cpu_us'];b=c['engines']['perry']['cpu_us']
    lines.append(f'| {c["fixture"]} / {c["operation"]} | {a:.6f} | {b:.6f} | {(b/a-1)*100:+.2f}% |')
lines += ['', '## Provenance and limits', '',
    'Candidate source: `9415ce52ec36e72d643a70b0833240b407ea31d1`. Immediate parent: `b9bfe3070f90f223e1d9d3526df54beb57308328`. Frozen R26 reference: `3aac4d6335da54abeeed73df842decbbe6dd5d71`. Build artifacts, source patches, fixture hashes, command exits, raw samples and outputs are recorded. All 150 original remote staging hashes and the additional Korean assets are verified. Every terminal timing window is archived before subsequent remote operations.', '',
    'Initial controller launches occurred before local bootstrap finished and exited 2 without running Cargo or lint. A separate local staging attempt later refused a stale receipt filename before contacting the benchmark machine. It was corrected to read the actual successful canonical-validation receipt; the original failed scripts/logs/exit codes remain archived. These setup errors are not runtime/test failures. The deliberate post-unit production hold is also preserved separately from the successful test exit.', '',
    'No R43 full-50, stringify-options, retained-output or access-specific timing window ran. The earlier R41 packed-escaping options measurements and R32 integer-remainder access gains belong to their respective sources; R32 is outside this experimental lineage. Current main was not benchmarked. No CI waiting or administrative merge is involved.', '',
    'Next investigation: refine the outlined scanner continuation used for ordinary Korean UTF-8 after a valid ED prefix. Its standalone prototype is not a runtime performance result. Parser/stringifier regressions still require correction and integration before a ready PR can carry the combined experimental changes.', '']
(bench/'DOMINANT_TOKEN_DISPATCH_R43.md').write_text('\n'.join(lines))
print('Wrote R43 report:',len(lines),'lines;',len(regressions),'separated regressions')
