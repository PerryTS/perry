from pathlib import Path
import json

w = Path(__file__).resolve().parent
bench = w.parents[1]
parent = w.with_name('compact-template-capture-r39')
read = lambda p: json.loads(p.read_text())
child = read(w/'large-options-analysis.json')['phases']['options']
prior = read(parent/'large-options-analysis.json')['phases']['options']
comparison = read(w/'immediate-parent-comparison.json')
lines = [
    '# Native vector escaping: R41', '',
    'R41 is not promoted. Dense escaped pretty printing takes 1,775.855 µs versus 897.777 µs in its immediate parent (+97.81%); the replacer case takes 1,793.242 µs versus 924.234 µs (+94.02%). R41 remains about 3.16×/3.14× Bun on those cases. The ASCII and Unicode controls remain much faster than Node and Bun. Peak RSS changes versus the parent are between −16 and +16 KiB.', '',
    'These are two separately qualified quiet windows, each with seven interleaved R26/Perry/Node/Bun repetitions per case. R39 and R41 were not interleaved against each other. Each window has 168 timed trials and 30 complete-output verification records: 336 timed trials and 60 verifications in total. The six parent measurements are new reference observations for this investigation, additional to the earlier R39 evidence commit.', '',
    '## Implementation and validation', '',
    'The candidate adds a bounded ARM vector writer to the existing native stringify buffer path. UTF-8 validation, expansion planning, capacity growth, short-string dispatch and the WTF-8 fallback stay in place. It classifies sixteen source bytes, then emits one escape before continuing. Repeated classification of overlapping blocks is a candidate explanation for the slowdown, not a qualified sampling attribution. No GC policy, threshold, parse boundary, cache admission or managed intermediate allocation changes.', '',
    (w/'validation-verdict.txt').read_text().strip(), '',
    'The original native finding set, the primitive-key finding, and the large-emitter non-moving string-handle/overflow-store finding remain unsuppressed. Shadow checks and ordinary/callback native checks pass. R26 reference emitter output and moving-GC failures remain explicit; candidate output checks pass. No full, rotating, Korean, changing-object, retained-output or access performance window ran for R41. The seven R39 full-screen regressions are inherited concerns and are not remeasured here.', '',
    '## Immediate parent comparison', '',
    'CPU is user plus system time per loop iteration. Peak RSS covers the complete process, including startup. The reference is frozen R26, not current main. Source commits: R26 `3aac4d6335da54abeeed73df842decbbe6dd5d71`; parent R39 `e69d1292110cf9abee24f8690c584652805a4167`; candidate R41 `3952e832058a5e1efcd4cdf9cf28f26e31d60ea3`.', '',
    '| Fixture / operation | R39 µs | R41 µs | Change | R39 peak MiB | R41 peak MiB |',
    '|---|---:|---:|---:|---:|---:|',
]
for r in comparison['cases']:
    lines.append(f"| {r['fixture']} / {r['operation']} | {r['r39_cpu_us']:.6f} | {r['r40_cpu_us']:.6f} | {r['cpu_delta_pct']:+.2f}% | {r['r39_peak_rss_mib']:.3f} | {r['r40_peak_rss_mib']:.3f} |")
for name, phase in [('R41 candidate', child), ('R39 immediate parent', prior)]:
    lines.extend(['', '## '+name, '', f"Window: {phase['window']['started_utc']} to {phase['window']['finished_utc']}.", '',
        '| Fixture / operation | R26 µs | Perry µs | Node µs | Bun µs |', '|---|---:|---:|---:|---:|'])
    for r in phase['cases']:
        e = r['engines']
        lines.append('| '+r['fixture']+' / '+r['operation']+' | '+' | '.join(f"{e[a]['cpu_us']:.6f}" for a in ['baseline','perry','node','bun'])+' |')
    lines.extend(['', '| Fixture / operation | R26 peak MiB | Perry peak MiB | Node peak MiB | Bun peak MiB |', '|---|---:|---:|---:|---:|'])
    for r in phase['cases']:
        e = r['engines']
        lines.append('| '+r['fixture']+' / '+r['operation']+' | '+' | '.join(f"{e[a]['peak_rss_mib']:.3f}" for a in ['baseline','perry','node','bun'])+' |')
lines.extend(['', '## Artifact provenance', '',
    'The index includes both benchmark windows with their distinct measured source commits, original commands, verification records, complete CPU/RSS vectors and frozen artifact hashes. The supplementary alternate-worktree refusal is separate from the successful canonical run. Standalone correctness includes 509 full outputs matching the pinned Node corpus and is not timing evidence. Every benchmark window was archived before further remote work. This branch is experimental evidence, not a ready PR or a merged-main result.', ''])
(bench/'PACKED_VECTOR_ESCAPE_R41.md').write_text('\n'.join(lines))
print('Wrote R41 report with', len(lines), 'lines')
