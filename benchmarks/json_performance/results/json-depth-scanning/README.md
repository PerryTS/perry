# ARM64 block scanning for JSON nesting

The selected depth22 implementation reduces escaped 1 MB parse CPU from
**1.587 to 1.148 ms (27.67% less CPU)** in the qualified comparison.
Node measured 1.734 ms and Bun 2.083 ms. Perry's selected peak RSS is
57.047 MiB, versus Node 74.094 MiB and Bun 64.172 MiB.
These comparisons all use the same fresh measurement window. The earlier
1.46 ms observation came from a rejected window and is not the basis for this
result.

**This remains experimental.** The selected implementation is at or below
both engines on **16/38 CPU rows** and **32/38 peak-RSS rows**. Small RSS
increases remain in many comparisons with the preceding Perry build. The
older full CPU/RSS inventory and remaining semantic gaps are still open.

[All 38 CPU and RSS rows](launch26/results/recheck/all-38.md) ·
[Observed ranges](launch26/results/recheck/summary.json) ·
[Retained memory](launch26/results/recheck/memory-summary.json) ·
[Work including final GC](launch26/results/lifetimes/summary.json)

## Source and selection

- **Selected: depth22**, `ae9283ca3276a983be0ac38c067b4a7bd56337d3`, on
  `codex/json-depth-blocks-1520`.
- Depth23, `f5f54a766dcacddaaa75ecab4f3daef8ee548b8c`, is preserved on
  `codex/json-depth-admission-1520`.
- The preceding escaped decoder is decode18,
  `b3367bd932f78c7e8f64a283b82cdb756c758155`.

Version remains 0.5.1520. These branches are local. The original GC branch
and draft PR 9849 are unchanged.

Depth23 delays the ordinary bulk helper until 128 bytes, rather than 16.
It offers little benefit on the target escaped row. It raises main-process
peak RSS by approximately 64–80 KiB versus depth22 on most rows, and empty-object
parse is 1.071% slower than the preceding decoder with separated observed
ranges. Depth22 has no main CPU row with separated slower ranges in this
window; that observation does not establish the absence of smaller regressions.

## What changed

The nesting preflight previously restarted a quote/backslash search at each
escape. The new ARM64 helper classifies 16 bytes together, tracking odd-length
backslash runs across block boundaries. After 128 bytes in an escaped quoted
span, the preflight switches to this classifier. Ordinary runs use a separate
helper that checks four 16-byte loads together; short tails retain the existing
scanner. Other architectures keep their existing path and were not measured.

Every load is bounded by the complete 16- or 64-byte remaining length.
Comparison results occupy four-bit lanes. Adding one at the start of a run
carries to its first non-backslash lane; opposite start/end parity identifies
an odd-length run. Overflow from an odd-position start carries the escaped-byte
state into the next block. An incoming escape consumes byte zero before it can
start another run.

Only unescaped quotes end a span. Controls, invalid escapes, arbitrary bytes
and brackets inside strings retain the preflight's prior behavior. The actual
parser remains responsible for syntax validation.

The helper allocates nothing, owns no managed values and calls no GC functions.
It reads the rooted input and returns an offset. Construction batching, GC
thresholds, deferred collection and output rooting are unchanged. Stringify
code is unchanged and is included in the regression matrix.

## CPU and memory limits

The selected escaped parse also improves by 27.16–27.24% in discard, latest-output
and retained-output lifetime trials that include final collection. The
24-iteration large-object roundtrip changes by +0.03%. Small-record stringify
lifetime medians change by -0.51%, +0.43% and +0.82%; their observed ranges overlap.

Long ordinary-string parse improves by 3.65%, and Unicode-string parse by
3.58%. Small-record stringify remains about 2.6 times Node's CPU cost. General
object and string workloads still contain substantial gaps to Bun and Node.

Main-process peak RSS changes versus the preceding decoder range from
-0.703 to +0.078 MiB. Across 40 retained-memory groups, selected median peak RSS
changes by -16 to +80 KiB; 37 groups increase. Current RSS after the operation
changes by -32 to +80 KiB. Lifetime peak RSS increases range from 96 to 208 KiB.
These are whole-process measurements. An allocation-free helper does not
imply an unchanged process RSS. No-regression acceptance remains open.

## Measurement and the launch-method control

The qualified run uses the Apple M1 / 8 GiB benchmark Mac, Node 26.5.1 and
Bun 1.3.14. Runtime and stdlib archives were rebuilt together with the
shipping-equivalent release profile: thin LTO, runtime/stdlib codegen-units 1,
wrapper codegen-units 16. Application object files are unchanged. Per-arm
provenance files pin sources, archives, profiles and link commands.

The first depth22 window completed but failed its exit load limit
(3.844 versus 2.5); XProtect used 68.8% CPU in the exit snapshot. The six-engine
depth23 repeat stayed below the load limit but failed the added XProtect check
(46.2% CPU at exit). XProtect exceeded 5% CPU in 163/185 observations. Both full
windows are preserved as **unqualified** evidence under their respective arms.

A native `execv` launcher now runs immutable executable files while keeping
Perry's `argv[0]` fixed at
`/Users/perry/json-escape-runtime-v18-20260907-codex/active`. Fixture paths and
working directory also use the preceding window's canonical directory.
No executable pathname is replaced between trials. The argument probe confirms
Perry sees the same `process.argv[0]` and `process.execPath`.

The same-binary launch25 control contains 84 randomized ABBA timings and four
separate traced processes. Each trace records 37 GC cycles, 28 full collections
and 36,210,217 pointer slots. Launcher CPU medians differ by +0.47% for retained
small-record stringify, -0.07% for escaped parsing and -0.07% for large
roundtrips; ranges overlap. One launcher small-record sample reached 70 ms
against a 44 ms median. Big-case median peak RSS is equal; small-record peak
RSS is 16 KiB lower. All 26 control observations have XProtect below 5% CPU.
This supports the method change without proving the cause of the earlier scans.

The final launch26 window contains **228 output checks, 1,140 timing trials,
480 retained-memory trials and 84 lifetime trials**. Its entry/exit gates pass
(load 1.285 → 2.376), with 79 clean workload observations and no recorded
XProtect activity above 5% CPU. The shared lock serializes work. Process
monitoring and load gates limit interference but cannot eliminate all machine
noise. No system security settings were changed.

CPU is loop user plus system time per call; peak RSS includes process
initialization. Lifetime CPU includes final cleanup, and retained outputs are
checked after collection. Traces are separate from timing samples. Every
Perry trial verifies the immutable source binary's hash. No results from the
rejected windows are pooled into the qualified comparison.

## Correctness

Both depth22 and depth23 pass **3,286 runtime tests** (four ignored), all
**40 compiled Node comparisons**, and **52 seeded moving-GC cases** plus controls.
Each seeded case exercises collections, copying minors, moved objects and loop
polls. The inherited root-array `Object.prototype.toJSON` discrepancy is
explicitly checked against its reference behavior rather than counted as Node
parity. Saved output and counters are in the per-arm validation directories.

New tests exhaust all 65,536 backslash masks with both incoming states,
compare scalar quote/depth oracles on truncations and 30,000 arbitrary inputs,
and place ordinary/backslash runs against a protected allocation boundary.

Root-holder and Node-version inventories pass. Existing file-size failures in
`inprocess.rs` and `hir/lower/tests.rs`, and the existing address-classification
findings, are unchanged. No GC root holder or environment knob is added.

## Prototypes and next work

Depth19's broad quoted-span replacement improved escaped scans but regressed
ordinary strings and short escaped arrays. Depth20's escaped-span cutoff fixed
most of the latter regression. Depth21's inline wider loop regressed local
record scans; two remote attempts were rejected by another benchmark's lock.
Depth22's outlined helper improved the qualified isolated escaped depth check
by 70.24%, ordinary input by 28.45% and Unicode input by 29.19%. These are
preflight-only timings and are not whole-parse speedups. Reference throughput
also shifted across standalone binaries, so isolated record/tiny deltas are
not used to claim runtime gains.

A further full structural scanner, depth24, passes four expanded release tests
and looks promising in local preflight-only measurements. It is not integrated
into the runtime. Stringify's string-only key plans remain the next separate
investigation. [Design and limits](next-investigations.md).

Run `python3 benchmarks/json_performance/results/json-depth-scanning/verify.py`
to verify the saved source identities, measurements and validation evidence.
