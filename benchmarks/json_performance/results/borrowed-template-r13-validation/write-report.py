from pathlib import Path
import json

w = Path(__file__).resolve().parent
bench = w.parents[1]
a = json.loads((w / 'rotating-analysis.json').read_text())
r = json.loads((w / 'rotating-recheck-analysis.json').read_text())
p = json.loads((w / 'provenance.json').read_text())
changing = next(c for c in r['cases'] if c['mode'] == 'rotating')
same = next(c for c in r['cases'] if c['mode'] == 'same')
assert changing['delta_pct'] > 0 and changing['slower_pairs'] == 11
assert not changing['separated_regression'] and same['separated_improvement']
v = 'results/borrowed-template-r13-validation/'
verdict = 'R13 rejected for landing: repeated-input small-record parsing improves 10.47% with separated ranges, but changing-input parsing remains 0.93% slower in all eleven recheck pairs, with overlapping ranges. The initial changing-input slowdown was 0.82% with separated ranges. The no-regression requirement is not established. 508 timed trials, 68 calibration trials and 116 full-output checks across two archived quiet windows. 297 JSON units, 28 Node fixtures and 14 options per arm pass. Seven native root findings remain unsuppressed and identical to main; both shadow checks pass. Known lazy/fraction gaps and lint freshness failure remain. Full and broader qualification not run.'
(w / 'validation-verdict.txt').write_text(verdict + '\n')
lines = ['# Borrowed JSON object templates (R13)', '',
    '**Rejected for landing.** Repeated-input small-record parsing improves 10.47% in the longer recheck, with separated sample ranges. Changing-input parsing remains 0.93% slower in all eleven pairs, with overlapping ranges. Its initial 0.82% slowdown had separated ranges. The persistent changing-input cost remains unresolved, so this combined candidate does not meet the no-regression requirement.', '',
    f"Measured source `{p['source_commit']}` on `codex/json-borrowed-template-r13`, compared with actual main `{p['base_commit']}` (0.5.1531). It retains R11/R12's source-length and construction-context changes; R8/R9/R10 production changes are excluded. The original fresh-main compiler and archives were reused by verified hashes, with workers, fixtures and IR recompiled at R13 paths. Remote main was independently reconfirmed before timing.", '',
    'The cache-hit path now borrows its immutable construction template inside the existing GC suppression scope. Fresh objects and mutable arrays are still allocated for every parse. The initial cache-match predicate is inlined; its construction helper is called only on a match. Cache admission and bounds, root registration, collection hooks, allocator policy and output ownership remain unchanged.', '',
    'The existing pending-collection hook runs before the cache is borrowed. The borrow ends before suppression is lifted and before cleanup/scheduling hooks. Source inspection found no user callback or cache-replacement path during construction; the existing GC scanner remains responsible for rewriting every cached heap pointer. This does not remove later tracing work.', '',
    '## Measurements', '',
    'Main, R13, Node 26.5.1 and Bun 1.3.14 ran on the quiet M1 Mac mini with 8 GiB RAM. Eight equal-size sources are preloaded outside timing. Rotation defeats the existing single-source cache; repeated-input and input-selection cases are separate controls. This is a bounded changing-input corpus, not a fresh source allocation on every call. Selection cost is not subtracted.', '',
    'The initial 15 cases use seven interleaved fresh-process repetitions. The two-case recheck uses eleven repetitions and four times the initial work: 2,325,580 changing-input calls and 6,818,180 repeated-input calls, both with 5,000 warmup calls. Total: 508 timed trials, 68 calibration trials and 116 complete-output verification trials. Verification checks all eight corpus members plus the actual final loop value against Node, including multiple rotating end indices.', '',
    'CPU is microseconds per operation; negative deltas are faster. Separated means observed sample ranges do not overlap, not a confidence interval or proof of cause. Every sample and outlier is retained. Peak RSS is whole-process MiB, including eight live inputs, outputs, runtime and allocator storage; it is not retained heap or directly comparable with the single-input worker.', '',
    'Initial changing-input Unicode parsing improves 47.40%, ASCII-string parsing 11.93%, and 1 MiB record-array parsing 1.09%; repeated-input record-array parsing improves 1.00%. Unicode and ASCII still trail Bun. Repeated-input small-record parsing improves 9.74% initially and 10.47% in the recheck. The changing-input slowdown persists. The other nine initial controls have overlapping ranges.', '',
    'In the longer recheck, small-record peak RSS is 31.969 MiB versus main’s 31.984 MiB with changing inputs and 146.578 MiB for both with repeated input. This shows no material peak-RSS change in those controls; it is not a retained-memory qualification.', '']
for label, data, suffix in [('Initial comparison', a, 'rotating'), ('Longer recheck', r, 'recheck-rotating')]:
    win = data['window']; slug = 'quiet-' + w.name + '-' + suffix
    lines += [f'## {label}', '',
        f"Quiet window {win['started_utc']}–{win['finished_utc']}; one-minute load {win['load_before'][0]:.3f}→{win['load_after'][0]:.3f}. The quiet gate passed and no competing workload was detected at either boundary. Each terminal window was archived before the next remote operation.", '',
        '| Fixture / mode | Iterations | Main CPU | R13 CPU | Node CPU | Bun CPU | R13 vs main | Ranges | Slower pairs |',
        '|---|---:|---:|---:|---:|---:|---:|---|---:|']
    for c in data['cases']:
        e = c['engines']; values = ' | '.join(f"{e[n]['cpu_us']:.6f}" for n in ['baseline','perry','node','bun'])
        note = 'separated slowdown' if c['separated_regression'] else 'separated gain' if c['separated_improvement'] else 'overlap'
        lines.append(f"| {c['fixture']} / {c['mode']} | {e['perry']['iterations']} | {values} | {c['delta_pct']:+.2f}% | {note} | {c['slower_pairs']}/{len(e['perry']['cpu_samples_us'])} |")
    lines += ['', '| Fixture / mode | Main peak RSS | R13 peak RSS | Node peak RSS | Bun peak RSS |', '|---|---:|---:|---:|---:|']
    for c in data['cases']:
        values = ' | '.join(f"{c['engines'][n]['peak_rss_mib']:.3f}" for n in ['baseline','perry','node','bun'])
        lines.append(f"| {c['fixture']} / {c['mode']} | {values} |")
    lines += ['', f'[Timing](results/{slug}/timing.jsonl), [calibration](results/{slug}/calibration.jsonl), [full-output verification](results/{slug}/verify.jsonl), [host and sources](results/{slug}/host.json), [quiet window](results/{slug}/window.json).', '']
lines += ['## Validation and machine code', '',
    '- Clean-source production build: `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static`, 334.50 seconds. Compiler and both archives were frozen, hashed, and checked for mtimes after build start. All four generated worker objects match main byte-for-byte and link the corresponding frozen runtime.',
    '- `RUST_TEST_THREADS=1 cargo test --release -p perry-runtime --lib json`: 297 pass. The new unit exercises cache replacement immediately after a hit, distinct object/array identities, and retention of prior output. Existing large-string Unicode/malformed-byte and fallback coverage remains.',
    '- Main and candidate each pass 28 Node behavioral runs across auto/tape/direct parsing and normal/scheduled/full GC, plus 14 stringify-options checks. All scheduled checks assert positive protected page sets and moved objects. The new cache-mutation/retention fixture has 2,158 protected sets and 99,893 moved objects under auto/direct parsing; tape has 1,999 and 95,991. Callback-only has 16 and 13,305.',
    '- Full native static checking retains seven UNSUPPRESSED findings: five unrooted globals, one unrooted string handle and one stale allocation value. All match actual main fingerprints: 83 functions, 2,226 statepoints and 8,372 relocates. All fourteen IR files match after removing only the first native ModuleID path comment; shadow IR needs no normalization. Both shadow variants pass, and native ordinary/callback controls have zero findings. No allowance was increased; this is not an all-clean static result.',
    '- Script lint: 73/74 pass; public benchmark evidence freshness fails. Compile tier and two CI-only checks are skipped. The Rust file cap passes. Full CI is not claimed.', '',
    f'[Build]({v}build-provenance.json), [main provenance]({v}reference-main.json), [worker comparison]({v}worker-object-comparison.json), [behavior]({v}candidate-fixture-validation.json), [options]({v}candidate-options-validation.json), [root comparison]({v}root-comparison.json), [lint]({v}script-lint.log.gz).', '',
    'Disassembly confirms removal of the 583-byte whole-template memcpy and the separate 64-byte planned-array vector copy. The helper loads planned values directly from the cache. Its frame shrinks from 912 to 224 bytes, including saved registers; parse_slow remains 512 bytes. The remaining 64-byte external call initializes the output-value array with memset_pattern16, as verified against the indirect-symbol table; it is not a template copy. The cache borrow count is decremented before suppression restoration.', '',
    f'[Borrow/reentrancy audit]({v}borrow-audit.json), [machine comparison]({v}cached-machine-comparison.json), [main symbols]({v}main-cached-entry-machine.json), [candidate symbols]({v}candidate-cached-entry-machine.json).', '',
    '## Limits and next investigation', '',
    'All 24 lazy-array probes preserve actual main outcomes and complete stdout. All twelve two-record cases match Node. At 180 records, six zero/true spacing cases crash with SIGSEGV, two plain whitespace/duplicate-key cases return noncanonical raw JSON, and four remaining cases pass. The main/Bun versus Node positive fractional-spacing difference is separately preserved. These baseline failures are not conformance passes.', '',
    f'[Lazy main]({v}lazy-main-probes.json), [lazy candidate]({v}lazy-candidate-probes.json), [fraction baseline]({v}fraction-baseline.json), [fraction candidate]({v}candidate-fraction.json).', '',
    f'[Initial verifier]({v}analyze-rotating.py) and [recheck verifier]({v}analyze-rotating-recheck.py) independently check timing/calibration checksums, full-output hashes, CPU/RSS vectors, source/worker/corpus hashes, patches and windows. [Initial vectors]({v}rotating-analysis.json), [recheck vectors]({v}rotating-recheck-analysis.json), [manifest]({v}manifest.json). Initial staging verified 102 remote hashes; recheck staging verified 104, including unchanged binaries and corpus.', '',
    'The next experiment should isolate only the borrowed-template change on actual main, excluding R11/R12’s source-length and construction-context changes. This separates the established repeated-input benefit from the persistent changing-input cost; it does not assume which change caused that cost.', '',
    'The original 38 parse/stringify rows plus consumption, broader access/rotating, retained-memory, short-call and stringify-options timing were not rerun after these controls failed qualification. Prepared drivers are not timing evidence. The full objective remains open. R13 is parked without a PR or release version bump.', '',
    '[R12 report](https://github.com/PerryTS/perry/blob/72bfd454b960eb0005cbfaa90cbf76dcdcbd103c/benchmarks/json_performance/CONSTRUCTION_CONTEXT_R12.md) records the preceding rejected experiment. Earlier accepted work landed through [merge train #10037](https://github.com/PerryTS/perry/pull/10037).', '']
report = '\n'.join(lines)
(bench / 'BORROWED_TEMPLATE_R13.md').write_text(report)
(w / 'report-draft.md').write_text(report)
print('Wrote all 17 CPU/RSS rows, both windows, evidence and qualification limits.')
