from pathlib import Path
import json, subprocess

w = Path(__file__).resolve().parent
bench = w.parents[1]
read = lambda name: json.loads((w / name).read_text())
source = read('build-provenance.json')['source_commit']
assert source == subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=w.parents[3], text=True).strip()
screen = read('screen-analysis.json')['phases']['focus']
recheck = read('recheck-analysis.json')['phases']['focus']
full = read('full-analysis.json')['phases']['full']
access = read('access-analysis.json')['phases']['access']
controls = read('controls-analysis.json')['phases']['focus']
small = next(r for r in controls['cases'] if r['fixture'] == 'small_record')
assert small['slower_pairs'] == 10 and not small['separated_regression']
assert sum(p['timed_trials'] for p in [screen, recheck, full, access, controls]) == 2416
assert not any(r['separated_regression'] for p in [screen, recheck, full, access, controls] for r in p['cases'])
v = 'results/warm-key-uniqueness-r21-validation/'
verdict = ('Parked without a runtime PR: large object parsing improves about1.7–2.1% and array scans about1.2–1.6%, '
           'but small-record stringify has a persistent adverse CPU trend across three quiet windows. '
           'Its longer recheck is +0.4566%,10/11slower pairs with overlapping ranges. '
           'No regression-free acceptance or remaining performance qualification is claimed.')
(w / 'validation-verdict.txt').write_text(verdict + '\n')
tables = w / 'full-tables.md'
tables.write_text(tables.read_text().replace('R21 remains under investigation.', 'R21 is parked because small-record stringify remains slightly slower across three windows; no acceptance or complete qualification is claimed.'))

window_rows = []
for label, phase, slug, verify in [
    ('18-row screen', screen, 'screen-focus', 90),
    ('Object-parse recheck', recheck, 'recheck-focus', 5),
    ('38 + 12 full matrix', full, 'full', 250),
    ('12 access rows', access, 'access', 12),
    ('Three longer controls', controls, 'controls-focus', 15),
]:
    win = phase['window']
    window_rows.append(f"| [{label}](results/quiet-warm-key-uniqueness-r21-{slug}/window.json) | {win['started_utc']} | {win['finished_utc']} | {win['load_before'][0]:.3f} → {win['load_after'][0]:.3f} | {phase['timed_trials']} | {verify} |")

control_rows = []
for r in controls['cases']:
    e = r['engines']
    control_rows.append('| ' + r['fixture'] + ' / ' + r['operation'] + ' | ' +
                        ' | '.join(f"{e[k]['cpu_us']:.5f}" for k in ['baseline', 'perry', 'node', 'bun']) +
                        f" | {r['delta_pct']:+.3f}% | {r['slower_pairs']}/11 | Overlap |")

report = f'''# Warm key uniqueness (R21)

**Parked; no runtime PR or release bump.** Removing redundant duplicate-key searches improves large object parsing by roughly 1.7–2.1% and array scans by 1.2–1.6%. The mandatory longer 1 MiB object-parse check confirms a 1.6645% reduction, with all eleven pairs faster and separated sample ranges. Small-record stringify, however, remains slightly slower across three independent quiet windows: +0.54%, +0.55%, and +0.4566%. The last has ten of eleven pairs slower and overlapping ranges. This persistent adverse trend does not meet the no-regression acceptance requirement; it is **not** described as a separated slowdown.

Measured source `{source}`, directly based on main `1a9c0de6cb790d2467b0ca22a660870025179b37` (0.5.1531). Earlier accepted JSON work already merged through [#10037](https://github.com/PerryTS/perry/pull/10037). This experiment contains neither R18's tape producer nor R20's cached-subtree producer.

## All rows and peer comparison

[All 38 original parse/stringify CPU and RSS rows, followed by all 12 consumption rows]({v}full-tables.md). [Machine-readable full results]({v}full-analysis.json), [screen]({v}screen-analysis.json), [access]({v}access-analysis.json).

Both **main and candidate** have lower median CPU than Node 26.5.1 and Bun 1.3.14 on all 38 original rows in this repeated-input window. That was already true of the reference; it is not a new 38-row victory created by this patch. Cached inputs and deferred materialization make consumption and rotating-input tests essential. In the full matrix, the candidate's 16 KiB array scan is **1.62× Bun**, the 1 MiB scan **1.30×**, and the 8 MiB scan **1.05×**. The earlier screen uses more iterations and its 16 KiB ratio is 1.94×; do not mix the two windows. The full objective remains open.

Every full-matrix comparison either improves with separated samples or has overlapping ranges. The 1 MiB sequential-access control also improves by 1.41%. The full matrix's peak-RSS median deltas range from -0.016 to +0.484 MiB; the screen ranges from 0 to +0.063 MiB. These are whole-process peak measurements, not a proof of neutral live heap or retained-output memory. No new R21 retained-output performance run was performed.

## Longer controls

The full matrix suggested a +3.51% Unicode-stringify median shift, +0.55% small-record stringify, and +0.34% 16 KiB sparse access. All had overlapping ranges. Follow-ups were declared before their measurement: Unicode stringify 19,800 iterations, small-record stringify 10,000,000, and sparse access 31,472; eleven interleaved repetitions per engine and unchanged warmups.

CPU is user plus system microseconds per operation; negative delta means less candidate CPU.

| Fixture / operation | Main µs | Candidate µs | Node µs | Bun µs | Delta | Slower pairs | Ranges |
|---|---:|---:|---:|---:|---:|---:|---|
{chr(10).join(control_rows)}

The Unicode and sparse median shifts reverse in the longer run; neither is established as a regression. Small-record stringify retains its adverse trend. The full matrix also has a +0.61% long-string parse shift with overlapping samples and a very short candidate timed interval; a proposed longer A/B-only check was not executed after parking. No unresolved observation is relabelled a clean pass.

[Control declarations]({v}controls-cases.json), [control analysis]({v}controls-analysis.json), [object-parse recheck]({v}recheck-analysis.json).

## Runtime change and construction isolation

Completed direct-parser shapes contain ordered, deduplicated key sequences. While a subsequent object's captured warm-shape prefix continues matching, the next key cannot duplicate an earlier prefix key. The patch skips that redundant search. Once the shape mismatches or ends, the original pointer/content duplicate search remains. Nested objects may update the parser's hot shape without changing the parent's captured snapshot.

There is no new cache, allocation, root holder, GC boundary, threshold, parse routing, or collection-policy change. All key/value writes and fallback behavior remain intact. The new tests cover every prefix length, escaped duplicates, extra fields, nested shape changes, insertion order, later consumption, and retained values under moving GC. [Invariant review]({v}invariant-review.md).

The initial proposed repeated-string cache was discarded before implementation. Across all nineteen fixtures there are zero repeated heap-eligible value strings within one parse. Five-byte tags already use inline strings in both direct and tape materialization. The 1 MiB record array has 30,400 value strings: 15,200 inline and 15,200 unique heap-eligible values. This is a decoded-source census, not allocation instrumentation. Existing across-parse reuse can reduce allocation further. [Census]({v}value-string-census.json).

An existing R18 main profile was reanalyzed, not remeasured: 752 main-thread samples, with 24.20% exclusive samples in tape construction and 16.09% in object parsing. These are sampled locations, not exact phase timings or an attribution of all gains to removed comparisons. Aggregated program-counter lists cannot assign a weight to each listed instruction. No R21 profile was collected. [Profile provenance]({v}reference-profile-provenance.json), [exclusive reanalysis]({v}main-profile-exclusive.json).

## Validation and build evidence

- All **293 JSON Rust unit tests** pass on the clean committed source, including both new tests.
- Both frozen builds pass **46 Node behavior runs and 14 stringify-option checks**. The new fixture's scheduled runs retire 402 protected page sets in each parser mode and move 39,798 objects in auto, 46,606 in tape, and 38,289 in direct mode, per arm. These exercise actual moving/protected GC.
- All **18 IR files per arm** match after removing only the first native ModuleID path comment; shadow IR needs no normalization. Native analysis keeps the same eight unsuppressed findings: five unrooted globals, two string-handle findings, one stale allocation value. Coverage is 2,994 safepoints, 2,259 live bundles, 10,737 relocates, and 10,619 pairs. Shadow checks and ordinary-worker/callback native subsets pass. This is not an all-clean native result.
- All four ordinary/access/rotating/options worker object files are byte-identical between arms. The exact production build command is `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static`. It started 2026-09-11 at 00:14:24.867821 UTC and took 306.77 seconds. Compiler and both archives are frozen with matching SHA-256 hashes and mtimes after build start. Wrapper/compiler source mtimes were refreshed without changing bytes or flags. Main uses R19's independently rebuilt, byte-identical original reference.
- Script lint passes 73/74 checks, retaining the existing public-benchmark freshness failure. File-size checks pass. The compile tier and two CI-only checks were skipped; full CI is not claimed.
- All 24 lazy probes retain main's stdout and exit outcomes, including six large zero/true-spacing SIGSEGV cases and two noncanonical raw outputs. Fractional-spacing output preserves the separately recorded main/Bun versus Node difference; its main reference was reused after compiler/runtime/fixture hash verification. These remain correctness gaps.

Setup metadata initially referenced the obsolete `value.rs` path and was corrected to the split value modules. A later preparation assertion treated the case-declaration dictionary as a list and was corrected before starting builds. Neither was a runtime/test failure. The earlier formatter session was no longer available after context recovery; a new `rustfmt --check` passed before the source commit. No authoritative source/test mismatch occurred.

[Validation summary]({v}validation-summary.json), [unit evidence]({v}unit-source.json), [build provenance]({v}build-provenance.json), [source freshness]({v}source.json), [root comparison]({v}root-comparison.json), [fraction reference reuse]({v}fraction-reference-reuse.json).

## Preserved measurements and next step

Every window passed the quiet gate and was archived as the first subsequent remote operation. Counts below include all four engines; access verification is checksum-based, while the other windows also compare full-output hashes.

| Window | Start UTC | Finish UTC | Load before → after | Timed trials | Verification trials |
|---|---|---|---:|---:|---:|
{chr(10).join(window_rows)}

Total: **2,416 timed trials and 372 verification trials**, with declared counts, checksums, CPU/RSS vectors and medians, source patches, versions and 104 staged hashes checked. No new rotating-input, retained-output, dedicated short-call focus or stringify-options performance windows ran. Prepared supplement drivers are not execution evidence. The remaining qualification was stopped once the persistent small-record stringify trend prevented acceptance.

The next investigation should compare the actual emitted small-record stringify hot path between these frozen builds before adding another runtime optimization. Repeating the already-proven main rebuild or identical-binary filename control would not answer that question. An instruction, branch, or code-placement explanation remains unproven; the parser improvement must not be landed with an unexplained adverse control trend.

[Validation artifact manifest]({v}manifest.json).
'''
(bench / 'WARM_KEY_UNIQUENESS_R21.md').write_text(report)
print('Wrote R21 parked report, complete38+12 tables, five measured windows and unresolved limitations.')
