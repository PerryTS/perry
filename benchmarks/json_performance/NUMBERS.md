# Number formatting and the full parity target

Follow-up: [bounded parse-shape retention](CACHE.md) reduces the sustained
wide-object cost identified below. This document preserves the number-only run.

The active target is both parse and stringify, every measured input, CPU and
RSS, with no correctness or performance regressions. It is **not achieved**.
The [per-row inventory](results/numbers/parity.md) reports every remaining
gap rather than averaging together wins and losses. Only 5 of 38 CPU medians
currently meet the better Node/Bun median. RSS meets that target in 58 of 74
peak measurements and 28 of 36 retained-output measurements. These counts
are an inventory, not statistical proof of parity or regression.

## Stack-buffer number formatting

The JSON scalar writer and specialized number entry now use `ryu-js` 1.0.3
for non-integer shortest-round-trip output. The integer `itoa` path stays in
place. The formatter writes into stack storage instead of allocating a Rust
String per number; it also follows ECMAScript's shortest-digit tie rule.
`ryu-js` was already in the workspace lockfile through SWC; this names it as
a direct runtime dependency. Its [upstream implementation](https://github.com/boa-dev/ryu-js)
targets ECMAScript number-to-string spelling.

The [fresh quiet-M1 run](results/numbers/tables.md) compares the first fast-path
patch (baseline arm) with this formatter (Perry arm), using the same generated
application object and the same runtime build settings. Numeric-array
stringify improves from **7.908 to 6.583 ms (1.20×)**. Node takes 1.933 ms and
Bun 2.954 ms, so Perry still takes 3.41× and 2.23× their CPU respectively.
Most other rows move by about 1% or less; this run does not establish the
requested absence of all small regressions. Numeric stringify peak RSS is
60.48 → 61.55 MiB. Retained-output RSS is generally within about 0.1 MiB of
the reference. The remaining memory differences must also be resolved.

The updated [numeric profile](results/numbers/profiles/numbers_1m-stringify.txt)
still spends most samples in `ryu-js` digit generation, followed by copies
and traversal. Removing allocation alone is insufficient; compact decimal
fast paths and emitting directly into the result buffer remain candidates.

## Correctness and allocation checks

`python3 check_numbers.py` runs the same deterministic corpus generator in
Node 26.5.1 and Bun 1.3.14 and requires identical oracle files. It extracts
the actual `write_number` function into an optimized leaf harness, supplies
exact double bits (bypassing decimal parsing), and checks every output byte.
A counting allocator checks that each call into a pre-reserved output makes
zero allocations. BigInt callbacks are outside this finite-number leaf test;
the compiled/runtime tests exercise the full integration separately.

All **1,050,129** corpus entries match both engines, including all binary
exponents, representative significands, decimal-boundary neighbors and one
million deterministic random bit patterns. The new formatter also fixes
existing last-digit errors, for example:

| Exact value | Previous Perry JSON | Node, Bun and new Perry JSON |
|---|---|---|
| 562949953421312.25 | 562949953421312.3 | 562949953421312.2 |
| 1125899906842624.25 | 1125899906842624.3 | 1125899906842624.2 |
| 100000000000000.125 | 100000000000000.13 | 100000000000000.12 |

Validation: 46 runtime JSON tests; 22 compiled fixtures matching Node;
152 benchmark output comparisons; 456 timing and 432 memory trials.
GC stress seeds 17/9013 pass with from-space protection and evacuation
verification, including 9/11 copying collections and 19,278/19,101 moved
objects. GC sources and `parse_api.rs` remain unchanged. The [validation
record](results/numbers/validation.json), [source/archive provenance](results/numbers/provenance.json)
and [quiet-window record](results/numbers/window.json) preserve the evidence.

## Sustained wide-object parsing changes the next priority

The first comparison used only 3 timed parses of the 50,000-key object,
because calibration also included the much slower original parser. It
reported 10.8 ms per patched parse. With both faster Perry arms, calibration
selects 36 timed calls: **both arms now take about 44.4 ms per parse** and
reach about 227 MiB peak RSS. This is an existing repeated-call cost, not a
number-formatter regression. Comparisons across runs must preserve iteration
counts or explicitly separate short runs from sustained throughput.

The [sustained wide-object profile](results/numbers/profiles/wide_1m-parse.txt)
places 2,708 of 2,991 samples under the loop's moving-GC safepoint. JSON's
retained parse-shape cache accounts for a substantial root-scanning subtree.
Inspection shows the cache bounds its entry count (256), but not the number
of retained keys. Meanwhile the separate parse-key cache is cleared above
4,096 keys: repeated very wide shapes get new key pointers, miss the
pointer-identity shape cache, and leave previous key graphs retained there.
Reducing this retained parser metadata is now a priority. Collection should
continue to trace live input/output; changing thresholds would conceal the
retention problem.

The independent Unicode-counter prototype also matches every Unicode scalar
value and boundary-prefix tests. Its NEON loop is much faster than the current
UTF-16 iterator in a local leaf experiment; it is not yet part of this runtime
or the official matrix. Integrate and validate it separately before claiming
an end-to-end Unicode gain.
