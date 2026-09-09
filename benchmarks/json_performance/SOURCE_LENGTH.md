# Dominant JSON string length reuse

Work in progress on `codex/json-source-length`, based on PR #10032 at
`b2111219c1a4353b05f35fc55d9305c55d2cb135` (0.5.1528). The candidate package version
is 0.5.1529. This is not a measurement of merged main.

## Change and proof

An unescaped JSON string value already occupies a contiguous range of its
input. The full input string header already contains its UTF-16 length. If the
bytes outside that token are ASCII, their byte count is also their UTF-16 count;
subtracting it can replace another full traversal of the token.

Admission requires a token of at least 512 KiB, the exact whole-source owner and
input address, a token range derived from the parser's consumed closing quote,
and at most 512 ASCII bytes outside the token. A bounded malformed-tail check
rejects cases where the legacy counter could consume the closing quote as part
of a declared-width sequence. Checked subtraction and range checks decline
inconsistent lengths. Escaped strings, source slices, non-ASCII surroundings and
other declined cases use the existing counting path.

The result has its own storage. Allocation, copying, escape flags and completed
output debt accounting use the same constructor body. This adds no managed
roots, caches, retained input views, collection points or GC policy changes.
Construction remains inside the existing rooted/suppressed parse interval.

Three unit tests cover all 65,536 two-byte patterns under four prefix lengths,
astral and surrogate sequences, malformed/truncated tails, vector boundaries,
exact owner identity, slices, non-ASCII surroundings and bounded surroundings.
Acceptance requires both positive and negative admission witnesses. Compiled
Node comparisons and the existing retained-output and moving-GC witnesses
exercise the actual parser and result lifetime separately from CPU timings.

## R1: useful speedup, rejected shared-path cost

The first quiet focused run used five interleaved trials per engine, eight
preloaded inputs changing one value, and the same 0.5.1528 reference binary.
Inputs and outputs are checked before timing; file I/O is outside the timed loop.

| Rotating fixture | Reference CPU µs | R1 CPU µs | Change | R1 / fastest Node or Bun |
|---|---:|---:|---:|---:|
| Small record, 109 B | 0.498084 | 0.503749 | +1.14% | 1.970× |
| Object, 1,021 B | 0.496556 | 0.498423 | +0.38% | 2.077× |
| ASCII string object, 1,048,594 B | 108.041 | 95.621 | −11.50% | 1.354× |
| Unicode string object, 884,018 B | 148.298 | 81.300 | −45.18% | 1.292× |

All five small-record R1 CPU samples exceed all five reference samples. The
complete original matrix also finds small regressions on several object rows.
Disassembly shows the shared string-value parser growing from 976 to 2,476
bytes and its stack frame from 96 to 144 bytes: constructor counting code moved
into the common parser. R1 is superseded, not accepted for release.

Raw evidence: [focused matrix](results/quiet-source-length-r1-focus-r5/comparison.md),
[original matrix](results/quiet-source-length-r1-all-r5/comparison.md). Both include
quiet-window records, exact source patches, hashes and their disposition.

## R2: isolate the large-token work

R2 moves the proof and counted allocation into an outlined helper, retaining a
simple size dispatch and the ordinary constructor in the small-token arm.
The matched release build and all 282 JSON tests pass, as do compiled lifetime
and moving-GC witnesses. Disassembly still shows a 2,408-byte shared parser and
144-byte frame: the compiler inlines the normal counter despite the helper.
See [R2 validation](results/source-length-r2-validation/README.md). R2 remains
an experiment; acceptance requires resolving the common-path growth and the
separate lazy-surrogate review finding on its base PR.
