# Large parsed strings: preserved reclamation requests

Status: the integrated R6 implementation passes 279 runtime tests, compiled
lifetime/cadence checks, and both CPU/RSS matrices. Exact release-artifact
verification is recorded in [FOLLOWUP_RESULTS.md](FOLLOWUP_RESULTS.md).
R4 was rejected for a small stringify RSS regression; the correction preserves
its large-parse reclamation fix.

The exact PR #10022 merge peaks at 1,357 MiB for rotating parses of the 1 MiB
ASCII-string document, and 1,261 MiB for the Unicode-string document. Bun peaks
at 123 and 153 MiB respectively. The worker preloads eight sources, repeatedly
replaces its last result, and retains no result list. The ordinary repeated
source test reuses an immutable parsed string token and does not expose this
allocation stream.

The scheduling sequence explains how the request is lost:

1. `string_from_json_bytes` creates an individually tracked malloc leaf and
   calls `note_completed_malloc_json_output` after filling its bytes.
2. At 32 MiB of completed leaves, the JSON output hook lowers the malloc-count
   threshold to request a later sweep.
3. The parse completion hook calls `gc_bump_json_malloc_trigger_deferred`.
   `gc_bump_malloc_trigger_with_snapshot` unconditionally replaces that count
   threshold with `current + step`, cancelling the request.
4. The next `service_json_output_sweep_boundary` calls the ordinary trigger
   check, then clears its completed-byte counter. That check no longer sees
   malloc work due. Small object headers exert little arena pressure while
   their large malloc leaves keep accumulating.

A diagnostic run of the R2 binary (whose large-string path matches the merge)
performed 1,000 timed parses plus eight warmup parses of the rotating Unicode
corpus. RSS grew from 28,786,688 to 914,817,024 bytes. GC diagnostics reported
zero copying minors, zero incremental cycle starts, and zero minor/full GC time.
This run was on the development host and is a scheduling witness, not an
acceptance timing result.

[Diagnostic stdout, collector counters, and binary hash](results/large-string-debt-diagnostic/witness.json).

A separate development-host stack sample points to copying, UTF-16 counting,
string-token scanning, and nesting preflight as the major CPU costs. It included
startup and was deliberately interrupted after sampling; its footprint and
timings are not benchmark results. Reclamation must be repaired before using
that profile to choose throughput work.

The candidate republishes completed-byte debt after parse accounting and
acknowledges it only when the malloc-count request is no longer due.
Construction stays suppressed and callback-free. The already rooted input is
the collection boundary; the completed result is returned without being traced
mid-construction. No background thread or new GC knob is needed.

Acceptance must cover alternating distinct sources, retained outputs, both parse
and stringify, suppression/deferred servicing, and actual malloc-leaf reclaim.
Rerun the original rows and rotating-input CPU/RSS comparison with the original
merge as an interleaved reference. A lower footprint does not waive CPU losses.

## R4: preserved completed-output debt

The focused four-engine run passed all 80 output comparisons and 240 timing
trials, with quiet admission before and after measurement. The reference is the
original merge, interleaved in fresh processes with identical work counts.

| Rotating parse | Reference CPU µs | R4 CPU µs | Bun CPU µs | Reference peak MiB | R4 peak MiB | Node / Bun peak MiB |
|---|---:|---:|---:|---:|---:|---:|
| 1 MiB ASCII string | 151.816 | 107.805 | 70.456 | 1,345.828 | 90.141 | 177.500 / 152.422 |
| Unicode string | 186.050 | 147.772 | 61.665 | 1,245.469 | 63.266 | 160.391 / 124.891 |

CPU fell 29.0% and 20.6%; peak RSS fell 93.3% and 94.9%. Both footprints are
below Node and Bun. Bun still leads CPU by factors of 1.53 and 2.40.
Rotating small-record and 1 KiB-object CPU remained within 0.3% of the reference.
The same-input large-string medians rose about 0.8% (roughly 3 ns/call), which
needs longer replication before calling it a regression or parity.

All 278 JSON runtime tests passed, followed by a matched release build. New
runtime tests prove the 32 MiB request survives parse completion, stays pending
through a blocked boundary, causes actual collection, and reclaims discarded
malloc leaves. The supported unsafe-zone test hook models a blocked boundary;
this is not a claim that the legacy suppression API can wrap arbitrary nested
runtime calls.

A compiled test retains 96 alternating ASCII/Unicode parse results, mutates all
96 objects independently, and checks string lengths and endpoint characters.
It matches Node with normal GC, full GC, and scheduled moving GC. Normal mode
ran two copying minors. The scheduled arm ran 59 copying minors and protected
59 retired page sets. The lazy-record scan also matches Node under full GC and
1,323 copying minors with protected retired pages. These are correctness checks,
not throughput measurements.

[Complete focused CPU/RSS table](results/quiet-output-debt-r4-rotating-focused-r5/comparison.md),
[raw trials](results/quiet-output-debt-r4-rotating-focused-r5/timing.jsonl),
[quiet window](results/quiet-output-debt-r4-rotating-focused-r5/window.json),
[build and source hashes](results/quiet-output-debt-r4-rotating-focused-r5/provenance.json),
and [GC witnesses](results/quiet-output-debt-r4-rotating-focused-r5/gc-witness.json).

## Full original run: stringify cadence regression

The complete original comparison passed 200 output checks and validated all
344 measurement groups under quiet admission. The scan CPU reductions remain
78.4%, 30.1%, and 31.7%. The prior tiny stringify CPU increases disappeared:
`string_a` is identical at 9.6905 ns/call; `small_record` is 45.784 versus
45.733 ns/call, with overlapping samples.

Several large stringify peaks rise consistently by 0.75–1.31 MiB. Inspection
shows a collection-cadence issue: R4 waits until a deferred sweep has completed
before clearing the byte counter, but clears bytes produced after the earlier
request too. That admits one extra output before each subsequent collection.
A 150-call Unicode stringify diagnostic records the merge collecting at malloc
counts 39, 40, 40, 40; R4 records 39, 41, 41. The diagnostic is on the development
host and establishes scheduling behavior, not comparative speed.

The next correction must preserve completed-byte debt while carrying forward
bytes produced after a deferred request. Lower rotating-parse RSS does not
waive this stringify regression.

[Full original comparison](results/quiet-output-debt-r4-all-r5/parity.md),
[raw timings](results/quiet-output-debt-r4-all-r5/timing.jsonl), and
[quiet admission](results/quiet-output-debt-r4-all-r5/window.json).

The complete rotating-input run then passed all 380 output comparisons and
1,140 timing trials, with quiet admission at both ends. It reproduced the
large-string improvements: ASCII 107.824 µs / 90.156 MiB and Unicode
147.650 µs / 63.266 MiB. No parse-mode CPU median increased by more than 2.5%
against its interleaved reference, and no peak increased by more than 0.75 MiB.
Selection-only controls have some 0.4–0.6 ns/call increases; they perform no
JSON operations and remain reported separately, without subtraction.

[All rotating CPU/RSS rows](results/quiet-output-debt-r4-rotating-all-r5/comparison.md)
and [full rotating quiet window](results/quiet-output-debt-r4-rotating-all-r5/window.json).

R6 carries a scalar byte cutoff for the previous deferred request. A request
completed before the next JSON boundary retires only that cutoff; bytes produced
since the request remain counted. A collection completed synchronously at the
current boundary retires the entire current count. Blocked attempts preserve
the outstanding debt. This adds no managed allocation or GC environment knob.
A new runtime test checks actual collection intervals across 120 large
stringifies, with the input and current result rooted at caller safepoints.
R6 passes all 279 JSON runtime tests and the matched release build. Compiled
retained-result checks match Node under normal, full, and scheduled moving GC.
The 150-call Unicode stringify diagnostic now matches the baseline's malloc
trigger counts exactly: 39, 40, 40, 40. The 1,000-call rotating Unicode diagnostic
runs 26 copying minors and ends at 63,455,232 RSS bytes. These development-host
runs establish correctness and cadence, not acceptance timings.

The first complete R6 quiet-host attempt failed its final admission check
(one-minute load 2.718 > 2.5), so its timing results are rejected. Its
[disposition](results/quiet-output-debt-r6-all-r5/DISPOSITION.md) preserves that
failure. The [repeat](results/quiet-output-debt-r6-all-repeat-r5/parity.md) passed
quiet admission and removed the extra stringify output's RSS cost.

The integrated candidate additionally includes upstream `d342c816b`'s template
ownership correction. Its matched release build and 279 JSON tests passed.
The [complete original comparison](results/quiet-integrated-r6-all-r5/parity.md)
passed 200 correctness checks and all 344 measurement groups, with quiet
admission at both ends. No timing CPU median increased by more than 2.5% against
the interleaved reference; no peak-RSS median increased by more than 0.75 MiB.
These are screening thresholds, not statistical guarantees of zero regression.

A [longer focused comparison](results/quiet-integrated-r6-focus/comparison.md)
investigated the preliminary ASCII-stringify timing and 13 KiB roundtrip RSS
flags. The roundtrip RSS increase did not reproduce across 25 repetitions, or
across nine longer 20,000-call trials. ASCII stringify's 32,768-call median was
1.8% higher with broadly overlapping trial ranges; the final complete run's
median was 1.1% higher. There is no stable CPU regression established by these
runs, and the uncertainty remains visible in the raw samples.

The measured integrated artifacts identify themselves as 0.5.1527. Upstream
then used that version for release metadata only (`f2dc03582`), so the follow-up
release is 0.5.1528. The original recorded binary hashes and source patches are
preserved; they are not relabelled as later builds.
