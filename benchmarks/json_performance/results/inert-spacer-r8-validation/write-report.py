from pathlib import Path
import json

w = Path(__file__).resolve().parent
bench = w.parents[1]
a = json.loads((w / 'analysis.json').read_text())
p = json.loads((w / 'provenance.json').read_text())
rows = [c for phase in a['phases'].values() for c in phase['cases']]
regressions = [r for r in rows if r['separated_regression']]
lines = ['# Bounded JSON stringify inert spacers (R8)', '',
         'DECISION_PENDING', '',
         f"Source `{p['source_commit']}` on `codex/json-inert-spacer-r8`, based on freshly built main `{p['base_commit']}` (0.5.1531). R3/R5/R6/R7 production changes are excluded. Four engines ran interleaved on the M1 Mac mini (8 GiB), seven fresh-process trials per row, Node 26.5.1 and Bun 1.3.14.", '',
         'CPU is microseconds per operation. Access rows count one loop iteration after parsing; focus and options rows count a complete operation. Options inputs are eagerly parsed before timing. Peak RSS is MiB for the whole process, including input, runtime, output, and allocator storage; it is not retained heap. Negative CPU deltas are faster. All samples are retained. “Separated” means the seven observed ranges do not overlap, not a confidence interval or proof of causality.', '',
         f"The candidate leads both peers on {sum(r['beats_both_peers'] for r in rows)}/34 CPU medians and {sum(r['rss_beats_both_peers'] for r in rows)}/34 peak RSS medians. These are expanded controls, not the original 38-row matrix.", '']
for phase, title in [('access', 'Access after parsing'), ('focus', 'Parse, stringify, and consumption'), ('options', 'Stringify arguments')]:
    data = a['phases'][phase]
    window = data['window']
    lines += ['## ' + title, '',
              f"Window: {window['started_utc']} to {window['finished_utc']}; one-minute load {window['load_before'][0]:.3f} to {window['load_after'][0]:.3f}. Quiet gate passed, with no detected competing workloads at either boundary.", '',
              '| Workload | Main | R8 | Node | Bun | R8 vs main | Observed ranges |',
              '|---|---:|---:|---:|---:|---:|---|']
    for c in data['cases']:
        e = c['engines']
        values = [f"{e[n]['cpu_us']:.6f}" for n in ['baseline', 'perry', 'node', 'bun']]
        note = 'separated slowdown' if c['separated_regression'] else 'separated gain' if c['separated_improvement'] else 'overlap'
        lines.append('| ' + c['fixture'] + ' / ' + c['operation'] + ' | ' + ' | '.join(values) + f" | {c['delta_pct']:+.2f}% | {note} |")
    lines += ['', 'Peak RSS:', '', '| Workload | Main | R8 | Node | Bun |', '|---|---:|---:|---:|---:|']
    for c in data['cases']:
        e = c['engines']
        values = [f"{e[n]['peak_rss_mib']:.2f}" for n in ['baseline', 'perry', 'node', 'bun']]
        lines.append('| ' + c['fixture'] + ' / ' + c['operation'] + ' | ' + ' | '.join(values) + ' |')
    slug = 'quiet-' + w.name + '-' + phase
    lines += ['', f'[Raw samples](results/{slug}/timing.jsonl), [output verification](results/{slug}/verify.jsonl), [CPU/RSS vectors and medians](results/{slug}/summary.json), [qualified window](results/{slug}/window.json).', '']
lines += ['## Implementation and build evidence', '',
          'The runtime admits primitive true and either sign of numeric zero to four existing bounded output attempts when the replacer is absent. This covers computed spacers as well as literals. Boxed Number/String values retain coercions, and the later lazy-source shortcut keeps its original admission predicate. No GC policy, object representation, callback order, or cache is added or changed.', '',
          'Both revisions were built clean with `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static`, then frozen. Compiler and both static archives have recorded hashes and modification times after build start. All four generated worker objects are byte-identical across main/candidate. Each binary is linked against its corresponding fresh runtime archive. The public stringify entry retains its address, but its stack frame grows from 272 to 288 bytes. LLVM lowers the source zero-bit test to a floating zero comparison; this is not an integer-only machine check.', '',
          '[Build and source provenance](results/inert-spacer-r8-validation/provenance.json), [worker object comparison](results/inert-spacer-r8-validation/worker-object-comparison.json), [machine observations](results/inert-spacer-r8-validation/machine-observations.json).', '',
          '## Validation and limits', '',
          '- 292 runtime JSON unit tests pass on the final source, single-threaded.',
          '- Main and candidate each pass 19 Node output checks: new spacer and prior callback/reentry fixtures across auto/tape/direct parsing and normal/scheduled/full GC, plus a callback-only scheduled control. Every scheduled run asserts positive protected page sets and moved objects. The new spacer fixture records 1,190 protected sets and 44,380/44,455/44,380 moved objects; callback-only records 16 protected sets and 13,305 moved objects.',
          '- Main and candidate each pass 14 options checks: plain, literal zero, computed zero, pretty, key-list and callback small-record cases, plus a 16 KiB pretty array, under normal and scheduled GC.',
          '- Full native IR checking reports nine UNSUPPRESSED hazards across six modules (eight unrooted global values and one stale allocation value). All match actual main fingerprints; native IR is byte-identical after removing only the first ModuleID file-path comment. Shadow IR is byte-identical without normalization, and both shadow checks pass. Native ordinary workers and callback-only controls have zero findings. This is not an all-clean static result.',
          '- Script lint: 73/74 pass; public benchmark evidence freshness fails. Compile tier and two CI-only checks are skipped. Rust file cap passes (replacer.rs: 1,985 lines). Full CI is not claimed.', '',
          '[Behavioral validation](results/inert-spacer-r8-validation/candidate-fixture-validation.json), [options validation](results/inert-spacer-r8-validation/candidate-options-validation.json), [root comparison and fingerprints](results/inert-spacer-r8-validation/root-comparison.json), [lint log](results/inert-spacer-r8-validation/script-lint.log.gz).', '',
          '### Existing baseline correctness gaps', '',
          'A dedicated 24-case lazy-array probe runs each case in a separate process. All twelve two-record cases match Node. For 180-record arrays, plain canonical input matches, while plain whitespace and duplicate-key input return noncanonical raw JSON; all six numeric-zero/true cases crash with SIGSEGV; the three pretty cases match Node. Candidate outcomes and complete stdout match main for all 24 cases, including the failures. This preserves baseline behavior and is not a conformance pass. Widening the lazy shortcut would spread its raw-output mismatch, so the initial wider draft was rejected before performance measurement.', '',
          'A separate fractional-spacing probe records another baseline difference: main and local Bun emit compact output for positive fractions below one, while pinned Node emits newlines without indentation. The candidate exactly preserves main output. This case is kept separate from passing Node checks, with the original discovery and output retained; no specification conclusion is asserted.', '',
          '[Lazy main probes](results/inert-spacer-r8-validation/lazy-main-probes.json), [candidate probes](results/inert-spacer-r8-validation/lazy-candidate-probes.json), [scope decision](results/inert-spacer-r8-validation/lazy-scope-decision.md), [fraction baseline](results/inert-spacer-r8-validation/fraction-baseline.json), [candidate fraction check](results/inert-spacer-r8-validation/candidate-fraction.json).', '',
          f"All {a['trials']} timed checksums, CPU/RSS sample vectors, medians, source-patch hashes, recorded local source/worker hashes, and quiet windows were independently checked by [analyze.py](results/inert-spacer-r8-validation/analyze.py). Full output hashes are checked for focus/options; access checks the complete workload checksum.", '',
          'The full original 38-row matrix, broader consumption, rotating-source, short-call, and retained-memory suites have not been rerun for this experiment. They remain requirements for the overall performance objective. Earlier main results are in [merged-main measurements](https://github.com/PerryTS/perry/blob/4fc51192eddce6efeb11fb70c5c10ba3c25978cc/benchmarks/json_performance/MERGED_MAIN_53DF.md). The no-regressions objective remains open.', '']
(w / 'report-draft.md').write_text('\n'.join(lines))
print('Wrote', w / 'report-draft.md')
