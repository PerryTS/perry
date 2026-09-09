# Next parse investigations

The [opening-scan and entry-frame experiment](OPENING_SCAN.md) now has complete R3 original and changing-input comparisons against [freshly built merged main 0.5.1529](MERGED_MAIN_EEE.md), with persistent sparse/heterogeneous CPU concerns in the longer replay. The [large-container memory diagnosis](GC_MEMORY_GROWTH.md) has separate default/full-GC measurements.

These are hypotheses, not accepted speedups. Bounded lazy-record construction
and preserved large-output sweep requests landed through #10033 at `e7223f700`.
The escaped-record correction landed through #10035 at `eee3881c4`; the opening-scan follow-up retains that corrected behavior.

Fresh [merged-main stack samples](results/main-e722-location-profiles/README.md)
confirm the same broad targets. The Unicode object has 1522 main-thread samples:
607 at UTF-16 counting, 353 at nesting preflight, 322 at token scanning and 158
at copying. The small-record sample includes startup and supports location
diagnosis only; its main-loop stack still shows generic parsing, pressure and
occupancy queries, template bookkeeping, and string construction.

## Large Unicode strings

A two-second R4 rotating-input stack sample has 1,565 main-thread samples:
672 at UTF-16 counting, 343 at string-token scanning, 220 at nesting preflight,
and 143 at copying. The workload starts before sampling and is intentionally
terminated afterwards. The development host is compiling concurrently, so these
are location diagnostics, not quantitative acceptance timings.

The source StringHeader already has its UTF-16 length. For a dominant unescaped
string token with a bounded ASCII prefix and suffix, its length may be derivable
by subtracting those outside bytes. The proof must require the exact rooted
source/input range, checked subtraction, and a token boundary that preserves
the existing malformed/WTF-8 counting behavior. Standalone byte parsers and
other shapes need the current counter. No source/output pointer may survive an
allocation without its existing root/suppression protection. This could remove
an entire large counting pass without retaining more strings.

## Small records with changing input

The small-record profile shows material time in pressure checks, arena occupancy
walks, and parse-completion bookkeeping. It also shows generic object parsing,
per-field TLS access, and representation/shape cleanup. The existing same-source
and tiny-value entries already amortize some boundary accounting, while a cold
small record enters the full parse boundary.

Investigate whether bounded, callback-free small parses can use the same
construction scope and pressure cadence as bounded lazy record construction.
Retain pending-work servicing, full-GC behavior, active-cycle handling, error
semantics, and result rooting. Size bounds must make deferred pressure safe;
this must not be implemented as an unconditional GC bypass. Do not change
object representation cleanup without proving address-reuse correctness.

[Compressed stack samples and binary provenance](results/output-debt-r4-profiles/provenance.json).

## Merged baseline

Upstream `d342c816be56c8f8b011144939a66674d1a42027` retains reusable parse
template ShapeId ownership during a full trace. It changes `json/mod.rs`,
`json/parse_reuse.rs`, and a GC regression test, and is integrated into the
measured candidate. The later `f2dc03582` added release metadata only. That
follow-up landed as 0.5.1528; the escaped-record correction is based on the actual
`e7223f700` merge. Existing trial artifacts remain measurements of their
recorded revisions and must not be relabelled as measurements of a later build.

## Untouched lazy-array roundtrip

`try_stringify_lazy_array` currently canonicalizes number spellings, then makes
a fresh managed string. If normalization returns a borrowed whole-input span,
returning the rooted original immutable string may avoid the final copy and
UTF-16 recount. Require the entire original string, rather than retaining a
large source for a tiny substring. Preserve all existing mutation/fallback
checks and establish canonical output equivalence before considering reuse.
This is a follow-up hypothesis; no implementation or measured gain is claimed.

## Avoid a duplicated array nesting pass

Eligible lazy arrays currently scan nesting before building a native tape whose
explicit stack already tracks container depth. Returning depth metadata from
that existing tape pass could eliminate the preflight for valid shallow arrays.
Keep the 1000-level recursive and 500000-level iterative limits, route valid deep
trees through iterative materialization, and preserve the old depth/error path
before any malformed-tape recursive fallback. Typed, throwing and fallible
entries need consistent admission; force-on object roots also need protection.
No input borrow may cross the tape callback's collection points. Measure the
extra per-container depth accounting as well as the pass removed. [Tape-depth R1/R2](TAPE_DEPTH.md) implement this design with six new Rust tests
and a compiled nine-mode depth/GC witness. R1 has array gains in focused and
full original measurements, with conflicting Unicode stringify evidence. R2
keeps the builder separate; its performance evaluation is pending.
