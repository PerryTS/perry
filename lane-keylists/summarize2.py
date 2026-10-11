#!/usr/bin/env python3
"""Report keylists2 using only retained interleaved measurements."""
import argparse
import json
import statistics
from pathlib import Path

p = argparse.ArgumentParser()
p.add_argument('evidence', type=Path)
p.add_argument('output', type=Path)
p.add_argument('--head', required=True)
p.add_argument('--base', required=True)
p.add_argument('--ab', default='ab-final.json')
p.add_argument('--pause', default='pause-final.json')
p.add_argument('--baseline-note', default='untouched origin/main')
a = p.parse_args()
ab = json.loads((a.evidence / a.ab).read_text())
pause = json.loads((a.evidence / a.pause).read_text())
names = ['hello', 'tscwork', 'zodwork', 'qs_parse', 'qs_stringify', 'commander',
         'fastify', 'effect', 'property-prototype', 'shapes-options',
         'schema-record', 'root-survival', 'buffer_heavy', 'worker_heavy',
         'class_getter', 'object_array', 'class_virtual', 'object_spread']
micros = names[-4:]


def median(source, name, arm, key, iterations=None):
    rows = [r for r in source if r['name'] == name and r['arm'] == arm
            and r['iterations'] == iterations]
    assert len(rows) == 5, (name, arm, iterations, len(rows))
    assert all(r['huge_kb'] == 0 for r in rows)
    return statistics.median(r[key] for r in rows)


def pair(source, name, key, iterations=None):
    return [median(source, name, arm, key, iterations)
            for arm in ['obs-main', 'obs-head']]


def delta(v):
    return 100 * (v[1] / v[0] - 1) if v[0] else (0 if not v[1] else float('inf'))


def integers(v):
    return ' → '.join(f'{x:,.0f}' for x in v)


breaches = []
for n in names:
    it = 200000 if n in micros else None
    for label, source, key in [('instructions', ab, 'instructions'), ('GC pause', pause, 'gc_pause_us')]:
        d = delta(pair(source, n, key, it))
        if d > .5:
            breaches.append(f'{n} {label} {d:+.3f}%')
for n in micros:
    for label, source, key in [('instructions', ab, 'instructions'), ('GC pause', pause, 'gc_pause_us')]:
        d = delta(pair(source, n, key, 50000))
        if d > .5:
            breaches.append(f'{n} 50k {label} {d:+.3f}%')
    v = [(median(ab,n,arm,'instructions',200000) -
          median(ab,n,arm,'instructions',50000)) / 150000
         for arm in ['obs-main','obs-head']]
    if delta(v) > .5:
        breaches.append(f'{n} instructions/iteration {delta(v):+.3f}%')
out = [f'Keylists2, 2026-10-11. Baseline: {a.baseline_note} '
       f'`{a.base}`; candidate `{a.head}`. '
       + ('**Not landable:** ' + '; '.join(breaches) + '.' if breaches else
          'All measured instruction and GC-pause deltas satisfy the requested +0.5% limit.'),
       'Architecture: removes the canonical/class relocation scanners and minor death walks; '
       'keeps the O5-proven transition and shape caches. Canonical keys and their pointer-free '
       'attributes use existing old storage and the shared-layout movement exclusion. Full '
       'sweeps still reclaim them. Owned mutable lists and prototype words retain relocation '
       'custody; the existing writer log now survives full scans without rebuilding it over '
       'every old family. No new table, cache, latch, runtime option or per-site code was added.',
       'Why the four programs regressed: old keys initially retained young attributes and '
       'young key strings, expanding copying/remembered work. Full collections also rescanned '
       'dirty pages after already recording all live old-parent edges during marking. '
       'Attributes now share stationary old storage; nursery key strings request promotion '
       'on their first copy using the existing tenured flag. Every barrier remains until '
       'the atom lane supplies its own immortality proof. Synchronous fulls reuse marked '
       'coverage; sliced fulls and minors retain their repair walks. Marking tests distinguish '
       'edge liveness from remembered coverage, including observer rewrites and multiple pages.',
       'The initial private diagnostic attribution below is instructions in millions, '
       'candidate minus its matched baseline, before those fixes. It is diagnostic evidence '
       '(baseline 15b6149e5be4 plus the same rooted-value build correction), not the acceptance '
       'A/B. Allocation includes any nested collection and must not be summed with GC phases.',
       '| Program | Census | Roots | Mark | Sweep | Dirty repair | Copying minor | Key allocation |',
       '|---|---:|---:|---:|---:|---:|---:|---:|',
       '| Effect | +72.670 | +3.239 | +1.300 | +40.924 | ~0 | +77.506 | +1.466 |',
       '| schema-record | +17.049 | +3.472 | +2.237 | +27.670 | ~0 | +180.422 | +1.452 |',
       '| buffer | +4.219 | −1.320 | +15.379 | +12.612 | +24.417 | −1.609 | +0.286 |',
       '| worker | −1.623 | +4.562 | +18.046 | −1.214 | +29.117 | 0 | +1.132 |',
       '',
       'Matched initial allocator-call census (logical payload bytes; includes owned lists):',
       '| Program | Allocator calls main → head | Payload bytes main → head |',
       '|---|---:|---:|',
       '| Effect | 13,248 → 13,489 | 5,430,920 → 5,429,016 |',
       '| schema-record | 13,282 → 13,509 | 5,568,776 → 5,499,512 |',
       '| buffer | 2,328 → 2,328 | 277,424 → 277,424 |',
       '| worker | 9,322 → 9,322 | 1,160,592 → 1,160,592 |',
       '',
       'The allocation volume does not explain the excess. After making attributes old, '
       'Effect/schema remembered-slot visits were 455,668/474,140 versus main 7,126/10,331. '
       'Their first-copy promotion fix removes repeated survival work. Buffer/worker dirty '
       'repair visited 116,171/139,469 slots while adding zero pages beyond marked coverage. '
       'The final mark loop specializes the existing old-parent proof at object entry. '
       'Malloc parents share the ordinary loop plus one cold post-observer coverage pass, '
       'rather than a third complete loop copy. Synchronous full sweeps reuse the already '
       'completed element-shape death prune, preserving header/epoch/numeric invalidation. '
       'Copying root producers retain their decoded RootWord and publish diagnostic TLS '
       'provenance once per walk, removing repeated per-slot work. Fastify runs 29 to 42 '
       'minors after old key birth leaves its nursery cap at 4 MiB; nursery pacing remains '
       'unchanged. All root alignment, header, handle and tag checks remain. '
       'String words cannot invoke foreign handle observers, so they '
       'retain the shared decode even during an active handle trace. Child descriptors '
       'still honor that active trace, and foreign-word callbacks retain the post-mark reread.',
       '',
       'n=5, interleaved, qb6 CPUs 0–23,32–55, ASLR off, diagnostics off. THP is disabled '
       'and verified in every row. Separate sources/targets per arm. Fresh current-main '
       'frontend objects are identical between arms (no compiler/codegen diff); pure shipping '
       'twins supply `.text`. Emitted operation bytes per site: **0-byte change**. The private '
       'observer performs two atomic updates per collection, counting every worker collector '
       'and synchronous full; it is absent from shipping source.',
       '| Program | Instructions main → head | Δ | RSS KiB main → head | Full/minor main → head | `.text` bytes main → head |',
       '|---|---:|---:|---:|---:|---:|']
for n in names:
    it = 200000 if n in micros else None
    ins = pair(ab, n, 'instructions', it)
    counts = [f"{median(ab,n,arm,'full',it):g}/{median(ab,n,arm,'minor',it):g}"
              for arm in ['obs-main', 'obs-head']]
    out.append(f'| {n} | {integers(ins)} | {delta(ins):+.3f}% | '
               f"{integers(pair(ab,n,'rss_kb',it))} | {' → '.join(counts)} | "
               f"{integers(pair(ab,n,'text',it))} |")
out += ['', 'Class micro metric: (200k instructions − 50k instructions) / 150k iterations.',
        '| Micro | Main instructions/iteration | Head | Δ |', '|---|---:|---:|---:|']
for n in micros:
    v = [(median(ab, n, arm, 'instructions', 200000) -
          median(ab, n, arm, 'instructions', 50000)) / 150000
         for arm in ['obs-main', 'obs-head']]
    out.append(f'| {n} | {v[0]:.4f} | {v[1]:.4f} | {delta(v):+.5f}% |')
busy = max(v for r in pause for v in r['sibling_busy_pct'].values())
out += ['', f'GC pauses: n=5 under MEASURE.lock, CPUs 56–63, ASLR off. '
        f'Maximum observed sibling utilization (CPUs 24–31): {busy:.2f}%. '
        'Worker pauses sum active collector durations, rather than process-wide stop time.',
        '| Program | Main GC ms | Head GC ms | Δ ms | Δ % |', '|---|---:|---:|---:|---:|']
for n in names:
    it = 200000 if n in micros else None
    v = [x / 1000 for x in pair(pause, n, 'gc_pause_us', it)]
    out.append(f'| {n} | {v[0]:.3f} | {v[1]:.3f} | {v[1]-v[0]:+.3f} | {delta(v):+.3f}% |')
out += ['', 'Buffer/worker pass current main and are included in both output and performance gates; '
        'the previously reported #12091/#12092 failures do not reproduce. O5 probes remain '
        'the original evidence: transition removal costs Zod +88.2%, qs_parse +45.3%, '
        'object_spread +319.8%; shape-cache removal costs Zod +4.3%.',
        'Full release unit suite counts and matched failures are retained in the raw logs. '
        'Both frozen-baseline and candidate suites reproduce seven shared failures: date-expando numeric '
        'representation check, two descriptor-padding checks, perex exact length, dense '
        'constructor captures, stored undefined, and typed-feedback array-literal latch. '
        'Added stationary attributes, full death, first-copy promotion and full-mark '
        'coverage/arming tests pass, with active-observer positive and negative controls. '
        'The shape writer-log test detects suppression during a full scan.',
        'Correctness, native-corpus populations, negative controls and root-audit '
        'results are recorded separately for this exact source cohort. No older '
        'cohort\'s counts are inferred from these performance rows. Native C '
        'definitions and bodies join the root checker\'s symbol/call population; '
        'its controls reject prototypes/prose and check platform bodies and transitive calls.',
        'Atom/intern files are unchanged. Coordination notes document the first-copy '
        'promotion request and retained barriers; the parallel atom lane is not assumed '
        'complete. No version fields changed, no push or publishing occurred.',
        'Raw rows, logs, stress results and baseline controls: '
        '`scratchpad/codex-small/keylists2-evidence/`. Bundle: '
        '`/Users/amlug/projects/perry/secret-tests/scratchpad/codex-small/keylists2.bundle`.']
a.output.write_text('\n\n'.join(out[:4]) + '\n\n' + '\n'.join(out[4:]) + '\n')
