from pathlib import Path
import json

w = Path(__file__).resolve().parent
bench = w.parents[1]
read = lambda p: json.loads(p.read_text())
large = read(w / 'large-options-analysis.json')['phases']['options']
rotating = read(w / 'rotating-analysis.json')
full = read(w / 'full-analysis.json')['phases']['full']
groups = [('Large stringify options', large), ('Fresh and repeated inputs', rotating), ('Complete 50-row screen', full)]
cases = [c for _, phase in groups for c in phase['cases']]
assert len(cases) == 71
regressions = [c for c in cases if c['separated_regression']]
rss = [c['engines']['perry']['peak_rss_mib'] - c['engines']['baseline']['peak_rss_mib'] for c in cases]
repeat = [c for c in full['cases'] if c['operation'] in ['parse', 'stringify']]
assert len(repeat) == 38
lines = [
    '# Packed vector escaping: R42', '',
    'The packed writer closes the measured dense-escaping stringify gap. For the 1 MiB escaped fixture, pretty printing takes 336.781 µs versus Bun\'s 561.605 µs, and the replacer callback takes 363.656 µs versus Bun\'s 572.148 µs. Perry uses 40.03% and 36.44% less CPU than Bun on these two rows. Peak RSS falls by 3.500 and 3.563 MiB versus frozen R26.', '',
    f'This experimental source is not promoted: the full lineage still has {len(regressions)} rows with separated CPU regressions against R26 in this screen. The next investigation outlines object parsing from the recursive value dispatcher. No claim of merged-main performance or general parity follows from the escaping result.', '',
    'Three qualified quiet windows contain 1,988 timed trials, 380 complete-output verification records and 60 calibration trials. Each timed case has seven interleaved R26/Perry/Node/Bun repetitions. Node is pinned to 26.5.1 and Bun to 1.3.14. CPU is user plus system time per loop iteration; peak RSS includes the whole process and startup.', '',
    f'Across all 71 comparisons, median peak-RSS changes range from {min(rss)*1024:+.0f} to {max(rss)*1024:+.0f} KiB versus R26. Of the 38 repeated-source parse/stringify rows, {sum(c["beats_both_peers"] for c in repeat)} have CPU medians below both peers and {sum(c["all_samples_beat_both_peers"] for c in repeat)} have all measured CPU samples below both peers. Fresh-input measurements remain separate below.', '',
    'One earlier full-screen window failed the unchanged quiet-load gate at its end (2.5546875 one-minute load). Its 1,400 timed trials and 250 verification records are archived separately and excluded from every performance conclusion and qualified total above. The retry uses a distinct result directory and the same cases, repetitions and quiet criteria.', '',
    '## Implementation and validation', '',
    'For escaped blocks on ARM64, a 4 KiB byte-shuffle table packs eight input bytes and their short escapes in one operation. Plain blocks retain the existing sixteen-byte copy. Rare control bytes use the scalar six-byte encoder. Every speculative sixteen-byte output store requires at least sixteen input bytes remaining; the output length is published only after the exact bytes have been written. UTF-8 validation, expansion planning, native buffer growth, the 256-byte dispatch threshold and the fallback remain unchanged. The writer makes no managed allocation or callback, and this round changes no GC policy, roots, parse boundary, cache admission or cap.', '',
    (w / 'validation-verdict.txt').read_text().strip(), '',
    'Known native-root findings, lazy descriptor and fractional-spacing findings, and frozen-reference emitter output/moving-GC failures remain explicit. Their receipts are retained; this is not a blanket conformance pass. R42 has no alternate-worktree validation refusal. The earlier R40 refusal is documented in its own evidence.', '',
    '## Regressions in this screen', '',
    '| Fixture / operation | CPU change vs R26 | Slower pairs |',
    '|---|---:|---:|',
]
for c in regressions:
    lines.append(f'| {c["fixture"]} / {c.get("operation", c.get("mode"))} | {c["delta_pct"]:+.2f}% | {c["slower_pairs"]}/7 |')
lines += ['', '“Separated” means the smallest candidate sample exceeds the largest reference sample. It is a descriptive screen, not a significance test. Overlapping samples do not prove equivalence. These R42 rows have not received a second independent R42 recheck.', '']
for name, phase in groups:
    lines += ['## ' + name, '', f'Window: {phase["window"]["started_utc"]} to {phase["window"]["finished_utc"]}.', '',
              '| Fixture / operation | R26 µs | Perry µs | Node µs | Bun µs | Change vs R26 |',
              '|---|---:|---:|---:|---:|---:|']
    for c in phase['cases']:
        e = c['engines']
        lines.append('| ' + c['fixture'] + ' / ' + c.get('operation', c.get('mode')) + ' | ' + ' | '.join(f'{e[a]["cpu_us"]:.6f}' for a in ['baseline', 'perry', 'node', 'bun']) + f' | {c["delta_pct"]:+.2f}% |')
    lines += ['', '| Fixture / operation | R26 peak MiB | Perry peak MiB | Node peak MiB | Bun peak MiB |', '|---|---:|---:|---:|---:|']
    for c in phase['cases']:
        e = c['engines']
        lines.append('| ' + c['fixture'] + ' / ' + c.get('operation', c.get('mode')) + ' | ' + ' | '.join(f'{e[a]["peak_rss_mib"]:.3f}' for a in ['baseline', 'perry', 'node', 'bun']) + ' |')
lines += ['', '## Earlier scalar writer and profile follow-up', '',
    'The prior scalar writer in R39 measured 897.777 µs for escaped pretty printing and 924.234 µs for the replacer case. R42 reduces those medians by 62.49% and 60.65%. These are separately qualified windows, not paired R39/R42 trials. R39 is the earlier scalar implementation; the immediate source parent is the rejected R40 vector writer. The R39 control window and raw samples are in [R40 evidence](https://github.com/PerryTS/perry/blob/df1c4e18edc15ea37ec294a2b7c25af070001276/benchmarks/json_performance/NATIVE_VECTOR_ESCAPE_R40.md). They are not counted as new R42 trials.', '',
    'A follow-up analyzes four earlier R26/R39 profiles that independently exceed the unchanged 500-workload-sample floor: both wide-object stringify profiles and both 8 MiB record-object parse profiles. Every complete output matches a fresh pinned-Node execution of the same loop. The two wide-parse profiles remain excluded at 293 and 448 samples. The original six-case analysis refusal and failed profiling attempts are preserved. Sampled-process CPU/RSS is diagnostic, not benchmark evidence. The historical first-attempt envelope loss remains explicit in its recovery note; no byte-preservation claim is made for those lost envelopes.', '',
    'These profiles identify a large inlined object/value parser and time spent scanning keys for stream flags during wide-object stringify. They motivate new experiments but do not establish the cause of the small measured regressions.', '',
    'The successful full retry initially hit a local archive filename assertion. Its first subsequent remote operation recovered the complete quiet window; a local wrapper-argument comparison then needed correction before the verification receipt was written. Both controller failures, the successful archive transfer and final local verification are recorded explicitly. The hash-matched remote controller completed its checked benchmark command, and every raw output and sample passed the ordinary analyzer. No remote performance rerun or gate relaxation was used for that archive recovery.', '',
    '## Artifact provenance and limits', '',
    'Measured candidate source: `b9bfe3070f90f223e1d9d3526df54beb57308328`. Frozen R26 reference source: `3aac4d6335da54abeeed73df842decbbe6dd5d71`. Normal release flags and all three production packages were used. Compiler, runtime, stdlib, workers, fixture bytes, commands, complete verification outputs and CPU/RSS sample vectors are hash-checked and indexed. Every remote window was archived before further remote operations.', '',
    'No R42 Korean, small-options, changing-object, retained-output or access-specific performance window ran. The full screen includes its normal scan/roundtrip rows. Experimental harness files copied from earlier rounds are not evidence that their uninvoked phases ran. R32 integer-remainder access improvements are outside this source lineage and must be integrated and remeasured before landing.', '']
(bench / 'OUTLINED_OBJECT_PARSER_R42.md').write_text('\n'.join(lines))
print('Wrote', len(lines), 'report lines;', len(regressions), 'screen regressions')
