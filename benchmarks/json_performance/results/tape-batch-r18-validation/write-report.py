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
assert read('root-comparison.json')['checks_match']
for arm in ['main', 'candidate']:
    for name, count in [('fixture', 37), ('options', 14)]:
        rows = read(arm + '-' + name + '-validation.json')
        assert len(rows) == count and all(r['matches_node'] for r in rows)
    for record in read(arm + '-workers-provenance.json'):
        for path, digest in record['files'].items():
            assert hashlib.sha256((root / path).read_bytes()).hexdigest() == digest
screen = read('screen-analysis.json')['phases']['focus']
recheck = read('recheck-analysis.json')['phases']['focus']
assert screen['timed_trials'] == 504 and recheck['timed_trials'] == 44
control = recheck['cases'][0]
assert control['separated_regression'] and control['slower_pairs'] == 11
v = 'results/tape-batch-r18-validation/'
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
e = control['engines']
profile_rows = []
for r in read('profiles-analysis.json')['cases']:
    p = r['phases']
    profile_rows.append('| ' + r['case']['mode'] + ' | ' + str(r['samples']) + ' | ' +
                        ' | '.join(f"{p[k]['pct_of_main_thread_samples']:.2f}%" for k in
                                   ('tape_build', 'lazy_full_materialization', 'tape_record_producer', 'direct_array_parser', 'collection')) + ' |')

verdict = ('Rejected after the longer control recheck: 1 MiB object parse is 0.9767% slower, separated with 11/11 slower pairs. '
           'Full scans improve 4.36% at 1 MiB and 6.00% at 8 MiB, with little RSS change. '
           'The 120-record/16 KiB scan cannot enter the changed producer under the existing guard. '
           'No PR/version bump or broader performance qualification; full objective remains open.')
(w / 'validation-verdict.txt').write_text(verdict + '\n')
report = f'''# Batched tape record construction (R18)

**Rejected: the longer control recheck confirms a regression.** Full-scan CPU improves 4.36% at 1 MiB and 6.00% at 8 MiB, but an unrelated 1 MiB object parse slows 0.98%, with separated samples and all 11 paired repetitions slower. The 16 KiB scan is essentially flat, and RSS changes little. This does not meet the no-regression requirement; the prototype is preserved on `codex/json-tape-batch-r18`, without a runtime PR or release bump.

Measured source `{head}`, directly based on main `1a9c0de6cb790d2467b0ca22a660870025179b37` (0.5.1531). [R17's diagnosis](https://github.com/PerryTS/perry/blob/fb87e2b4ff7d6a052d8a549f7c0595c00cd70941/benchmarks/json_performance/SCAN_PROFILE_R17.md) motivates this experiment. Earlier accepted JSON work landed through [merge train #10037](https://github.com/PerryTS/perry/pull/10037).

## What changed

Only the existing full lazy-materialization producer changes. A validated tape supplies record and field boundaries for records with at most eight field pairs, containing scalars or flat scalar arrays. The producer reuses the direct parser's string/number decoders, shape cache, ordinary object constructors and collection-suppressed construction batch. Unsupported later subtrees use the direct parser at their source offsets; an unsupported first record declines before allocation. The completed array uses its known length. Existing publication patches cached values back over the new slots, preserving their identity and mutations.

There is no new cache, leaf-end metadata, GC policy, threshold, lazy-admission or sparse-access change. GC files, parse entry dispatch, string constructors and stringify implementation retain main's source. The change therefore still pays to reconstruct cached subtrees before the existing patch overwrites them, and still decodes strings and numbers from their source bytes.

## CPU screen: all 18 cases

Same M1/8 GiB host; Node 26.5.1, Bun 1.3.14. Eighteen cases were declared before timing. Large-case work is twice the original matrix counts; tiny/small counts and all warmups stay unchanged. Seven interleaved fresh-process repetitions per engine/case give 504 timed trials and 90 full-output verifications. All checksums, complete-output hashes, declared iteration/warmup counts and CPU/RSS sample vectors pass verification. Both arms use identical generated worker objects linked to their respective frozen runtimes.

CPU is median user+system microseconds per operation. “Slower pairs” compares candidate to main; “separated” means the two seven-sample ranges do not overlap, not a statistical confidence interval.

| Fixture / operation | Main µs/op | R18 µs/op | Node µs/op | Bun µs/op | R18 vs main | Slower pairs | Sample ranges |
|---|---:|---:|---:|---:|---:|---:|---|
{chr(10).join(cpu_rows)}

[Predeclared counts]({v}screen-cases.json), [all samples and comparisons]({v}screen-analysis.json), [raw timings](results/quiet-tape-batch-r18-screen-focus/timing.jsonl), [full-output hashes](results/quiet-tape-batch-r18-screen-focus/verify.jsonl).

## Peak resident memory

Median process peak RSS, in MiB, including startup. This is separate from the instrumented sampling run below.

| Fixture / operation | Main MiB | R18 MiB | Node MiB | Bun MiB | R18 − main MiB |
|---|---:|---:|---:|---:|---:|
{chr(10).join(rss_rows)}

The 1 MiB scan is nearly unchanged in RSS; the 8 MiB scan saves about 2.47 MiB. This experiment does not resolve the excess survival or post-reclamation resident-memory retention documented in R17.

## Longer control recheck

The largest separated control slowdown, `records_object_1m / parse`, was rechecked with 324 iterations, two warmups and 11 repetitions per engine. That is four times the original matrix work. The two frozen workers are unchanged. All 44 timed checksums and five complete-output verifications pass.

| Main µs/op | R18 µs/op | Node µs/op | Bun µs/op | R18 vs main | Slower pairs | RSS delta |
|---:|---:|---:|---:|---:|---:|---:|
| {e['baseline']['cpu_us']:.6f} | {e['perry']['cpu_us']:.6f} | {e['node']['cpu_us']:.6f} | {e['bun']['cpu_us']:.6f} | {control['delta_pct']:+.4f}% | 11/11, separated | {control['rss_delta_mib']:+.5f} MiB |

This confirms rejection without spending a full qualification run. It does not establish why unchanged source paths slowed. No attribution to a filename, compiler layout, allocator, GC policy or rebuild identity is proven here.

[Recheck declaration]({v}recheck-cases.json), [all samples]({v}recheck-analysis.json), [raw timings](results/quiet-tape-batch-r18-recheck-focus/timing.jsonl).

## Actual sampled mechanism

Two predeclared, instrumented 1 MiB scans run 600 iterations each, with two warmups. Both outputs match Node, worker and sampler exits are zero, and the 2 GiB/20-second watchdogs are not reached. All 105 staged input hashes match. The profiles establish actual candidate producer entry, beyond the separate linkage witness.

These are inclusive short sampled stack shares, not exact phase timers. Full materialization contains the tape producer or direct array parser, so those columns overlap and **must not be added**.

| Arm | Main-thread samples | Tape build | Full materialization | New tape producer | Direct array parser | Collection |
|---|---:|---:|---:|---:|---:|---:|
{chr(10).join(profile_rows)}

The new producer replaces the full-array parser in the sampled candidate, yet materialization still accounts for about half the sampled work. Removing structural reparsing this way yields only the modest uninstrumented gains above; it does not remove object/string allocation or scalar decoding. Sampling RSS and elapsed time are diagnostic and are not used as speedup measurements.

[Profile analysis]({v}profiles-analysis.json), [profile records](results/quiet-tape-batch-r18-profiles/profiles.json), [profile declaration]({v}profile-cases.json), [linkage witness]({v}producer-symbols.json).

The 16 KiB fixture has 120 records. The existing sequential-scan rule requires a streak of at least `max(64, length / 64)` and fewer than half the records cached. At a streak of 64, the second condition is already false; sequential traversal also does not reach the alternative cumulative-walk threshold. Thus this row never enters the changed producer. The 1 MiB and 8 MiB arrays have 7,600 and 59,000 records and can enter it. This is a source-derived admission analysis, not an invented phase attribution for the small row. [Guard and fixture counts]({v}scan-admission-analysis.json).

## Correctness, GC and artifact provenance

- `RUST_TEST_THREADS=1 cargo test --release -p perry-runtime --lib json`: **293 pass on the committed source**. Two focused preliminary tests also pass. The new Rust coverage checks tape-produced scalars/records against the direct decoder and verifies declines preserve parser position. The new TypeScript fixture mixes duplicate/escaped keys, Unicode, numeric edges, shallow and fallback records, retained outputs, cached aliases/mutations, and scan/stringify-triggered materialization.
- Both arms pass **37 Node behavioral runs** across automatic/tape/direct parsing and normal/scheduled/full GC, plus **14 stringify-option checks**. Scheduled runs assert actual movement and protected retired sets. The new fixture has 620 protected sets and 90,315 moved objects in auto/tape modes, and 620/88,873 in direct mode, on both arms.
- All sixteen emitted IR files match main after removing only the first native ModuleID path comment; shadow IR uses no normalization. Native analysis retains **eight unsuppressed findings identical to main**: five unrooted globals, two string-handle warnings and one stale allocation value. The new fixture adds one of those string-handle warnings to this expanded baseline corpus. Coverage is 2,469 safepoints, 1,819 live bundles and 8,998 relocates. Both shadow checks and the ordinary-worker/callback native subsets pass. This is not an all-clean native static result.
- All 24 lazy probes preserve main's exit codes and full stdout: six 180-record zero/true-spacing cases still SIGSEGV, two plain whitespace/duplicate-key cases still return noncanonical JSON, and the other sixteen match Node. The main/Bun versus Node fractional-spacing difference also remains. These are preserved baseline gaps, not conformance passes.
- Script lint passes **73/74**, with the existing public-benchmark freshness failure. The file cap passes. The linted staged patch is byte-identical to the final committed source, verified separately. Compile tier and two CI-only checks were skipped; full CI is not claimed.

The exact production command was `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static`. From the clean committed source it took 332.49 seconds. The compiler and both archives have mtimes after build start and were frozen with verified hashes. Mtime-only invalidation of the two static-wrapper entrypoints and compiler entrypoint is recorded; source bytes and build flags are unchanged. Main reuses the original fresh main build by verified hashes, with workers, fixtures and IR recompiled at the common R18 paths.

The first options-validation setup attempt failed before executing Perry because the JS oracle worker had not been copied. That failure is preserved; after copying the unchanged JS and building main workers, all fourteen checks passed. No failed subject run is discarded.

[Units]({v}unit-source.json), [build]({v}build-provenance.json), [source and freshness]({v}source.json), [main reference]({v}reference-main.json), [candidate behavior]({v}candidate-fixture-validation.json), [options]({v}candidate-options-validation.json), [static comparison]({v}root-comparison.json), [lint equivalence]({v}lint-source-equivalence.json), [setup failure]({v}main-options-setup-failure.log.gz).

## Preserved windows and next investigation

All times are UTC on 2026-09-10. Every terminal window was archived as the first subsequent remote operation.

| Window | Start–finish | Load before → after | Verdict |
|---|---|---|---|
| 18-case screen | 22:32:12–22:36:10 | 1.618 → 2.118 | Quiet pass |
| Two profiles | 22:38:22–22:38:26 | 1.790 → 1.727 | Quiet pass |
| Longer control | 22:41:35–22:42:07 | 1.214 → 1.835 | Quiet pass |

Total: **548 uninstrumented timed trials, 95 full-output verification trials and two instrumented profiles**. No full-50, access, rotating, retained-output, short-call or options performance qualification ran. Prepared drivers are not execution evidence. Parsing and stringify remain in the unchanged full objective.

Before another code variant, an independently rebuilt main should be compared to the existing frozen main reference on the recurring controls. Prior identical-binary A/A checks were flat; they do not test rebuilding. This is a diagnostic next step, not evidence that the existing baseline is wrong. The 120-record path also needs a producer that can efficiently combine already-cached records with the uncached remainder, with a cost-based admission review and identity/mutation/GC checks. Merely changing the threshold to make one row win is not a validated solution.

[Validation archive manifest]({v}manifest.json).
'''
(bench / 'TAPE_BATCH_R18.md').write_text(report)
print('Wrote rejected R18 report: all 18 rows, confirmed control regression, actual profiles and explicit limits.')
