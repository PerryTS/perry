from pathlib import Path
import hashlib
import json
import subprocess

w = Path(__file__).resolve().parent
bench = w.parents[1]
root = w.parents[3]
read = lambda name: json.loads((w / name).read_text())
head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
assert head == read('build-provenance.json')['source_commit'] == read('unit-source.json')['source_commit']
assert not subprocess.check_output(['git', 'diff', '--name-only'], cwd=root)
assert read('unit-source.json')['exit_code'] == 0
assert read('root-comparison.json')['checks_match']
screen = read('screen-analysis.json')['phases']['focus']
recheck = read('recheck-analysis.json')['phases']['focus']
assert screen['timed_trials'] == 504 and recheck['timed_trials'] == 44
control = recheck['cases'][0]
assert control['slower_pairs'] == 10 and not control['separated_regression']
for arm in ['main', 'candidate']:
    for kind, count in [('fixture', 37), ('options', 14)]:
        rows = read(arm + '-' + kind + '-validation.json')
        assert len(rows) == count and all(r['matches_node'] for r in rows)
    for record in read(arm + '-workers-provenance.json'):
        for path, digest in record['files'].items():
            assert hashlib.sha256((root / path).read_bytes()).hexdigest() == digest

cpu_rows, rss_rows = [], []
for r in screen['cases']:
    e = r['engines']
    label = r['fixture'] + ' / ' + r['operation']
    cpu_rows.append('| ' + label + ' | ' +
                    ' | '.join(f"{e[k]['cpu_us']:.3f}" for k in ('baseline', 'perry', 'node', 'bun')) +
                    f" | {r['delta_pct']:+.2f}% | {r['slower_pairs']}/7 | " +
                    ('Slower, separated' if r['separated_regression'] else 'Faster, separated' if r['separated_improvement'] else 'Overlap') + ' |')
    rss_rows.append('| ' + label + ' | ' +
                    ' | '.join(f"{e[k]['peak_rss_mib']:.3f}" for k in ('baseline', 'perry', 'node', 'bun')) +
                    f" | {r['rss_delta_mib']:+.3f} |")

v = 'results/cached-construction-r20-validation/'
verdict = ('Parked without a runtime PR: scan CPU gains are 0.34% at 1 MiB and 0.88% at 8 MiB; '
           'the recurring object-parse control is 0.74% slower in the longer recheck, with 10/11 slower pairs and overlapping ranges. '
           'The initial screen also has a separated 16 KiB sparse-access slowdown. No broader performance qualification.')
(w / 'validation-verdict.txt').write_text(verdict + '\n')
e = control['engines']
report = f'''# Cached-subtree construction (R20)

**Parked; no runtime PR or release bump.** Skipping already-cached subtrees produces only small full-scan CPU gains: 0.34% at 1 MiB and 0.88% at 8 MiB. Peak RSS falls 2.81 MiB and 6.09 MiB respectively. The recurring 1 MiB object-parse control is 0.59% slower in the screen with separated samples, then 0.74% slower in the longer recheck with 10/11 slower pairs. The recheck ranges overlap: this is a persistent adverse trend, **not** a second separated result. The small gains do not justify advancing this candidate under the no-regression requirement.

Measured source `{head}`, based directly on main `1a9c0de6cb790d2467b0ca22a660870025179b37` (0.5.1531). Earlier accepted JSON work merged through [#10037](https://github.com/PerryTS/perry/pull/10037). [R19's independent rebuild](https://github.com/PerryTS/perry/blob/87f571d734eddca2bf997561c17adb072f5132c1/benchmarks/json_performance/MAIN_REBUILD_R19.md) produced byte-identical compiler/runtime/stdlib artifacts and rules out rebuild drift in that environment; it does not identify the cause of these control changes.

## Change and scope

Within the existing minority-cache full-materialization admission, a single exact-capacity array receives cached values directly. The validated tape supplies source offsets and subtree ends, while the ordinary DirectParser builds missing values. Cached aliases and mutations survive without constructing replacements that would immediately be overwritten. The existing rooted publication check and cache patch remain.

Parse routing, sparse-read dispatch, the 16 MiB limit, scan thresholds and GC policy are unchanged. Construction uses the existing suppression window and array builder, including aggregate layout and old-to-young edge tracking. This is an independent candidate on main, not a combination with rejected R18. The 120-record/16 KiB sequential scan still cannot enter this full-materialization producer under its existing threshold.

The source-level skip is demonstrated by a unit test that observes no interned key from the skipped subtree, in addition to value and identity checks. Linked-symbol evidence alone is not a sampled-entry or phase-time measurement. No R20 profile was collected.

## CPU: all 18 screen rows

CPU is user plus system time per operation, in microseconds; values are medians of seven fresh-process trials. Negative delta means less candidate CPU than main. Large cases use the predeclared doubled original iteration count; tiny/small counts and warmups are unchanged. “Separated” compares the complete sample ranges in this window.

| Fixture / operation | Main µs | Candidate µs | Node µs | Bun µs | CPU delta | Slower pairs | Samples |
|---|---:|---:|---:|---:|---:|---:|---|
{chr(10).join(cpu_rows)}

## Peak RSS: the same screen

MiB, median process peak RSS. This includes the whole worker workload and is not a measurement of live output bytes or allocator capacity.

| Fixture / operation | Main MiB | Candidate MiB | Node MiB | Bun MiB | Candidate delta |
|---|---:|---:|---:|---:|---:|
{chr(10).join(rss_rows)}

[Screen analysis]({v}screen-analysis.json), [declarations]({v}screen-cases.json), [raw screen window](results/quiet-cached-construction-r20-screen-focus/window.json).

## Longer recurring-control check

The predeclared 1 MiB object-parse check uses 324 iterations, two warmups and eleven interleaved fresh-process repetitions per engine. Candidate CPU is **{e['perry']['cpu_us']:.3f} µs**, main **{e['baseline']['cpu_us']:.3f} µs**, Node **{e['node']['cpu_us']:.3f} µs**, Bun **{e['bun']['cpu_us']:.3f} µs**. Candidate versus main is **+{control['delta_pct']:.4f}%**, with peak RSS **+{control['rss_delta_mib']:.5f} MiB**.

Ten paired CPU deltas are positive (about +0.55% to +1.03%); one is -0.054%. Full ranges overlap. The separate 16 KiB sparse-access slowdown is a screen finding only; it received no longer recheck. Neither observation is relabelled as a clean control.

[Recheck analysis]({v}recheck-analysis.json), [predeclared control]({v}recheck-cases.json), [raw recheck window](results/quiet-cached-construction-r20-recheck-focus/window.json).

## Validation and provenance

- All **294 JSON Rust tests** pass on the clean committed source. Three initial focused tests also pass, covering skipped decoding, bitmap word boundaries and pointer layout, and decline behavior.
- Both frozen arms pass **37 Node behavior runs** and **14 stringify-option checks**. The expanded fixture includes mixed/duplicate/escaped/Unicode/nested records, cached aliases and mutations, and four 2,600-element arrays that force materialization and subsequent collections. Its scheduled auto/tape runs have 854 protected retired sets and 159,027 moved objects per arm; direct runs have 1,118/155,168. These are actual moving/protected runs.
- All sixteen IR files match main after removing only the first native ModuleID path comment; shadow IR needs no normalization. Native analysis retains eight equal, unsuppressed findings: five unrooted globals, two string-handle warnings and one stale allocation value. Coverage: 2,775 safepoints, 2,097 live bundles, 10,213 relocates and 10,123 pairs. Shadow checks and ordinary-worker/callback native subsets pass. This is not an all-clean native result.
- All four worker object files are byte-identical at the common R20 source paths. Compiler and both archives are frozen from the exact production package set, with mtimes after build start and verified hashes. The build started at 23:24:55 UTC on 2026-09-10 and took 331.95 seconds. Main uses R19's independently rebuilt, byte-identical reference.
- Script lint passes 73/74 checks, with the existing public-benchmark freshness failure; the file cap passes. Compile tier and two CI-only checks were skipped; full CI is not claimed.
- All 24 lazy probes retain main's full stdout and exit outcomes, including six large zero/true-spacing SIGSEGV cases and two noncanonical raw outputs. The inherited fractional-spacing reference has matching compiler/runtime/fixture hashes, and the candidate preserves its difference from Node. These gaps remain gaps.

The first baseline setup failed because an unused optional fixture copy stopped the filename update, leaving a stale R18 subject name. Its script and the first eighteen passing baseline runs are preserved. After correcting and preflighting the fixture paths, the full baseline suite passed. Formatting also overlapped the start of the preliminary focused compile; the initial hash mismatch is recorded. The authoritative full JSON suite ran later on the final clean commit.

[Validation summary]({v}validation-summary.json), [units]({v}unit-source.json), [build]({v}build-provenance.json), [source freshness]({v}source.json), [static comparison]({v}root-comparison.json), [construction review]({v}construction-safety-review.md), [setup failure]({v}initial-baseline-setup/failure.json), [formatting record]({v}format-source-equivalence.json), [fractional reference reuse]({v}fraction-reference-reuse.json).

## Preserved windows and remaining work

Both windows passed the predeclared quiet gate, and each terminal window was archived as the first subsequent remote operation. The screen ran 23:35:17–23:39:14 UTC, load 1.949 → 2.413. The recheck ran 23:40:01–23:40:33 UTC, load 1.724 → 1.953.

Total: **548 timed trials and 95 full-output verification trials**. Checksums, full-output hashes, declared iteration counts, every CPU/RSS sample vector and median, source patches, tool versions and 103 staged input hashes were checked. No full-50, access, rotating, retained-output, short-call or options performance qualification ran. Prepared drivers are not execution evidence. Parsing and stringify remain in the full objective.

Skipping a small cached fraction is insufficient here. The next investigation should quantify repeated value-string allocations and audit whether sharing such immutable values inside one construction window can safely remove more work. Its effect on stringify and retained outputs must be measured, and the recurring object-parse control remains mandatory. Code-placement effects remain a hypothesis, not a proven explanation for that control.

[Validation artifact manifest]({v}manifest.json).
'''
(bench / 'CACHED_CONSTRUCTION_R20.md').write_text(report)
print('Wrote R20 parked-candidate report with all 18 CPU/RSS rows and the overlapping 10/11 control recheck.')
