# Consistent escaped JSON records

PR #10032, incorporated into main by #10033 at `e7223f700`, used the tape decoder for an isolated lazy-array read and
the batched direct parser for consecutive reads. The old tape decoder discarded
unmatched surrogate escapes. A 128-record input containing `"s":"\ud800"`
therefore produced an empty first value and preserved subsequent batched values.
Node preserves all of them.

The correction keeps the unescaped tape scan and delegates escaped tokens to the
canonical direct decoder. Decoded strings use the same lone-surrogate guard and
builder constructor as eager JSON parsing. Sparse reads, consecutive reads,
iterative materialization and forced cache merges now agree on those values.

New coverage exposed one further stringify defect: an unmatched surrogate in a
property name became a synthetic `field1` name when an object contained nested
values. The UTF-8 key path now falls back to the existing byte escaper for
validated WTF-8. Validation finishes before output is changed; malformed bytes
keep the prior fallback. The normal UTF-8 key path is unchanged in source.

The batch's collection test uses the producer-specific completed-record hook
landed in #10033. It cannot pass by falling back to the tape producer. This
correction preserves that hook and its rewritable root unchanged.

No new collection point, GC policy, retained input view or managed scratch
allocation is introduced by the correction. The byte-key writer only grows its
existing Rust output buffer. Escaped decoding retains its temporary Rust Vec.

## R1 validation and performance

- All **283 JSON unit tests pass**, including seven Node-checked escaped-record
  cases, isolated/ascending/descending reads, the forced-materialization path,
  UTF-16 length and lone-surrogate flags, and malformed-key rejection before
  output mutation.
- The producer-specific moving-GC witness passes.
- Raw-handle, address-class and runtime-root checks pass without new exceptions.
- The matched release build passes. All 14 compiled escaped-record comparisons
  match Node; 12 of them fail on the old binary. Scheduled moving/protected GC,
  full GC, retained-output lifetime and malloc-sweep cadence witnesses pass.
  See [validation evidence](results/pr10032-decoder-validation/README.md).
- The full original matrix passed 200 output comparisons and 344 measurement
  groups. Perry led 46/50 CPU rows, 67/86 peak-RSS rows and 31/36 retained-current
  RSS rows, the same counts as the earlier build. The changing-input matrix
  passed 380 comparisons and 1140 timing trials. Both windows passed quiet
  admission. These are R1 measurements against the original PR head, not main.
- R1 is **not accepted as regression-free**: the small-array sparse-read
  slowdown repeated in longer paired trials, reaching +2.65%. The smaller
  escaped-parse and ASCII-stringify shifts did not repeat. See the
  [paired-trial disposition](results/quiet-pr10032-decoder-r1-focus/DISPOSITION.md).

## Follow-up on merged main

Main `e7223f700` has a freshly built, matched release compiler and static runtime
and stdlib. Its workers are pinned independently from the earlier PR binaries.
R2 (0.5.1529) keeps `parse_string_bytes_static` out of line, restoring the scanner
call boundary removed by compiler inlining after the duplicate escape decoder
was deleted. The shared canonical decoder and correctness checks remain.
R2 passes its own 283 JSON tests, matched release build, 14 compiled Node
comparisons and live GC/retained-output/cadence witnesses. See the
[rebuilt validation evidence](results/decoder-r2-validation/README.md).

The [longer paired comparison](results/quiet-decoder-r2-focus-r9/README.md)
passes 18 output comparisons and 162 timing trials. Sparse reads are +0.063%
against main and -2.38% against R1, with overlapping main samples. The other
parse/scan cases are within +0.184% and overlap too. RSS differs by at most
48 KiB.

Both full matrices now pass quiet admission and output validation against the
fresh main build, Node and Bun. The [original suite](results/quiet-decoder-r2-main-all-r5/README.md)
validates 200 output checks and 344 measurement groups. Its largest slower CPU
median is +0.315%, with overlapping samples; RSS differences are at most 80 KiB.
The [changing-input suite](results/quiet-decoder-r2-main-rotating-r5/README.md)
validates 380 comparisons and 1140 trials. All CPU sample ranges overlap; the
largest slower parse median is +0.660% for a same-source control and +0.377%
for changing input. Selection-only controls remain separate. Peak RSS does not
increase in that suite; current RSS differences are at most 16 KiB.

These checks find no repeatable slowdown in the measured matrices. They do not
establish parity on every workload: [fresh merged-main results](MERGED_MAIN_E722.md)
still show small-object, large-string, full-scan and memory gaps against Node/Bun.

The independent source-length experiment is on
`codex/json-source-length` at `91030c9d3`; it is not included in this correction.
