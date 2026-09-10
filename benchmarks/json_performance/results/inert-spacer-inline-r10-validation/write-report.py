from pathlib import Path
import json

w = Path(__file__).resolve().parent
bench = w.parents[1]
a = json.loads((w / 'full-analysis.json').read_text())
r = json.loads((w / 'full-recheck-analysis.json').read_text())
p = json.loads((w / 'provenance.json').read_text())
v = 'results/inert-spacer-inline-r10-validation/'
lines = ['# JSON stringify bounded-writer inlining (R10)', '',
    '**Rejected for landing.** The longer recheck confirms object parsing +0.78%, large-array stringify +0.80%, and numeric-array stringify +0.70% versus actual main, with separated sample ranges and all 11 paired repetitions slower. Zero-spacing stringify is roughly 9.5× faster than main, but this does not satisfy the no-regression requirement.', '',
    f"Measured source `{p['source_commit']}` on `codex/json-inert-spacer-inline-r10`, based on main `{p['base_commit']}` (0.5.1531). The exact fresh-main build from R8 is reused by verified hashes; main workers, fixtures and IR were recompiled at R10 paths. This does not measure a newer main revision.", '',
    'The candidate retains R9’s separate helper for primitive true and numeric ±0 spacers, and changes three existing bounded-writer annotations to `#[inline(always)]`. It retains original fallback arguments and lazy-source admission. GC policy, representation and callback semantics are unchanged. R3/R5/R6/R7/R8 production changes are excluded.', '',
    'Four engines: main, R10, Node 26.5.1, Bun 1.3.14 on the quiet M1 Mac mini with 8 GiB RAM. The initial 34 controls and original 50-row matrix use seven interleaved fresh-process repetitions. The five-case recheck uses 11 repetitions and four times each corresponding full-matrix iteration count, retaining the same warmup. All 2,572 timed checksums, full-output verification hashes, CPU/RSS vectors and medians, source/worker/input hashes, patches and five terminal quiet windows were independently verified. All samples and outliers are retained.', '',
    'CPU values below are microseconds per operation. Negative deltas are faster. “Separated” means observed sample ranges do not overlap; it is not a confidence interval or a causal attribution. RSS values are whole-process peak MiB, including input, runtime, output and allocator storage; they are not retained heap. Small RSS differences do not support a general memory claim.', '',
    'The original 38 parse/stringify rows plus 12 consumption rows give R10 46/50 CPU wins over both peers and 36/50 peak-RSS wins. The remaining CPU gaps are 16 KiB, 1 MiB and 8 MiB scans and the 20 MiB array round trip. The overall performance goal remains open.', '',
    'The first 34 controls have no separated slowdowns. The full matrix exposes five. Rechecking those five confirms three, clears 16 KiB sparse access (−0.01%, overlapping ranges), and leaves 20 MiB object stringify +0.32% with overlapping ranges but 10/11 pairs slower. The latter is not evidence of no regression. The focus long-string stringify +3.32% and full-matrix −0.73% both have overlapping ranges and different work counts; they are not interchangeable A/B measurements.', '']

phases = [(k, val, False) for k, val in a['phases'].items()] + [('full', r['phases']['full'], True)]
for kind, phase, recheck in phases:
    slug = 'quiet-' + w.name + ('-recheck-' if recheck else '-') + kind
    timings = [json.loads(x) for x in (bench / 'results' / slug / 'timing.jsonl').read_text().splitlines()]
    counts = {(x['fixture'], x['operation']): x['iterations'] for x in timings}
    win = phase['window']
    lines += [f"## {kind.capitalize()}{' longer recheck' if recheck else ''}", '',
        f"{phase['timed_trials']} trials. Quiet window {win['started_utc']}–{win['finished_utc']}; one-minute load {win['load_before'][0]:.3f}→{win['load_after'][0]:.3f}. Quiet gate passed, with no competing workload detected at either boundary; terminal window archived before the next remote operation.", '',
        '| Workload | Iterations | Main CPU | R10 CPU | Node CPU | Bun CPU | R10 vs main | Ranges | Slower pairs |',
        '|---|---:|---:|---:|---:|---:|---:|---|---:|']
    for c in phase['cases']:
        e = c['engines']
        vals = ' | '.join(f"{e[n]['cpu_us']:.6f}" for n in ['baseline', 'perry', 'node', 'bun'])
        note = 'separated slowdown' if c['separated_regression'] else 'separated gain' if c['separated_improvement'] else 'overlap'
        reps = len(e['perry']['cpu_samples_us'])
        lines += [f"| {c['fixture']} / {c['operation']} | {counts[c['fixture'], c['operation']]} | {vals} | {c['delta_pct']:+.2f}% | {note} | {c['slower_pairs']}/{reps} |"]
    lines += ['', '| Workload | Main peak RSS | R10 peak RSS | Node peak RSS | Bun peak RSS |', '|---|---:|---:|---:|---:|']
    for c in phase['cases']:
        vals = ' | '.join(f"{c['engines'][n]['peak_rss_mib']:.3f}" for n in ['baseline', 'perry', 'node', 'bun'])
        lines += [f"| {c['fixture']} / {c['operation']} | {vals} |"]
    lines += ['', f'[Timed samples](results/{slug}/timing.jsonl), [full-output verification](results/{slug}/verify.jsonl), [sample vectors and medians](results/{slug}/summary.json), [quiet window](results/{slug}/window.json).', '']

lines += ['## Machine code and validation', '',
    '- The three writers are again inlined in the public full entry. Its frame grows from main’s 272 to 336 bytes, and the new helper has a 128-byte frame. Both public entries are 64-byte aligned. These observations do not establish the cause of the broad slowdowns.',
    '- Exact production build: `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static`, from frozen clean source, 326.68 seconds. All three artifact mtimes are after build start; compiler/runtime/stdlib hashes are recorded. All four main/candidate generated worker objects match byte-for-byte and link the corresponding frozen runtime. Sixty remote hashes, including all 19 fixture inputs, were verified.',
    '- `RUST_TEST_THREADS=1 cargo test --release -p perry-runtime --lib json`: 292 tests pass on final source.',
    '- Main and candidate each pass 19 Node fixture checks across auto/tape/direct parsing and normal/scheduled/full GC, including callback-only pressure; each passes 14 options checks. All scheduled runs assert positive protected page sets and movement. The new spacer fixture has 1,190 protected sets and 44,380/44,455/44,380 moved objects; callback-only has 16 sets and 13,305 moved objects.',
    '- Full native static checking retains nine UNSUPPRESSED findings: eight unrooted global values and one stale allocation value. All match actual main fingerprints. Twelve IR files match after removing only the first native ModuleID path comment; shadow IR needs no normalization. Both shadow variants pass. Native ordinary and callback-only controls have zero findings. No allowlist or stale-value allowance was increased; this is not an all-clean static result.',
    '- Script lint: 73/74 pass; public benchmark evidence freshness fails. Compile tier and two CI-only checks are skipped. The Rust file cap passes. Full CI is not claimed.', '',
    f'[Build provenance]({v}build-provenance.json), [reference main]({v}reference-main.json), [worker objects]({v}worker-object-comparison.json), [machine observations]({v}machine-observations.json), [behavioral checks]({v}candidate-fixture-validation.json), [options checks]({v}candidate-options-validation.json), [root comparison]({v}root-comparison.json), [lint log]({v}script-lint.log.gz).', '',
    '## Known limitations and remaining work', '',
    'All 24 isolated lazy-array probes preserve actual main outcomes and complete stdout. All twelve two-record cases match Node. At 180 records, plain canonical input passes, plain whitespace and duplicate-key inputs return noncanonical raw JSON, six zero/true spacing cases crash with SIGSEGV, and three pretty cases pass. Preserving baseline failures is not conformance. Normalizing zero to undefined would widen the faulty raw-source admission, so fallback retains the original spacer.', '',
    'Fractional spacing preserves the recorded main/Bun versus Node difference for positive values below one. It is an explicit baseline gap, excluded from passing Node checks.', '',
    f'[Lazy baseline]({v}lazy-main-probes.json), [candidate lazy probes]({v}lazy-candidate-probes.json), [fraction baseline]({v}fraction-baseline.json), [candidate fraction probe]({v}candidate-fraction.json).', '',
    f'[Verifier]({v}analyze.py) was run with `--with-full` and `--full-recheck`. [Validation manifest]({v}manifest.json) records every archived artifact and its original and stored hashes. Retained-memory, rotating-input and short-call drivers were prepared but not executed on this rejected candidate. They remain required for a future candidate; their presence in the archive is not a timing or validation claim.', '',
    '[R9 report](https://github.com/PerryTS/perry/blob/04fc70bbc1194d32ec8bc4d4e1caca5340797567/benchmarks/json_performance/INERT_SPACER_TAIL_R9.md) records the preceding rejected experiment. No PR is opened for R10 and no version bump is made.', '']
(bench / 'INERT_SPACER_INLINE_R10.md').write_text('\n'.join(lines))
(w / 'report-draft.md').write_text('\n'.join(lines))
print('Wrote all 89 CPU and peak-RSS rows, five windows, validation and limitations.')
