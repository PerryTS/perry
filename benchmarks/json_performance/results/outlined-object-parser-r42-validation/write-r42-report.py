from pathlib import Path
import json

w = Path(__file__).resolve().parent
bench = w.parents[1]
read = lambda p: json.loads(p.read_text())
phase = read(w / 'regression-recheck-analysis.json')['phases']['focus']
parent = read(w / 'parent-screen-reference/full-analysis.json')['phases']['full']
prior = {(c['fixture'], c['operation']): c for c in parent['cases']}
cases = phase['cases']
assert len(cases) == 12
regressions = [c for c in cases if c['separated_regression']]
assert len(regressions) == 7
lines = [
    '# Outlined object parsing: R42', '',
    'Outlining the large object parser partially reduces the wide-object parse regression. R42 takes 2,977.0625 µs, 3.19% above its interleaved R26 reference; the previous R41 screen measured a 4.29% gap. R42 remains unpromoted: seven of the twelve targeted rows have separated CPU regressions versus R26.', '',
    'One qualified quiet window contains 528 timed trials and 60 complete-output verification records, with eleven interleaved R26/Perry/Node/Bun repetitions per case. The twelve cases cover all ten regressions from the qualified R41 screen plus two long-string stringify controls. Every case was declared before R42 timing. Three additional rows were added to the initial nine-case declaration after R41 identified them; iterations and warmup remain unchanged.', '',
    'All twelve candidate CPU medians are below both Node and Bun in this repeated-input screen. This does not establish general parity or fresh-input performance. Median peak-RSS changes versus R26 range from −32 to +96 KiB. CPU is user plus system time per loop iteration; peak RSS covers the complete process, including startup. Node is pinned to 26.5.1 and Bun to 1.3.14.', '',
    '## Change and validation', '',
    'The only implementation change is an inline(never) annotation and comment on parse_object_untyped. In the linked production worker, the SOURCE_LENGTH=true parse_value function shrinks from 1,760 to 576 instructions and the outlined object parser has 1,182 instructions. The false specialization retains its 117-instruction value dispatcher and 1,186-instruction object parser. These counts verify the intended call boundary; they are not a timing or causal proof. No parser algorithm, GC policy, roots, collection boundary, cache admission or cap changes.', '',
    (w / 'validation-verdict.txt').read_text().strip(), '',
    'Known native-root findings, lazy-descriptor and fractional-spacing findings, and frozen-reference emitter output/moving-GC failures remain unsuppressed and documented. The successful candidate checks do not erase those findings or constitute a blanket conformance pass.', '',
    '## Targeted CPU screen', '',
    f'Window: {phase["window"]["started_utc"]} to {phase["window"]["finished_utc"]}.', '',
    '| Fixture / operation | R26 µs | R42 µs | Node µs | Bun µs | Change vs R26 | Screen |',
    '|---|---:|---:|---:|---:|---:|---|',
]
for c in cases:
    e = c['engines']
    label = 'Regression' if c['separated_regression'] else 'Gain' if c['separated_improvement'] else 'Overlap'
    lines.append('| ' + c['fixture'] + ' / ' + c['operation'] + ' | ' + ' | '.join(f'{e[a]["cpu_us"]:.6f}' for a in ['baseline', 'perry', 'node', 'bun']) + f' | {c["delta_pct"]:+.2f}% | {label} |')
lines += ['', '“Separated” means the smallest candidate sample exceeds the largest reference sample. It is a descriptive screen, not a significance test; overlapping ranges do not prove equivalence. R42 has no second independent timing recheck.', '',
          '## Peak RSS', '', '| Fixture / operation | R26 MiB | R42 MiB | Node MiB | Bun MiB |', '|---|---:|---:|---:|---:|']
for c in cases:
    e = c['engines']
    lines.append('| ' + c['fixture'] + ' / ' + c['operation'] + ' | ' + ' | '.join(f'{e[a]["peak_rss_mib"]:.3f}' for a in ['baseline', 'perry', 'node', 'bun']) + ' |')
lines += ['', '## Immediate parent control', '',
    'These R41 and R42 observations come from separate quiet windows, not interleaved R41/R42 pairs. R41 used seven repetitions per case and R42 used eleven. The parent window is historical evidence from [R41](https://github.com/PerryTS/perry/blob/b5bb7ecd299e9c9513964dcc423e3c33e46fa4d4/benchmarks/json_performance/PACKED_VECTOR_ESCAPE_R41.md), not an additional R42 trial count.', '',
    '| Fixture / operation | R41 µs | R42 µs | Difference |', '|---|---:|---:|---:|']
for c in cases:
    a = prior[c['fixture'], c['operation']]['engines']['perry']['cpu_us']
    b = c['engines']['perry']['cpu_us']
    lines.append(f'| {c["fixture"]} / {c["operation"]} | {a:.6f} | {b:.6f} | {(b/a-1)*100:+.2f}% |')
lines += ['', '## Storage recovery and provenance', '',
    'The first remote stage failed with “No space left on device” before any R42 benchmark ran. Under the owned benchmark lock, 66 previously recorded identical R26 reference binaries were replaced by hard links after complete hash and ownership verification. Every path, executable byte and benchmark output was preserved. Available disk space rose from 126,468,096 to 1,387,667,456 bytes. The retry verified all 150 staged hashes before timing. The failed transfer, exact replacement plan, before/after storage observations and per-file hashes are archived.', '',
    'Candidate source: `b9bfe3070f90f223e1d9d3526df54beb57308328`. Immediate parent: `3952e832058a5e1efcd4cdf9cf28f26e31d60ea3`. Frozen R26 reference: `3aac4d6335da54abeeed73df842decbbe6dd5d71`. The index verifies every archived payload and Git blob; commands, immutable compiler/runtime/stdlib hashes, full verification records, complete CPU/RSS vectors and linked assembly are included. The timing window was archived before subsequent remote work.', '',
    'No R42 full-50, fresh-input, Korean, stringify-options, retained-output or access-specific timing window ran. The existing R41 packed-escaping result belongs to its own measured source; this narrow parser screen does not remeasure it. R32 integer-remainder access improvements remain outside this experimental lineage.', '',
    'A read-only integration check observed main at `435d6396c14a5e82ae8fb4cec2df9206aceb2a5d`: the zero-spacing PR #10052 landed through train #10082, and all three changed code/test files match its PR head. PRs #10064 and #10074 were open and ready. No CI waiting, administrative merge or current-main performance claim is involved.', '',
    'The next experiment uses a bounded prefix check to reserve the long-token specialization for potentially useful documents. R42 alone does not meet the no-regression objective.', '']
(bench / 'OUTLINED_OBJECT_PARSER_R42.md').write_text('\n'.join(lines))
print('Wrote R42 report with', len(lines), 'lines')
