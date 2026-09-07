# Bounded escaped-string decoding

The experimental decoder reduces CPU for parsing the escaped 1 MB fixture by
about **30–31%**, including in workloads that explicitly collect afterwards.
It changes no GC policy. It has **not** met the overall no-regression or
Node/Bun parity objective and has not been pushed or merged.

The current experiment is on `codex/json-escaped-outlined-1520`, with runtime
source at `b3367bd93`. The preceding inlined variant is preserved at
`f441a6ca2` on `codex/json-escaped-decoder-1520`. Both include the earlier
experimental UTF-16 counter from `32e843c55`; the retained GC reference is
`0327b9460`. These are separate from the original GC-deferral branch/PR.

## Latest focused result

Seven fresh processes per engine and row, Apple M1 / 8 GiB, default GC,
matched default release settings, Node 26.5.1 and Bun 1.3.14. CPU includes
user and system time in the work loop; RSS is whole-process peak.

| Escaped 1 MB object | Previous Perry | Current experiment | Node | Bun |
|---|---:|---:|---:|---:|
| Parse CPU, ms/call | 2.748 | **1.891** | 1.733 | 2.084 |
| Parse peak RSS, MiB | 56.984 | **57.016** | 74.016 | 64.172 |
| Stringify CPU, ms/call | 1.102 | **1.112** | 1.893 | 2.084 |
| Stringify peak RSS, MiB | 55.953 | **55.984** | 124.609 | 74.781 |

Parse uses 31.17% less CPU than the previous counter candidate, about 9.3%
less than Bun and 9.1% more than Node. Stringify's +0.91% median has overlapping
observed ranges; it remains faster than both competitors on this fixture.
These are individual workloads, not aggregate performance claims.

The current variant changes only `parse_string_bytes`'s inlining attribute
relative to the preceding decoder. It reduces measured peak RSS by
0.109–0.141 MiB across the nine focused rows. Relative to the earlier counter,
those rows range from -0.015625 to +0.03125 MiB. It also gives back some of the
inlined variant's object-parse gain: small-record parse is +1.76% with separated
observed ranges, and 1 MB object-envelope parse is +2.18% with overlapping
ranges. Those changes are +0.41% and +0.61% against the earlier counter.
No universal preference between the layouts is established.

See the [nine-row inlining control](decode18/results/focused/summary.md) and
its [lifetime comparison](decode18/results/lifetimes/summary.json). The latter
includes final collection: escaped parse improves 31.50%, 31.45% and 26.71%
for discard, latest and retain versus the counter. The 24-round-trip large-object
workload changes -0.36%. Small-record stringify lifetimes change +0.55%, -0.15%
and +2.12%; their observed ranges overlap. This repeat does not reproduce the
preceding window's large retained-stringify median increase.

## Full 38-row sweep of the preceding layout

The inlined decoder (`decode17`) has a complete five-engine comparison:
190 output checks, 950 timing trials, 360 Perry retained-memory trials and
162 Perry lifetime trials. The shared-host entry/end gates pass, with 170
clean continuous-monitor observations. Both earlier Perry builds and the
candidate use one executable pathname; Node and Bun are measured in the same
window. Runtime and fixture hashes are recorded.

**[All 38 CPU/RSS rows](decode17/results/recheck/all-38.md)** retain every result.
The current out-of-line variant has only the subsequent focused comparison;
the all-38 table must not be attributed to it.

Escaped parse falls from 2.742 to 1.912 ms/call (-30.26%), with separated
observed ranges. Long ASCII parse changes +0.30% and Unicode parse -0.04%, both
overlapping. Several materialized record-parse rows improve about 2.4–2.8%.
The candidate is at or below both competitors' CPU medians on 14 of 38 rows.
Small-record stringify still costs about 2.9 times Node's CPU.

The concerns remain visible: numeric-array stringify is +1.18% and tiny-object
stringify +1.35%, both with separated observed ranges in this window. Most
main-loop peak RSS medians increase about 0.09–0.19 MiB; wide-object parse is
the exception at -1.58 MiB. The 40 retained-memory comparisons have peak-RSS
increases of 0.0625–0.1875 MiB and current-RSS changes of -0.015625–0.21875 MiB.
Scratch-capacity equivalence does not imply identical process RSS.

The first retained small-record stringify lifetime median increases 20.44%
versus the counter. Its counter samples span 44.2–70.0 ms, while the candidate
spans 71.55–71.74 ms; the earlier GC reference has a 71.10 ms median. The later
seven-repeat control has a much smaller difference, so the first figure must
not be treated as a stable causal estimate. Both windows remain in the data.
Overlapping ranges are not proof of equivalence, and separated sample ranges
are not confidence intervals.

The older complete CPU/RSS target inventory, including its other operations,
still requires fresh qualification. This sweep and the focused follow-up do
not close that work or the broader parity goal.

## What changed

The original escaped decoder performs checked `Vec::push` operations and
updates the vector length for each output byte. The new path decodes a bounded
input window through a raw pointer and commits the output length once per
window. Short strings and allocation growth use the scalar path.

Every chunk requires at least 64 remaining input bytes and 64 existing spare
output bytes. Each decode operation starts before byte 52 of that window. A
surrogate-pair escape consumes at most 12 input bytes and emits at most four,
so both reads and writes stay inside the proved bounds. Short tails retain
the original checked decoder. Invalid escapes, truncations, controls and lone
surrogate WTF-8 output retain their existing behavior.

The chunk path never grows the vector. This matters: an earlier version
reserved for a worst-case window and could double a 2 KiB buffer immediately
before the string ended. The retained design returns to scalar decoding when
spare capacity is insufficient, preserving the original allocation-growth
sequence. The differential capacity control finds 43 changes in the early
reserve variant and zero in the retained design on its escaped-tail corpus.

Temporary bytes still live in an ordinary Rust vector and are released after
the managed string is built. There are no new managed roots, GC calls, GC
environment knobs, per-object registration, or collection-scheduling changes.

## Experiments and validation

The standalone prototypes are diagnostic string decoders, without Perry object
construction, string finalization or GC. Their initial large escaped-string
gain is about 58%, but the first version makes arrays of short escaped strings
33% slower. Delayed admission and function boundaries reduce that problem;
only full-runtime measurements establish the results above. The source,
nightly compiler/profile details, tests and all three quiet measurement windows
are preserved under `prototypes/`.

Both complete runtime candidates pass 3,283 unit tests (four existing ignores),
40 compiled comparisons with Node and 52 seeded moving-GC checks, plus their
unseeded controls. Stress verdicts require actual scheduled collections,
copying minors, object movement and loop polls. The inherited root-array
Object.prototype.toJSON mismatch remains explicitly compared with the reference;
it is not newly classified as passing Node.

New tests cover window boundaries, all truncations of escaped strings, controls,
surrogate pairing and lone surrogates, generated strings against serde_json,
buffer capacity near powers of two, and inputs ending against a protected page.
The prototype also compares decoded bytes, parser position, validity and final
capacity against the original decoder over arbitrary bytes.

The runtime root-holder inventory and Node-version check pass. The existing
file-size failures and four JSON address-inventory findings are unchanged; no
baselines were relaxed. Their logs and exit codes are preserved. The inventory
run is on `decode17`; `decode18` changes only one inlining attribute.

The executable-installation probe in `inode12/` failed its end load gate and is
diagnostic only. It does not prove that hardlinks eliminate timing noise. The
first runtime trial also stopped on a harness error: POSIX rename is a no-op
when source and destination are hardlinks to the same inode. The installer now
handles that case explicitly; the failed partial run is archived separately.
No pinned executable is overwritten. Quiet gates record their specific checks,
not an assertion that all background activity or measurement noise is absent.

## Next investigations

Six separate three-second native profiles show the next useful targets. In
escaped parsing, the new decoder has 937 leaf samples and the nesting-depth
preflight 841; preflight now costs almost as much as decoding. Reducing its
repeated escape scanning is the next parse target. These are sampled stacks,
not independent per-function CPU measurements.

Small-record stringify spends its largest sample counts in string planning,
record emission and memory copies. A compact plan specifically for string keys
can avoid passing and storing the larger general scalar representation. That
needs its own correctness and CPU/RSS comparison; it is not implemented here.
Neither investigation requires changing GC policy.

Run `python3 benchmarks/json_performance/results/json-escaped-decoding/verify.py`
to verify the preserved evidence and source identities.
