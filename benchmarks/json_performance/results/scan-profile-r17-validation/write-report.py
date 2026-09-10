from pathlib import Path
import json

w = Path(__file__).resolve().parent
bench = w.parents[1]
read = lambda name: json.loads((w / name).read_text())
modes, profiles, memory = [read(name + '-analysis.json') for name in ('modes', 'profiles', 'memory')]
v = 'results/scan-profile-r17-validation/'
rows = []
rss_rows = []
for r in modes['cases']:
    e = r['engines']
    label = r['fixture'].replace('records_array_', '') + ' ' + r['operation']
    rows.append('| ' + label + ' | ' + str(e['main_auto']['iterations']) + ' | ' +
                ' | '.join(f"{e[k]['cpu_us']:.3f}" for k in ('main_auto', 'main_direct', 'node', 'bun')) +
                f" | {r['direct_vs_auto_cpu_delta_pct']:+.2f}% |")
    rss_rows.append('| ' + label + ' | ' +
                    ' | '.join(f"{e[k]['peak_rss_mib']:.3f}" for k in ('main_auto', 'main_direct', 'node', 'bun')) + ' |')
profile_rows = []
for r in profiles['cases']:
    c, p = r['case'], r['phases']
    profile_rows.append(f"| {c['operation']} {c['mode']} | {r['samples']} | " +
                        ' | '.join(f"{p[k]['pct_of_main_thread_samples']:.2f}%" for k in
                                   ('tape_build', 'lazy_full_materialization', 'direct_array_parser', 'collection')) + ' |')
memory_rows = []
for r in memory['cases']:
    c = r['case']
    values = [r['rss_after_timing_bytes'], *r['rss_snapshots_bytes'].values(),
              r['full_collection_freed_bytes'][0], r['final_arena']['arena_live']]
    label = c['fixture'].replace('records_array_', '') + ' ' + c['operation'] + ' ' + c['mode']
    memory_rows.append(f"| {label} | {c['iterations']} | " +
                       ' | '.join(f'{n / 1048576:.2f}' for n in values) + ' |')

report = f'''# Main full-scan diagnosis (R17)

**The lazy route pays for tape construction and later reparsing on full scans.** On unchanged main, forcing direct parsing reduces measured CPU by 55.1% for the 16 KiB scan, 28.8% for 1 MiB and 30.0% for 8 MiB. Direct parsing beats both Node and Bun on those measured scan rows. It also makes parse-only and sparse access 47–126% slower, with much higher RSS for the 8 MiB parse/sparse rows. Globally disabling lazy parsing would trade away existing wins.

This branch contains diagnostic evidence only. Source is main `1a9c0de6cb790d2467b0ca22a660870025179b37` (0.5.1531); no production runtime, collector policy, threshold or routing change was made. These results describe that pinned main, not a later release. Earlier accepted JSON work landed through [merge train #10037](https://github.com/PerryTS/perry/pull/10037).

## Matched CPU and RSS

Same M1/8 GiB host; Node 26.5.1 and Bun 1.3.14. Ten predeclared cases, seven interleaved fresh-process repetitions per case/engine, twice the original full-matrix iteration counts and unchanged warmup. CPU is median user+system microseconds per operation; RSS is median process peak RSS in MiB, including startup. All 280 timed checksums and 50 complete-output verifications pass. The two Perry modes execute **the exact same worker binary**; `main_direct` sets `PERRY_JSON_TAPE=0`, while `main_auto` unsets it. Node/Bun use their ordinary modes.

The 20 MiB scan is a same-route control: it exceeds the existing automatic lazy admission bound. Route selection changes construction and lifetime together; the difference is not an isolated parser-phase timer or a production speedup.

| Array / operation | Iterations | Auto CPU µs/op | Direct CPU µs/op | Node CPU µs/op | Bun CPU µs/op | Direct vs auto |
|---|---:|---:|---:|---:|---:|---:|
{chr(10).join(rows)}

| Array / operation | Auto peak MiB | Direct peak MiB | Node peak MiB | Bun peak MiB |
|---|---:|---:|---:|---:|
{chr(10).join(rss_rows)}

[All samples and comparisons]({v}modes-analysis.json), [raw timings](results/quiet-scan-profile-r17-modes/timing.jsonl), [output hashes](results/quiet-scan-profile-r17-modes/verify.jsonl), [predeclared cases]({v}mode-cases.json).

## Actual sampled stacks

Four instrumented 1 MiB profiles use `/usr/bin/sample` against the unchanged main worker. Each has over 750 main-thread samples, successful worker/sampler exits and complete output matching Node. Counts target about 1.4 seconds of CPU from the mode medians: parse auto/direct 1,537/750; scan auto/direct 478/671. These are short sampled inclusive stack shares, not exact phase timings or confidence intervals.

| Profile | Main-thread samples | Tape build | Full lazy materialization | Direct array parser | Collection |
|---|---:|---:|---:|---:|---:|
{chr(10).join(profile_rows)}

**The materialization and direct-parser columns overlap and must not be added.** In the auto scan, the direct parser is nested inside full materialization. Tape building plus materialization account for about 77% of the samples, consistent with the source path that validates a tape and then reparses the blob. The matched timings establish the route tradeoff; the samples locate the work.

Profile RSS is instrumented and counts differ between modes, so it is not used as a matched memory comparison. The separate matched 1 MiB scan above is 300.625 MiB auto versus 70.984 MiB direct.

[Profile analysis]({v}profiles-analysis.json), [profile records](results/quiet-scan-profile-r17-profiles/profiles.json), [predeclared profile counts]({v}profile-cases.json).

## Object lifetime and resident memory

Nine further probes retain the final output, explicitly collect, verify the live output again, then clear `last`, `input` and `retained` and collect again. Both explicit collections occur **after** measured work. Every output matches Node before and after the live collection; all 18 manual full collections ran and reclaimed bytes. The original harness's result RSS is before output verification; `MEMORY_LIVE` is after it, so the latter may include output-string allocation. The independent RSS monitor is approximate and is not `ru_maxrss`. These instrumented probes are not CPU speedup measurements.

All memory columns below are MiB. “First full freed” is collector-reported reclaimed bytes, not an RSS decrease. “Final arena live” is managed arena accounting, not total process memory.

| Probe | Iterations | RSS after work | RSS after verification | RSS after live GC | RSS after dropped GC | First full freed | Final arena live |
|---|---:|---:|---:|---:|---:|---:|---:|
{chr(10).join(memory_rows)}

The native tape is already released deterministically by `install_materialized` → `release_tape_after_materialize` → `json_tape_store::release`; its allocation drops and external-byte accounting decreases. RSS growth alone therefore does not establish a tape leak.

There is evidence of excess survival: the 1 MiB/138 auto-scan's first copying minor attributes about 12.1 MiB of promoted strings, objects and arrays to `remembered_set/lazy_array`. Lazy headers stay old and immovable, and their materialized/cache edges can keep young graphs alive until full collection. Later cycles include untraced in-place promotion; the scan has 65 copying-minor reports and no full collection before the two manual ones. The first manual full reclaims 222.39 MiB. At exit managed arena live bytes are 1.26 MiB, yet the post-drop RSS snapshot is 377.47 MiB. Reclamation happened; resident-memory retention remains a separate issue. This does not identify exactly which native allocator or runtime capacity holds every remaining page.

The longer auto probes grow substantially, while the matched direct scans remain near 32 MiB (16 KiB input) and 70 MiB (1 MiB input) before verification. The parse-only auto control instead runs 16 automatic `OldGenBytes` full collections plus the two manual ones, and zero copying minors. The analyzer preserves every cycle and selects the two Manual events explicitly. Its initial mistaken assumption that every case had only two full summaries, and the correction, are recorded; no collection or failure is hidden.

[Verified lifetime results]({v}memory-analysis.json), [probe source]({v}memory-worker.ts), [all probe records](results/quiet-scan-profile-r17-retry1-memory/memory-diagnostics.json), [analysis development notes]({v}analysis-development-notes.json).

## Windows, provenance and validation limits

All times are UTC on 2026-09-10. Each window had the exclusive benchmark lock and was archived as the first remote operation after termination.

| Window | Start–finish | Load before → after | Verdict |
|---|---|---|---|
| Modes | 21:22:37–21:25:19 | 1.218 → 2.115 | Pass |
| Samples | 21:34:40–21:34:46 | 2.346 → 2.446 | Pass |
| Initial memory | 21:47:50–21:48:02 | 2.407 → 2.504 | **Excluded**, over the unchanged 2.5 limit |
| Identical memory retry | 21:51:00–21:51:11 | 1.738 → 2.060 | Pass |

The initial memory outputs were archived and excluded before examining RSS to select the retry. Counts, input hashes, worker and runtime were unchanged. [Excluded window](results/quiet-scan-profile-r17-memory/INVALID_WINDOW.md). Raw logs and outputs are losslessly compressed with per-window hash manifests; failed-window evidence remains available.

The compiler and both static archives come from the original fresh main build using `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static`. It took 338.89 seconds, with all three mtimes after build start. R17 copied the frozen artifacts by verified hashes and recompiled its workers. Its cross-directory object-byte equality check initially failed before remote staging: generated objects embed different absolute source paths. Source hashes and all 114 defined worker symbols agree; cross-path object-byte equality is not claimed. Both timed Perry modes still share one identical R17 executable.

All 16 mode inputs, 18 profile inputs and 21 memory inputs were verified against staged hashes. The diagnostic worker's local before/after-GC smoke agrees with Node but is not performance evidence. [Fresh main build]({v}main-build-provenance.json), [reused artifacts]({v}reference-main.json), [worker provenance]({v}main-workers-provenance.json), [memory-worker provenance]({v}memory-worker-provenance.json), [path difference]({v}cross-path-object-review.json).

R17 changes no runtime source and does not rerun the full behavioral qualification. The same frozen main's prior Node fixtures, stringify options and actual moving/protected-GC checks are linked in the [validation reference]({v}validation-reference.json). Prior native static findings remain unsuppressed; this diagnosis is not a new all-clean static or GC-safety verdict. Existing lazy stringify crashes/noncanonical output and fractional-spacing differences remain baseline gaps, not conformance passes. Script lint passes 73/74 checks with the existing public-benchmark freshness failure; the file cap passes. Results are preserved in [lint provenance]({v}script-lint-source.json) and [lint output]({v}script-lint.log.gz). Compile and two CI-only gates were explicitly skipped, so full CI is not claimed.

## Next implementation

Prototype full-array construction from the validated tape, using the existing collection-suppressed construction batch and ordinary final objects. Preserve cached element identity and mutations, duplicate-key behavior, escaping/numbers and active incremental/full-GC fallbacks. The first experiment should change only the existing full-materialization producer; leave lazy admission, sparse access and GC scheduling intact. Verify that it removes duplicate parsing in actual profiles, then measure both parse and stringify, consumption, rotating inputs, retained results, short calls and options against the same main before considering a PR.

This CPU experiment cannot by itself promise lower lifetime-related RSS. The old-header retention and post-reclamation resident memory need separate evidence and a bounded fix, without hiding collection cost outside the reported workload. No runtime PR or release bump is created for this diagnostic branch, and the original no-regression/all-rows objective remains open.

[Diagnostic archive manifest]({v}manifest.json).
'''
(bench / 'SCAN_PROFILE_R17.md').write_text(report)
print('Wrote R17 report: 40 engine rows, four profiles, nine lifetime probes, one excluded window.')
