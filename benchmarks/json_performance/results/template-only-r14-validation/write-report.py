from pathlib import Path
import json

w = Path(__file__).resolve().parent
bench = w.parents[1]
read = lambda name: json.loads((w / name).read_text())
rotating = read('rotating-retry1-analysis.json')
full = read('full-analysis.json')['phases']['full']
recheck = read('full-recheck-analysis.json')['phases']['full']
aa, aa_path = read('aa-analysis.json'), read('aa-path-analysis.json')
assert sum(c['separated_regression'] for c in recheck['cases']) == 3
assert not any(c['separated'] for a in [aa, aa_path] for c in a['cases'])
p = read('provenance.json'); v = 'results/template-only-r14-validation/'
verdict = 'R14 rejected for landing: isolated template borrowing improves small-record parse 11.30% and object_1k parse 12.35%, but the longer recheck retains separated slowdowns for 1 MiB object parse +0.97%, 20 MiB array parse +0.49%, and 20 MiB object parse +0.57%, all eleven pairs slower. Candidate evidence: 2,128 valid timed trials, 60 calibration trials, 385 full-output checks in three quiet windows. Two flat identical-main A/A controls add 176 diagnostic timings and 20 checks. One failed quiet window with 420 timings, 60 calibrations and 100 checks is separately preserved and excluded. 292 JSON units, 28 Node fixtures and 14 options per arm pass. Seven native findings remain unsuppressed and match main; shadow checks pass. Known lazy/fraction gaps and lint freshness failure remain. Broader qualification not run.'
(w / 'validation-verdict.txt').write_text(verdict + '\n')
lines = ['# Isolated borrowed JSON templates (R14)', '',
    '**Rejected for landing.** Isolating template borrowing on main preserves the small-object gains, but three large-workload slowdowns remain separated in the longer recheck: 1 MiB object parsing +0.97%, 20 MiB array parsing +0.49%, and 20 MiB object parsing +0.57%. All eleven pairs are slower in each. Two identical-main controls are effectively flat and do not explain these differences.', '',
    f"Measured source `{p['source_commit']}` on `codex/json-template-only-r14`, directly based on actual main `{p['base_commit']}` (0.5.1531). Only template borrowing, its predicate/construction split, two tests and an experiment fragment are added. R11/R12’s source-length and construction-context changes are excluded; the parser and string constructor match main’s source exactly. Main’s original fresh build was reused by verified hashes, with workers, fixtures and IR recompiled at R14 paths. Remote main was reconfirmed before timing.", '',
    'The cached plan is borrowed during the existing GC suppression scope. Every mutable object and nested array is still allocated afresh. The collection hook runs before the borrow; the borrow ends before suppression restoration, cleanup and scheduling. Cache bounds, admission, root registration and GC policy remain unchanged. The match predicate is inlined into parse_slow; a separate helper constructs cache hits.', '',
    '## Results and method', '',
    'Main, R14, Node 26.5.1 and Bun 1.3.14 ran on the quiet M1 Mac mini with 8 GiB RAM. Every sample and outlier is retained. CPU is microseconds per operation; negative deltas are faster. Separated ranges are an observed sample property, not a confidence interval or proof of cause. Peak RSS is whole-process MiB, including inputs, outputs, runtime and allocator storage; it is not isolated heap size.', '',
    'The 15 rotating/repeated/selection controls preload eight equal-size sources outside timing and use seven interleaved fresh-process repetitions. Rotating input defeats the existing single-source cache; it is not a fresh source allocation on every call, and selection cost is not subtracted. The full 50 cases cover the original 38 parse/stringify rows plus 12 consumption cases, with seven repetitions and fixed work. The seven-case recheck uses eleven repetitions and four times the initial work, retaining each original warmup.', '',
    'Valid candidate evidence totals 2,128 timed trials, 60 calibration trials and 385 complete-output checks across three quiet windows. The two diagnostic A/A controls add 176 timings and 20 complete-output checks; neither measures candidate code. All results were archived before the next remote operation.', '',
    'The first rotating window failed the pre-existing quiet gate: ending one-minute load was 2.594, despite no competing workload being detected at either boundary. All 420 timings, 60 calibrations and 100 verification records are preserved separately and excluded from performance qualification. Its medians were not inspected to select the retry. The same comparison was repeated in a new directory and passed the quiet gate.', '',
    '[Invalid window](results/quiet-template-only-r14-rotating/window.json), [raw invalid timings](results/quiet-template-only-r14-rotating/timing.jsonl), [exclusion note](results/quiet-template-only-r14-rotating/INVALID_WINDOW.md).', '',
    'The valid initial controls show repeated-input small-record parsing 9.75% faster; changing-input parsing is 0.13% faster with overlapping ranges. None of the other controls has a separated slowdown. The full run shows small-record parsing 11.30% faster, the 1 KiB object case 12.35% faster and tiny-object parsing 2.06% faster. Perry beats both peers on 46/50 CPU medians and 36/50 peak-RSS medians; those counts do not establish the complete objective.', '',
    'Six full-run cases have separated slowdowns; one additional case is slower in every pair with overlapping ranges. The longer recheck retains three separated slowdowns. Its other four medians remain positive with overlapping ranges, with ten or eleven slower pairs. This candidate does not establish the required absence of regressions.', '']

def append_tables(label, data, slug, diagnostic=False):
    win = data['window']
    lines.extend([f'## {label}', '',
        f"Window {win['started_utc']}–{win['finished_utc']}; one-minute load {win['load_before'][0]:.3f}→{win['load_after'][0]:.3f}. Quiet gate passed; no competing workload detected at either boundary.", ''])
    names = ['Main B', 'Main A', 'Node', 'Bun'] if diagnostic else ['Main', 'R14', 'Node', 'Bun']
    lines.extend(['| Fixture / operation | Iterations | ' + ' CPU | '.join(names) + ' CPU | Delta | Ranges | Slower pairs |',
                  '|---|---:|---:|---:|---:|---:|---:|---|---:|'])
    for c in data['cases']:
        e = c['engines']; op = c.get('mode', c.get('operation'))
        values = ' | '.join(f"{e[n]['cpu_us']:.6f}" for n in ['baseline', 'perry', 'node', 'bun'])
        note = 'separated slowdown' if c.get('separated_regression') else 'separated gain' if c.get('separated_improvement') else 'overlap'
        lines.append(f"| {c['fixture']} / {op} | {e['perry']['iterations']} | {values} | {c['delta_pct']:+.3f}% | {note} | {c['slower_pairs']}/{len(e['perry']['cpu_samples_us'])} |")
    lines.extend(['', '| Fixture / operation | ' + ' peak RSS | '.join(names) + ' peak RSS |', '|---|---:|---:|---:|---:|'])
    for c in data['cases']:
        values = ' | '.join(f"{c['engines'][n]['peak_rss_mib']:.3f}" for n in ['baseline', 'perry', 'node', 'bun'])
        lines.append(f"| {c['fixture']} / {c.get('mode', c.get('operation'))} | {values} |")
    lines.extend(['', f'[Timings](results/{slug}/timing.jsonl), [full-output checks](results/{slug}/verify.jsonl), [host and worker hashes](results/{slug}/host.json), [window](results/{slug}/window.json).', ''])

append_tables('Changing-input and repeated-input controls', rotating, 'quiet-' + w.name + '-retry1-rotating')
append_tables('All 50 parse/stringify/consumption cases', full, 'quiet-' + w.name + '-full')
append_tables('Longer recheck', recheck, 'quiet-' + w.name + '-recheck-full')
lines.extend(['## A/A controls', '',
    'Two cases were selected to investigate a possible measurement bias: 1 MiB object parsing and 20 MiB array scanning, at the same longer work counts and eleven repetitions. Both Perry arms execute the exact frozen main bytes; Node and Bun remain in the interleaved order. The first control uses the same executable path in both arms. The second uses main-worker versus control00-worker, matching the five-byte basename-length difference between main-worker and candidate-worker. Main A is the driver’s perry-labelled arm and Main B its baseline-labelled arm; neither is R14.', '',
    'Same-path deltas are +0.00033% and −0.02928%, with overlapping ranges. Different-name deltas are −0.00281% and +0.00480%, also overlapping. These controls do not support timing-order or executable-name-length bias as the explanation for the candidate slowdowns. Runtime inspection confirms argv strings are allocated, but the proposed timing effect was not observed. Dedicated archives attribute only main’s build and source to these controls.', ''])
append_tables('A/A: same path and bytes', aa, 'quiet-' + w.name + '-aa-main-full', True)
append_tables('A/A: identical bytes, different name lengths', aa_path, 'quiet-' + w.name + '-aa-path-full', True)
lines.extend(['## Correctness and machine code', '',
    '- `RUST_TEST_THREADS=1 cargo test --release -p perry-runtime --lib json`: 292 pass. The new unit checks distinct object/array identities and mutable cache replacement immediately after a hit. The TypeScript fixture mutates earlier results and checks retained outputs across pressure.',
    '- Exact production build: `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static`, 333.28 seconds. Compiler and both archives were frozen and hashed, with all three mtimes after build start. Four generated worker objects match main byte-for-byte and link their corresponding frozen runtime.',
    '- Main and candidate each pass 28 Node behavioral runs across auto/tape/direct parsing and normal/scheduled/full GC, plus 14 stringify-options checks. Every scheduled run asserts positive protected page sets and moved objects. The cache fixture has 2,158 protected sets and 99,893 moved objects under auto/direct; tape has 1,999 and 95,991. Callback-only has 16 and 13,305.',
    '- Native static checking retains seven UNSUPPRESSED findings identical to actual main: five unrooted globals, one unrooted string handle and one stale allocation value. Coverage: 83 functions, 2,226 statepoints, 8,372 relocates. All fourteen IR files match after removing only the first native ModuleID path comment; shadow IR needs no normalization. Both shadow variants pass, and ordinary/callback native controls have zero findings. No allowance was increased; this is not an all-clean static result.',
    '- Script lint: 73/74 pass; public benchmark input freshness fails. Compile tier and two CI-only checks are skipped. The Rust file cap passes. Full CI is not claimed.', '',
    f'[Unit provenance]({v}unit-source.json), [build]({v}build-provenance.json), [main]({v}reference-main.json), [workers]({v}worker-object-comparison.json), [behavior]({v}candidate-fixture-validation.json), [options]({v}candidate-options-validation.json), [root comparison]({v}root-comparison.json), [lint]({v}script-lint.log.gz).', '',
    'Machine code confirms removal of the whole-template memcpy and planned-array copy. The hit helper’s frame shrinks from 912 to 224 bytes, including saved registers. The remaining 64-byte external call is memset_pattern16 initializing the output-value array. parse_slow remains 512 bytes; its DirectParser still occupies sp+0xa0 and calls the same-address constructor and parse_value functions in both builds. That evidence does not support a changed parser stack-slot explanation.', '',
    f'[Borrow audit]({v}borrow-audit.json), [cached code comparison]({v}cached-machine-comparison.json), [main symbols]({v}main-cached-entry-machine.json), [candidate symbols]({v}candidate-cached-entry-machine.json), [parser call-site comparison]({v}parse-slow-construction-call.json), [A/A verifier]({v}analyze-aa.py), [different-name A/A verifier]({v}analyze-aa-path.py).', '',
    '## Remaining scope and next experiment', '',
    'All 24 lazy-array probes preserve main’s outcomes and complete stdout: twelve two-record cases pass Node; at 180 records, six zero/true spacing cases crash with SIGSEGV and two plain whitespace/duplicate-key cases return noncanonical raw JSON. The other four pass. The separate main/Bun versus Node fractional-spacing gap is unchanged. These baseline failures are not conformance passes.', '',
    f'[Lazy main]({v}lazy-main-probes.json), [lazy candidate]({v}lazy-candidate-probes.json), [fraction baseline]({v}fraction-baseline.json), [fraction candidate]({v}candidate-fraction.json).', '',
    'The next minimal experiment should keep template borrowing within the original out-of-line reuse function, explicitly preventing its new admission code from being inlined into parse_slow. It should remove the predicate/construction split while preserving the existing collection and borrow ordering. This tests the changed call boundary; it does not assume it caused the slowdown.', '',
    'A separate later investigation is to construct captured templates directly in their final local struct. Read-only main disassembly shows a 576-byte local-values-to-entry copy followed by a 600-byte entry-to-cache copy, plus copied array plans for root-barrier iteration. Those copies are not removed in R14 and are not proven causes of its regression.', '',
    f'[Capture code]({v}main-template-capture-machine.json), [capture follow-up]({v}capture-followup.md).', '',
    'Access, extended short-call, stringify-options timings, broader rotating coverage and the 36 retained-output cases were not run after full-matrix qualification failed. Prepared drivers are not evidence of execution. The full objective remains open. R14 is parked without a PR or release version bump.', '',
    f'[Rotating verifier]({v}analyze-rotating-retry1.py), [full verifier]({v}analyze-full.py), [recheck verifier]({v}analyze-full-recheck.py), [rotating vectors]({v}rotating-retry1-analysis.json), [full vectors]({v}full-analysis.json), [recheck vectors]({v}full-recheck-analysis.json), [A/A vectors]({v}aa-analysis.json), [different-name A/A vectors]({v}aa-path-analysis.json), [validation manifest]({v}manifest.json). Initial staging verified 102 hashes; different-name control staging verified 103 including the identical-main copy.', '',
    '[R13 report](https://github.com/PerryTS/perry/blob/cc7eca082c2b075f6a0cb8f4760af0d5b63817f8/benchmarks/json_performance/BORROWED_TEMPLATE_R13.md) records the preceding combined experiment. Earlier accepted work landed through [merge train #10037](https://github.com/PerryTS/perry/pull/10037).', ''])
report = '\n'.join(lines)
(bench / 'TEMPLATE_ONLY_R14.md').write_text(report)
(w / 'report-draft.md').write_text(report)
print('Wrote 72 candidate CPU/RSS rows plus four A/A rows; failed window separately preserved.')
