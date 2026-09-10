from pathlib import Path
import json

w = Path(__file__).resolve().parent
bench = w.parents[1]
a = json.loads((w / 'rotating-analysis.json').read_text())
r = json.loads((w / 'rotating-recheck-analysis.json').read_text())
p = json.loads((w / 'provenance.json').read_text())
assert r['cases'][0]['separated_regression'] and r['cases'][0]['slower_pairs'] == 11
v = 'results/source-length-r11-validation/'
lines = ['# JSON source string length reuse (R11)', '',
    '**Rejected for landing.** Changing-input Unicode parsing uses 47.42% less CPU and ASCII-string parsing 12.26% less, but small-record parsing is 0.94% slower initially and 0.88% slower in the longer recheck. All seven initial pairs and all eleven recheck pairs are slower, with separated sample ranges. This fails the requested no-regression requirement.', '',
    f"Source `{p['source_commit']}` on `codex/json-source-length-r11`, forked from actual main `{p['base_commit']}` (0.5.1531). R8/R9/R10 production experiments are excluded. The exact fresh-main build is reused by verified hashes; main workers, behavioral fixtures and IR were recompiled at R11 paths. Remote main was confirmed at this revision before the experiment.", '',
    'An unescaped token of at least 512 KiB can reuse the source header’s UTF-16 length when surrounding bytes are ASCII and at most one eighth of the token length. A bounded tail check rejects any lead byte that could make the legacy malformed-byte counter consume the closing quote. The ASCII prefix proves the token starts at a decoder boundary. Checked subtraction then gives the token length without rescanning it. Otherwise the existing counter is used.', '',
    'The output still uses the original JSON construction allocator and copies its bytes. This adds no source view, cache, GC policy, root registry entry or retained intermediate. Existing UTF-16 counting is already vectorized for large strings; this experiment removes that pass rather than replacing a scalar counter.', '',
    '## Measurement', '',
    'Four engines on the M1 Mac mini (8 GiB): main, R11, Node 26.5.1 and Bun 1.3.14. Eight equal-size source strings are preloaded outside timing. Rotating them defeats the existing single-source cache; repeated-input and input-selection cases remain separate controls. This is a bounded changing-input corpus, not a newly allocated string on every call. Selection cost is reported without subtraction.', '',
    'Initial measurements have seven interleaved fresh-process repetitions. The longer small-record recheck has eleven repetitions and exactly four times the original count: 2,312,136 calls, with the same 5,000-call warmup. There are 464 timed trials, 64 calibration trials and 112 full-output verification trials overall. Every verification compares all eight source members and the actual final timed value against Node; rotating verification ends at multiple corpus indices.', '',
    'CPU is microseconds per operation. Negative deltas are faster. “Separated” means observed sample ranges do not overlap; it is not a confidence interval or proof of causality. Every sample and outlier is retained. Peak RSS is whole-process MiB, including eight live input strings, outputs, runtime and allocator storage. It is not retained heap and cannot be directly compared with the single-input worker’s RSS.', '',
    'Unicode parsing falls from 142.557 to 74.957 µs, versus Bun 63.922 and Node 440.438. ASCII-string parsing falls from 99.834 to 87.596 µs, versus Bun 69.746 and Node 371.869. Both still trail Bun. Rotating escaped strings and the 1 MiB record array have overlapping ranges versus main. Repeated-input and selection controls also have overlapping ranges. The overall CPU/memory goal remains open.', '']
for label, data, suffix in [('Initial comparison', a, 'rotating'), ('Longer recheck', r, 'recheck-rotating')]:
    win = data['window']
    slug = 'quiet-' + w.name + '-' + suffix
    lines += [f'## {label}', '',
        f"Quiet window {win['started_utc']}–{win['finished_utc']}, one-minute load {win['load_before'][0]:.3f}→{win['load_after'][0]:.3f}. The quiet gate passed, with no competing workload detected at either boundary. The terminal window was archived before the next remote operation.", '',
        '| Fixture / mode | Iterations | Main CPU | R11 CPU | Node CPU | Bun CPU | R11 vs main | Ranges | Slower pairs |',
        '|---|---:|---:|---:|---:|---:|---:|---|---:|']
    for c in data['cases']:
        e = c['engines']
        values = ' | '.join(f"{e[n]['cpu_us']:.6f}" for n in ['baseline', 'perry', 'node', 'bun'])
        note = 'separated slowdown' if c['separated_regression'] else 'separated gain' if c['separated_improvement'] else 'overlap'
        reps = len(e['perry']['cpu_samples_us'])
        lines += [f"| {c['fixture']} / {c['mode']} | {e['perry']['iterations']} | {values} | {c['delta_pct']:+.2f}% | {note} | {c['slower_pairs']}/{reps} |"]
    lines += ['', '| Fixture / mode | Main peak RSS | R11 peak RSS | Node peak RSS | Bun peak RSS |', '|---|---:|---:|---:|---:|']
    for c in data['cases']:
        values = ' | '.join(f"{c['engines'][n]['peak_rss_mib']:.3f}" for n in ['baseline', 'perry', 'node', 'bun'])
        lines += [f"| {c['fixture']} / {c['mode']} | {values} |"]
    lines += ['', f'[Timing samples](results/{slug}/timing.jsonl), [calibration samples](results/{slug}/calibration.jsonl), [full-output verification](results/{slug}/verify.jsonl), [host and source provenance](results/{slug}/host.json), [quiet window](results/{slug}/window.json).', '']
lines += ['## Build, behavior and GC validation', '',
    '- Exact clean-source build: `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static`, 334.72 seconds. Compiler and both static archives were frozen with hashes; all mtimes are after build start. All four generated worker objects are byte-identical to main and linked with the corresponding frozen runtime archive.',
    '- `RUST_TEST_THREADS=1 cargo test --release -p perry-runtime --lib json`: 295 tests pass. New coverage includes complete Unicode, non-ASCII surrounding syntax, ambiguous truncated tails, 20,000 arbitrary-byte cases against the existing scalar interpretation, and large full-entry strings with tracked allocation and exact bytes/lengths.',
    '- Main and candidate each pass 19 Node behavioral runs across auto/tape/direct parsing and normal/scheduled/full GC, plus 14 stringify-options checks. Scheduled runs assert positive protected page sets and movement. The new large-string fixture has 102 protected sets and 17,478/17,540/17,478 moved objects; callback-only pressure has 16 sets and 13,305 moved objects.',
    '- Full native static checking retains seven UNSUPPRESSED findings: five unrooted globals, one unrooted string handle and one stale allocation value. All match actual main fingerprints. Twelve IR files match after removing only the first native ModuleID path comment; shadow IR needs no normalization. Both shadow variants pass, and native ordinary/callback controls have zero findings. Seven versus R10’s nine reflects the changed fixture corpus, not a GC improvement. No allowance was increased and no all-clean static result is claimed.',
    '- Script lint: 73/74 pass; public benchmark evidence freshness fails. Compile tier and two CI-only checks are skipped. The Rust file cap passes. Full CI is not claimed.', '',
    f'[Build provenance]({v}build-provenance.json), [reference main]({v}reference-main.json), [worker comparison]({v}worker-object-comparison.json), [behavioral checks]({v}candidate-fixture-validation.json), [options checks]({v}candidate-options-validation.json), [root comparison]({v}root-comparison.json), [lint log]({v}script-lint.log.gz).', '',
    'Disassembly confirms that `parse_string_value` tests the large-token threshold and calls the new source-length helper. The parser and original byte constructor both retain their 96-byte frames. This identifies added dispatch instructions in the common string path; it does not prove that those instructions alone cause the small-record slowdown. A follow-up should investigate placing the metadata attempt inside existing large-string construction dispatch. That follow-up is not implemented or measured in R11.', '',
    f'[Main disassembly commands]({v}main-parser-machine.json), [candidate disassembly commands]({v}candidate-parser-machine.json).', '',
    '## Known baseline gaps and unrun work', '',
    'All 24 isolated lazy-array probes preserve actual main outcomes and complete stdout. All twelve two-record cases match Node. At 180 records, six zero/true spacing cases crash with SIGSEGV, two plain whitespace/duplicate-key cases return noncanonical raw JSON, and the remaining four cases pass. Preserving these failures is not conformance. The positive fractional-spacing difference between main/Bun and Node is also preserved separately, not counted as a Node pass.', '',
    f'[Lazy baseline]({v}lazy-main-probes.json), [candidate lazy probes]({v}lazy-candidate-probes.json), [fraction baseline]({v}fraction-baseline.json), [candidate fraction probe]({v}candidate-fraction.json).', '',
    f'[Initial verifier]({v}analyze-rotating.py) and [recheck verifier]({v}analyze-rotating-recheck.py) independently check every timed/calibration checksum, complete-output hash, sample vector/median, source/worker/corpus hash, patch and quiet window. [Initial analysis]({v}rotating-analysis.json) and [longer analysis]({v}rotating-recheck-analysis.json) contain every CPU/RSS vector. The [validation manifest]({v}manifest.json) records original and archived file hashes.', '',
    'The original full 38-row parse/stringify matrix plus consumption rows, broader access/rotating/retained/short-call suites and stringify-options timing were not rerun after this candidate failed the first control group. Prepared drivers are not measurement evidence. Their requirements remain open. R11 is parked without a PR or release version bump.', '',
    '[R10 report](https://github.com/PerryTS/perry/blob/ae4496ef64485bf57cc728db32763c69e157faa2/benchmarks/json_performance/INERT_SPACER_INLINE_R10.md) records the previous rejected experiment. The earlier accepted JSON work landed through [merge train #10037](https://github.com/PerryTS/perry/pull/10037).', '']
(bench / 'SOURCE_LENGTH_R11.md').write_text('\n'.join(lines))
(w / 'report-draft.md').write_text('\n'.join(lines))
print('Wrote all 16 CPU/RSS rows, both windows, validation and rejection limits.')
