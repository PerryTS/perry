# Tape depth admission R2 validation

Source/build `df64624c8c7b08ca09e78f0006468f3d0befed67`, package 0.5.1530.
All 291 JSON release tests pass single-threaded (4m40s compilation, 1.60s tests).
The matched compiler/runtime-static/stdlib-static release build passes in 5m20s.
HEAD and source hashes were fixed through both builds. R2 adds `inline(never)`
to the native builder; all [R1 depth-admission semantics and tests](../tape-depth-r1-validation/README.md)
remain unchanged. No collector-policy change is made.

LLVM emits separate 10396/10764-byte tape builders without/with depth capture.
`parse_slow` falls from R1's 13784-byte span and 560-byte stack allocation to
5624/512 bytes (opening-scan R3 was 5416/496; main was 7232/496). The deep
iterative helper has a 1472-byte symbol span. Full disassemblies and the R1
comparison are archived; these code changes do not establish a timing win.

All nine compiled auto/tape/direct x normal/scheduled/full-GC depth witnesses
match Node. Each scheduled arm protects 2772 retired sets and moves more than
46000 objects. Existing protected scan: 1323 copying minors, 208531 moved
objects, correct output. Retained outputs pass normal, scheduled-moving and
full-GC runs. Unicode's 1000-parse diagnostic performs 26 minors. The recurring
stringify malloc counts are 39,40,40,40 for both candidate and fresh main.
Fourteen compiled escaped-record comparisons pass; twelve fail on the unfixed
main negative control. Raw/address/root/store/fmt/file-size gates pass.
Human-audited store classes are not claimed as machine-verified.

The [focused replay](../quiet-tape-depth-r2-main-focus-r9-r27/README.md) passes
48 output checks and 378 timing trials. Array parse improves 24–25% versus main,
while Unicode same-source parse retains a +0.249% concern (8/9 pairs slower).
Full original and changing-input evaluation is pending. R1 measurements must
not be relabelled as R2 results. Local drivers are archived
as historical records with local paths, alongside logs, source patch and hashes.
