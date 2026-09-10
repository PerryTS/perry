from pathlib import Path
import json

w = Path(__file__).resolve().parent
bench = w.parents[1]
a = json.loads((w / 'rotating-analysis.json').read_text())
r = json.loads((w / 'rotating-recheck-analysis.json').read_text())
p = json.loads((w / 'provenance.json').read_text())
assert any(c['separated_regression'] for c in r['cases'])
assert all(c['slower_pairs'] == 11 for c in r['cases'])
v = 'results/construction-context-r12-validation/'
lines = ['# JSON construction context (R12)', '',
    '**Rejected for landing.** The longer recheck shows repeated-input small-record parsing +1.09%, with separated sample ranges. Changing-input small-record parsing remains +0.78%, with overlapping ranges. All eleven pairs are slower in both cases. The large-string gains remain, and 1 MiB record-array parsing improves, but the no-regression requirement is not met.', '',
    f"Measured source `{p['source_commit']}` on `codex/json-construction-context-r12`, based on actual main `{p['base_commit']}` (0.5.1531). R11’s source-length admission remains; R8/R9/R10 production changes are excluded. The original fresh-main build is reused by verified hashes. Main workers, fixtures and IR were recompiled at R12 paths; remote main was confirmed at this revision before the experiment.", '',
    'The parser passes itself as a compile-time string-construction context. The constructor selects its large-leaf path before counting units; only that path reads source metadata. Other callers specialize to their existing allocation batch with no source metadata. This removes R11’s extra size check and source-argument loading from the parser. There is no virtual dispatch, new cache, source view, arena field or GC policy change.', '',
    'Source-length reuse still requires an unescaped token of at least 512 KiB, ASCII surrounding bytes no larger than one eighth of the token, and a bounded tail proof preventing a malformed sequence from consuming the closing quote in the legacy counter. Output bytes are still copied into the existing tracked allocation. Large-string counting was already vectorized; the saving comes from avoiding that pass.', '',
    '## Measurement', '',
    'Four engines on the quiet M1 Mac mini (8 GiB): main, R12, Node 26.5.1 and Bun 1.3.14. Eight equal-size sources are preloaded outside timing. Rotating them defeats the existing single-source cache; repeated-input and selection cases remain separate controls. This is a bounded changing-input corpus, not a freshly allocated source on every call. Selection cost is not subtracted.', '',
    'The initial 15 cases have seven interleaved fresh-process repetitions. The longer two-case small-record recheck has eleven repetitions and exactly four times each initial iteration count: 2,321,080 changing-input calls and 6,217,616 repeated-input calls, with the same 5,000-call warmup. Overall: 508 timed trials, 68 calibration trials and 116 full-output verification trials. Verification checks every corpus member and the actual final loop value against Node, with multiple rotating end indices.', '',
    'CPU is microseconds per operation; negative deltas are faster. “Separated” means observed sample ranges do not overlap, not a confidence interval or proof of cause. Every sample and outlier is retained. Peak RSS is whole-process MiB, including eight live inputs, outputs, runtime and allocator storage; it is not retained heap and is not directly comparable with the single-input worker.', '',
    'Changing-input Unicode parsing improves 47.52% and ASCII-string parsing 12.18%. The 1 MiB record array improves 0.99% with changing input and 1.23% with repeated input. Unicode and ASCII still trail Bun. Initially, small records slow 0.79% with changing input and 1.15% with repeated input, both with separated ranges. The recheck retains the repeated-input regression; overlapping changing-input ranges do not erase its positive median and eleven slower pairs. The other nine initial controls have overlapping ranges.', '']
for label, data, suffix in [('Initial comparison', a, 'rotating'), ('Longer recheck', r, 'recheck-rotating')]:
    win = data['window']
    slug = 'quiet-' + w.name + '-' + suffix
    lines += [f'## {label}', '',
        f"Quiet window {win['started_utc']}–{win['finished_utc']}, one-minute load {win['load_before'][0]:.3f}→{win['load_after'][0]:.3f}. The quiet gate passed, with no competing workload detected at either boundary. Each terminal window was archived before the next remote operation.", '',
        '| Fixture / mode | Iterations | Main CPU | R12 CPU | Node CPU | Bun CPU | R12 vs main | Ranges | Slower pairs |',
        '|---|---:|---:|---:|---:|---:|---:|---|---:|']
    for c in data['cases']:
        e = c['engines']
        values = ' | '.join(f"{e[n]['cpu_us']:.6f}" for n in ['baseline', 'perry', 'node', 'bun'])
        note = 'separated slowdown' if c['separated_regression'] else 'separated gain' if c['separated_improvement'] else 'overlap'
        reps = len(e['perry']['cpu_samples_us'])
        lines += [f"| {c['fixture']} / {c['mode']} | {e['perry']['iterations']} | {values} | {c['delta_pct']:+.2f}% | {note} | {c['slower_pairs']}/{reps} |"]
    lines += ['', '| Fixture / mode | Main peak RSS | R12 peak RSS | Node peak RSS | Bun peak RSS |', '|---|---:|---:|---:|---:|']
    for c in data['cases']:
        values = ' | '.join(f"{c['engines'][n]['peak_rss_mib']:.3f}" for n in ['baseline', 'perry', 'node', 'bun'])
        lines += [f"| {c['fixture']} / {c['mode']} | {values} |"]
    lines += ['', f'[Timing](results/{slug}/timing.jsonl), [calibration](results/{slug}/calibration.jsonl), [complete-output verification](results/{slug}/verify.jsonl), [host and source provenance](results/{slug}/host.json), [quiet window](results/{slug}/window.json).', '']
lines += ['## Validation', '',
    '- Exact clean-source build: `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static`, 331.36 seconds. Compiler and both static archives were frozen with hashes and mtimes after build start. All four generated worker objects are byte-identical to main and linked with the corresponding frozen runtime.',
    '- `RUST_TEST_THREADS=1 cargo test --release -p perry-runtime --lib json`: 296 tests pass. R11’s Unicode/malformed-byte coverage remains, including 20,000 arbitrary-byte cases and tracked large allocations. The added test checks small and large Unicode output through a parser with no managed source and through a batch-only constructor.',
    '- Main and candidate each pass 19 Node behavioral runs across auto/tape/direct parsing and normal/scheduled/full GC, plus 14 stringify-options checks. Every scheduled run asserts positive protected page sets and moved objects. The large-string fixture has 102 protected sets and 17,478/17,540/17,478 moved objects; callback-only has 16 sets and 13,305 moved objects.',
    '- Full native static checking retains seven UNSUPPRESSED findings: five unrooted globals, one unrooted string handle and one stale allocation value. All match actual main fingerprints. Twelve IR files match after removing only the first native ModuleID path comment; shadow IR needs no normalization. Both shadow variants pass. Native ordinary/callback controls have zero findings. No allowance was increased and this is not an all-clean static result.',
    '- Script lint: 73/74 pass; public benchmark evidence freshness fails. Compile tier and two CI-only checks are skipped. The Rust file cap passes. Full CI is not claimed.', '',
    f'[Build provenance]({v}build-provenance.json), [reference main]({v}reference-main.json), [worker comparison]({v}worker-object-comparison.json), [behavioral checks]({v}candidate-fixture-validation.json), [options checks]({v}candidate-options-validation.json), [root comparison]({v}root-comparison.json), [lint log]({v}script-lint.log.gz).', '',
    '## Machine findings and next investigation', '',
    'The parser’s extra large-size shift/test is gone. Its string constructor call receives the parser directly; the size test now sits at the constructor entry, with a tail call to a separate large constructor. The parser, both normal constructor specializations and both large constructor specializations use 96-byte frames. This source change did not eliminate the measured small-record regressions.', '',
    'The cached small-object path returns before string construction, so its slowdown cannot be explained solely by that per-string dispatch. Both main and R12 enter a 512-byte parse_slow frame (96-byte save area plus 416 bytes) and a 912-byte reuse-helper frame (96 plus 816). Source and disassembly show the cached template copied to the stack: a 583-byte memcpy plus scalar header fields, and a separate 64-byte copy for a planned array. The indirect-symbol tables confirm the memcpy stub, which plain disassembly misleadingly labels relative to writev.', '',
    'A concrete follow-up is to examine borrowing the immutable template and its planned array values while the existing GC suppression scope covers construction, then release the borrow before scheduling/cleanup hooks. A smaller cache-miss entry could also avoid the construction frame on misses. Reentrancy and rooting must be checked before implementation. This is an investigation lead, not a proven cause of R12’s regression or an implemented R13 change.', '',
    f'[Main constructor disassembly]({v}main-parser-machine.json), [candidate constructor disassembly]({v}candidate-parser-machine.json), [cached-entry disassembly]({v}cached-entry-machine.json), [main indirect symbols]({v}main-indirect-symbols.txt), [candidate indirect symbols]({v}candidate-indirect-symbols.txt).', '',
    '## Known gaps and remaining scope', '',
    'All 24 isolated lazy-array probes preserve actual main outcomes and complete stdout. All twelve two-record cases match Node. At 180 records, six zero/true spacing cases crash with SIGSEGV, two plain whitespace/duplicate-key cases return noncanonical raw JSON, and four remaining cases pass. The main/Bun versus Node positive fractional-spacing difference is also preserved separately. These baseline failures are not conformance passes.', '',
    f'[Lazy baseline]({v}lazy-main-probes.json), [candidate lazy probes]({v}lazy-candidate-probes.json), [fraction baseline]({v}fraction-baseline.json), [candidate fraction check]({v}candidate-fraction.json).', '',
    f'[Initial verifier]({v}analyze-rotating.py) and [recheck verifier]({v}analyze-rotating-recheck.py) independently check every timed/calibration checksum, complete-output hash, CPU/RSS sample and median, source/worker/corpus hash, patch and window. [Initial vectors]({v}rotating-analysis.json), [recheck vectors]({v}rotating-recheck-analysis.json), [validation manifest]({v}manifest.json). Initial staging verified 102 remote hashes; recheck staging verified 104, including unchanged binaries and corpus.', '',
    'The original 38 parse/stringify rows plus consumption, broader access/rotating, retained-memory, short-call and stringify-options timing were not rerun after this candidate failed its first controls. Prepared drivers are not timing evidence. The full objective and all those qualification requirements remain open. R12 is parked without a PR or release version bump.', '',
    '[R11 report](https://github.com/PerryTS/perry/blob/c5d55834dc9e1369d3b2c2aacc874a7866d663bb/benchmarks/json_performance/SOURCE_LENGTH_R11.md) records the preceding rejected experiment. Earlier accepted work landed through [merge train #10037](https://github.com/PerryTS/perry/pull/10037).', '']
(bench / 'CONSTRUCTION_CONTEXT_R12.md').write_text('\n'.join(lines))
(w / 'report-draft.md').write_text('\n'.join(lines))
print('Wrote all 17 CPU/RSS rows, both windows, validation and rejection limits.')
