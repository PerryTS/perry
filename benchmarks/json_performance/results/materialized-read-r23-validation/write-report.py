from pathlib import Path
import json

w = Path(__file__).resolve().parent
bench = w.parents[1]
build = json.loads((w / 'build-provenance.json').read_text())
access = json.loads((w / 'access-analysis.json').read_text())['phases']['access']['cases']
full = json.loads((w / 'full-analysis.json').read_text())['phases']['full']['cases']
recheck = json.loads((w / 'regression-recheck-analysis.json').read_text())['phases']['focus']['cases']
control = json.loads((w / 'r22-control-analysis.json').read_text())['phases']['focus']['cases']
assert len(access) == 12 and len(full) == 50 and len(recheck) == len(control) == 5
assert sum(r['operation'] in ['parse', 'stringify'] for r in full) == 38
assert all(r['beats_both_peers'] for r in full if r['operation'] in ['parse', 'stringify'])

out = [
    '# Materialized lazy-array reads: R23', '',
    '**Unmerged and not qualified for landing.** Access loops improve by up to 49.5%, and 1 MB/8 MB full scans improve by about 4%. The wider matrix also exposes regressions against pinned main. Five representative regressions persist in an independent 11-repetition run and are already present in the earlier R22 sparse-cache version when linked with identical benchmark object code.', '',
    'Previously accepted JSON changes are merged through PR #10037, the merge train for closed PR #10036. R21 and R22 are separate unmerged experiments. R23 source is `' + build['source_commit'] + '` on `codex/json-materialized-read-r23`; no R23 PR has been opened. The controlled reference is independently built main `1a9c0de6cb790d2467b0ca22a660870025179b37` (0.5.1531). Remote main advanced to `603b074ace01464bc66fc07cc8d532f26ccf5a0f` (0.5.1532) during this investigation; that inspected delta contains release/CI plumbing and a workspace patch-version bump, with no runtime/compiler implementation change. This report is still explicitly a comparison to pinned main 1a9.', '',
    'The original 38 repeated-input parse/stringify medians remain faster than Node and Bun. That was already true of the reference; it does not establish general JSON dominance. Changing-input, retained-result and stringify-option performance controls were not rerun for R23 after the wider matrix found regressions.', '',
    '## Implementation and correctness', '',
    'R23 retains R22’s allocation-free sparse-cache hit and adds a dense read after full materialization. It resolves the live ordinary-array edge, checks descriptor flags and length/capacity, and returns an existing non-hole slot. Cold construction, holes, sparse indices, descriptors and other exceptional reads retain `lazy_get_rooted`. There is no production GC core, policy, threshold, parse-boundary or cache-admission change.', '',
    'The linked ARM64 array accessor contains a materialized-hit block that calls only the nonallocating ordinary-array resolver, plus the unchanged call-free sparse-hit block. Both return without opening handle scopes. The outer generic-array frame is still 112 bytes. The disassembly’s 18- and 14-instruction block counts exclude receiver classification and the epilogue and are not timing measurements.', '',
    '- 294 serial release JSON runtime tests pass (1.66 seconds of test execution). Coverage includes real `defineProperty` getter dispatch, mutation/growth/hole precedence, sparse identity and bitmap boundaries, and a materialized read after a witnessed copied-minor array move.',
    '- Both frozen arms pass 46 Node behavior comparisons and 14 stringify-option comparisons. The expanded cached-read fixture performs allocations between materialized passes. Scheduled auto/tape runs each record 2,608 protected retired sets and 103,251 moved objects; direct mode records 2,588 and 103,263. Every scheduled fixture asserts positive protection and movement.',
    '- All four benchmark object files are byte-identical between main and R23. All 18 native/shadow IR files match (only the first native ModuleID path comment is normalized). Shadow checks and the ordinary-worker/callback native subsets pass. The full native check retains **16 unsuppressed main findings: 15 unrooted, 1 stale**; this is not a clean full-native safety verdict. Coverage is 3,723 safepoints, 2,857 live bundles, 18,272 relocates and 18,182 safepoint/root pairs.',
    '- Final local lint passes 73 of 74 executed checks, including file size and GC custody checks. The pre-existing public-benchmark freshness check fails; the compile tier and two CI-only checks were skipped. This is not a full CI pass.', '',
    'Existing semantic failures remain visible: the 24-case lazy-spacer matrix has the same six SIGSEGV outcomes and two noncanonical outputs on both arms; the exact lazy-getter baseline still fails in auto/tape and passes in direct mode; fractional spacing still has the recorded main/Bun-versus-Node difference. These outcomes are preserved, not counted as conformance passes.', '',
    '## Measurement scope', '',
    'The quiet M1/8 GiB host used Node 26.5.1 and Bun 1.3.14. Each timing is a fresh process with interleaved engine order. All four windows passed the quiet gate and were archived before any subsequent remote operation. Analyzers verify checksums/output hashes, complete CPU/RSS sample vectors, medians, fixture/build/input hashes and source patches.', '',
    '| Window | UTC, 2026-09-11 | Timed trials | Verification records |',
    '|---|---|---:|---:|',
    '| R23 access, 12 cases × 7 reps × 4 engines | 05:45:51–05:46:15 | 336 | 12 |',
    '| R23 original 38 + 12 consumption, 7 reps | 05:47:22–05:54:12 | 1,400 | 250 |',
    '| R23 five-regression recheck, 11 reps | 05:59:32–06:00:24 | 220 | 25 |',
    '| R22 same-object-code control, 11 reps | 06:05:52–06:06:45 | 220 | 25 |', '',
    'There are 1,956 R23 timed trials plus 220 R22 control trials. Verification records are separate from timed trials; the ordinary harness records two Node verifications per case and one per other engine. “Separated regression/gain” means the complete seven- or eleven-sample ranges do not overlap. Overlap does not establish equivalence. CPU is user + system time per iteration; RSS is whole-process peak RSS and does not measure live-heap size or prove a leak.', '',
    '## Reads after one parse', '',
    'Parsing is outside the timed interval. These are access-loop iterations, not JSON.parse latency. The fields loop performs three indexed reads (`id`, `name.length`, `active`) per iteration. These parse-once RSS numbers must not be substituted for the repeated parse/consumption RSS below.', '',
]

def cpu_table(rows, label):
    out.extend([label, '', '| Fixture / operation | Main µs | R23 µs | Node µs | Bun µs | R23 vs main | Ranges |', '|---|---:|---:|---:|---:|---:|---|'])
    for r in rows:
        e = r['engines']
        verdict = 'regression' if r['separated_regression'] else 'gain' if r['separated_improvement'] else 'overlap'
        values = ' | '.join(f"{e[k]['cpu_us']:.6f}" for k in ['baseline', 'perry', 'node', 'bun'])
        out.append(f"| {r['fixture']} / {r['operation']} | {values} | {r['delta_pct']:+.2f}% | {verdict} |")
    out.append('')

def rss_table(rows, label):
    out.extend([label, '', '| Fixture / operation | Main MiB | R23 MiB | Node MiB | Bun MiB | R23 − main MiB |', '|---|---:|---:|---:|---:|---:|'])
    for r in rows:
        e = r['engines']
        values = ' | '.join(f"{e[k]['peak_rss_mib']:.3f}" for k in ['baseline', 'perry', 'node', 'bun'])
        out.append(f"| {r['fixture']} / {r['operation']} | {values} | {r['rss_delta_mib']:+.3f} |")
    out.append('')

cpu_table(access, 'Access CPU')
rss_table(access, 'Access RSS')
out.extend(['No access row has a separated regression. RSS differs from main by at most 0.015625 MiB. Despite the gains, the fields loop remains 10.42× Node at 16 KB and 5.60× Node at 1 MB; this is still a material remaining gap.', '', '## Original 38 parse/stringify rows and 12 consumption rows', '', 'The original iteration counts and warmups are retained in `full-cases.json`, with their reference commit/path/hash. “Sparse”, “scan” and “roundtrip” include parsing/consumption and differ from the parse-once access rows above.', ''])
cpu_table(full, 'Full matrix CPU')
rss_table(full, 'Full matrix RSS')
out.extend(['## Independent regression check and sparse-only control', '', 'The five rows below use the original work counts with eleven fresh-process repetitions per engine. Every listed slowdown has separated sample ranges and is slower in all eleven main/candidate pairs. The R22 control executable is compiled and linked with its frozen, verified compiler/runtime archives; its benchmark object file is byte-identical to both R23 arms. Its role and old source `8b7ffae96e733c96b31aa41d064902bc4f9a50ce` are explicit in the control archive.', '', '| Fixture / operation | R23 full run | R23 recheck | R22 same-object control |', '|---|---:|---:|---:|'])
initial = {(r['fixture'], r['operation']): r for r in full}
earlier = {(r['fixture'], r['operation']): r for r in control}
for r in recheck:
    key = r['fixture'], r['operation']
    assert r['separated_regression'] and r['slower_pairs'] == 11
    assert earlier[key]['separated_regression'] and earlier[key]['slower_pairs'] == 11
    out.append(f"| {key[0]} / {key[1]} | {initial[key]['delta_pct']:+.2f}% | {r['delta_pct']:+.2f}% | {earlier[key]['delta_pct']:+.2f}% |")
out.extend(['', 'This control establishes that those regressions already accompany the sparse-cache change. It does not prove a particular instruction or layout decision caused them. The next proposed experiment outlines the fast lazy accessor to test whether preserving the shared ordinary-array dispatcher’s structure removes them. That experiment is not part of R23.', '',
    '## Separate follow-up diagnostics', '',
    'A symbolized executable linked to the exact main runtime reproduces the lazy-array/zero-spacer crash in `write_escaped_bytes_from`, called by `js_json_stringify_full`; the apparent string length is `LAZY_ARRAY_MAGIC`. Source inspection finds missing lazy-array GC-tag cases in compact value/depth/per-element dispatch. An 18-case main-only root/object-wrapper/array-wrapper probe records 14 SIGSEGV outcomes and four Node matches. These overlap the existing failure family; they are not fourteen distinct bugs. The diagnostic executable retains symbols and is not used for performance claims. Replacer/pretty walks also need explicit GC custody checks when materialization introduces allocation.', '',
    'A separate, standalone scalar prototype compares a saturating u32 conversion plus exact float round-trip against the source-extracted current numeric-index predicate. It matches 15,525,263 tested bit patterns (2,000,477 accepted, 13,524,786 rejected). Source excerpts, hashes, Rust version, flags and assembly are recorded. This prototype is not integrated into the runtime, is not a complete semantic proof, and has no measured JSON speed claim.', '',
    '## Build and evidence', '',
    'The exact production command was `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static` on clean committed source, starting 2026-09-11T05:31:51.257171Z and completing in 381.60 seconds. All three emitted artifact mtimes are after the recorded start; frozen copies were hash-verified.', '',
    '| Artifact | SHA-256 |', '|---|---|'])
for name, row in build['files'].items():
    out.append(f"| {name} | `{row['sha256']}` |")
out.extend(['', 'Repair history is preserved: the first R23 unit compilation on 52570a335 was intentionally stopped after the new descriptor test tripped raw-handle debt lint; scoped handle calls fixed it without changing the ratchet. No production build was qualified from that superseded source. The initial candidate-validation controller later stopped on a missing copied fractional-spacing baseline JSON after the preceding checks had passed; its exact inputs were hash-verified, the missing file was restored and only the remaining checks resumed. Final source/tests/build/static/performance records all identify fc877118c.', '',
    'The four windows are under `results/quiet-materialized-read-r23-*`. The validation archive includes passing and failing outputs, source/build provenance, static findings, disassembly, standalone diagnostics and analyzers; executable/archive/object binaries are represented by hashes rather than committed. `results/materialized-read-r23-artifacts.json` indexes every committed evidence file. Prepared scripts for unexecuted controls are not evidence those controls ran.', ''])
(bench / 'MATERIALIZED_READ_R23.md').write_text('\n'.join(out))
(w / 'validation-verdict.txt').write_text('R23 fc877118c: 294 serial JSON units; exact all3 build; both arms46behavior14options; positive moving/protected witnesses;18equivalentIR retaining16unsuppressednativefindings;73/74lint knownpublicfreshnessFAIL and capPASS;1956R23 timed trials plus220R22 control trials over4quiet archived windows. Access gains up to49.5%, fullscan1m/8m about4%faster, but11full-matrix separated regressions and5independently persistent regressions alreadypresentinR22control. UNMERGED, NOT QUALIFIED. Existing lazy/spacing/getter failures preserved. No changing-input/retained/options performance rerun.\n')
print('Wrote report with all12 access and50 full CPU/RSS rows plus both five-case controls')
