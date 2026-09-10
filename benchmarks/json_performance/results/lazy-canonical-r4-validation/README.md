# Lazy canonical R4 validation

Source/build `8a7a29d72dd10d1ab51e3a34108cce9487bd2f3f`, version 0.5.1530.
All 304 release JSON tests pass single-threaded (1.72 seconds after compilation).
The matched compiler/runtime-static/stdlib-static build completed in 6m27s.
All 62 source hashes and HEAD match the frozen test/build tree. Three immutable
benchmark objects were linked against those archives; provenance records hashes.
An initial test-fixture warning was corrected before this final test/build chain.

R4 records a positive plain-string proof in the existing integer-only native tape
entry. Only KEY/STRING link value 1 skips the canonical body scan; zero and unknown
values conservatively retain it. Container links, the 12-byte entry layout,
managed allocation and collector policy are unchanged. UTF-8, compact separators,
key uniqueness/order and number normalization still require validation.
The complete consumer and construction-lifetime audit is in metadata-audit.md.

Six new tests cover every escape, empty/plain Unicode strings, scanner prefixes,
Vec growth, both depth modes, native transfer and scratch reuse, malformed-input
callback exclusion, raw WTF-8 and unknown metadata. All 162 candidate compiled
comparisons across 18 fixtures and nine tape/GC modes match Node. All 18 forced
main direct controls pass; 10 auto controls reproduce inherited canonicalization
failures. The existing 14 escaped-record comparisons pass.

Protected scan survives 1,323 copying minors and 208,531 moved objects; retained
outputs survive normal, scheduled and full GC. The Unicode witness moves 12,608
objects. Recurring stringify malloc counts are 39,40,40,40 for main and candidate.
Raw-handle (949), address-class, root-holder, store (419), formatting, file-size
and registration gates passed on the R4 implementation before the fixture-only
warning correction. Store INIT/POINTER_FREE/ROOT/STACK classes remain human audited.

Tape-builder symbol spans shrink from 10,396 to 10,228 bytes (depth disabled) and
10,764 to 10,592 bytes (depth captured). Parse-entry and string-constructor spans
remain unchanged; equal spans do not prove instruction identity. Combined
normalization grows from 4,356 to 4,472 bytes. Full tape/parse/lazy disassembly
and constructor entries are retained in the codegen JSON files.

The local two-second profile is diagnostic only and intentionally terminates the
worker. It records 1,455 main-thread samples, with 438 self samples in normalization.
Prominent offset +3312 is the duplicate-key slice loop; +752 checks string bounds.
These locations suggest further work, not a quantified isolated cost or speedup.

The qualified eight-case replay rejects R4: record roundtrips remain 15–22%
slower than main and 34–47% slower than the PR build. The 1 MiB parse control adds
1.687% versus the PR build. All 32 output checks and 216 trials completed under
the qualified quiet-host window. No full performance matrices are warranted.
Source stays on codex/json-lazy-canonical-r4; only evidence goes to PR #10036.
