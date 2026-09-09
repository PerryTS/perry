# Tape depth admission R1 validation

Source and matched release build: `f55569a565581fc964a73b063213774b4601d4e1`,
package 0.5.1530. All 291 JSON release tests passed single-threaded in 1.61s
following 4m41s compilation. The matched compiler/runtime-static/stdlib-static
build passed in 5m21s. Source hashes and HEAD stayed fixed across both builds.

Eligible arrays obtain shallow/deep admission from the native tape stack.
Deep valid trees materialize iteratively; rejected tapes retain the legacy
nesting/error fallback before recursion. Direct parses and forced oversized
inputs retain their preflight. The 1000/500000 limits are unchanged. Native
metadata introduces no managed roots; collection-capable callbacks re-read the
source from its existing root. This does not change global collector policy.

Six new Rust tests cover exact structural depth (including empty and quoted
containers), budget rejection and scratch reuse, 10000-level trees on a 2 MiB
worker stack, lazy/eager handoff at the recursive limit, force-on object roots,
and syntax/range error precedence and root cleanup across three parse entries.
The compiled depth witness matches Node in all nine auto/tape/direct x
normal/scheduled/full-GC combinations. Each scheduled arm protects 2772 retired
sets and moves more than 46000 objects. Normal depth arms execute zero copying
minors and are not presented as moving-GC coverage.

The existing protected scan performs 1323 copying minors, moves 208531 objects,
and matches Node; retained outputs pass normal/scheduled/full-GC comparisons.
The 1000-parse Unicode diagnostic performs 26 minors. Stringify's recurring
malloc counts remain 39,40,40,40 in candidate and freshly built main. Fourteen
compiled escaped-record comparisons pass; twelve fail on the unfixed-main
negative control. Raw-handle/address/root/store/format/file-size gates pass;
human-audited store classes are not claimed as mechanically verified.

LLVM inlines the specialized tape builders. `parse_slow` has a 13784-byte symbol
span versus R3's 5416 and main's 7232; its stack allocation grows from 496 to
560 bytes. The outlined deep helper remains present, and two conditional
nesting-check calls remain for direct admission and tape rejection. This is a
real code-generation change, not the compiled-away opening-scan R4 experiment.
Code size is a measurement concern; it does not establish a speedup.

The [focused replay](../quiet-tape-depth-r1-main-focus-r9/README.md) shows 15–17% faster array parsing, 12% faster sparse access and 4% faster full scan versus main. Unicode stringify has a +7.0% median concern (7/9 paired repetitions slower). Full CPU/RSS evaluation is pending. No no-regression or parity claim is made.
Exact local drivers, outputs, diagnostics, source patch and hashes are archived.
Driver paths are historical and must be adapted when replayed elsewhere.
