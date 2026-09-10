# Lazy canonical R3 validation

Source/build `6cb0854eb50837495d7ab67aa7c6c749f544dbf1`, version 0.5.1530.
All 298 release JSON tests pass single-threaded (1.71 seconds after compilation).
The matched compiler/runtime-static/stdlib-static build completed in 5m34s.
All 60 source hashes and HEAD match the frozen test/build tree. Three immutable
benchmark objects were linked against those archives; provenance records hashes.

Only lazy source admission selects padded four-to-seven-byte quote/backslash
tails. The ordinary scanner entry points retain their prior specialization.
Existing scalar-oracle exhaustive byte, adjacent-borrow, randomized and guard-page
tests now cover both the dispatched helper and its portable word specialization,
including tails after eight- and sixteen-byte prefixes. ARM64 NEON and the portable
word code ran on this host; the SSE2 implementation was not executed here.
There are no new unsafe sites, dependencies, managed intermediates, headers or
GC-policy edits. All 162 candidate comparisons across 18 fixtures and nine
tape/GC modes match Node 26.5.1. The 36 main controls preserve the same inherited
canonicalization failures; all 18 forced-direct controls pass.

The protected scan has 1,323 copying minors and 208,531 moved objects. Retained
outputs survive normal, scheduled and full GC; the Unicode witness moves 12,608
objects. Recurring stringify malloc counts remain 39,40,40,40 for both main and
candidate. All 14 escaped-record comparisons pass. Raw-handle (949), address-class,
root-holder, store (419), formatting, file-size and Node-version gates pass.
The store gate's INIT/POINTER_FREE/ROOT/STACK classes remain human audited.

Captured parse-entry and string-constructor symbol spans are unchanged from R2;
equal spans are not proof of identical instructions. Combined normalization grows
from 4,240 to 4,356 bytes. Full disassembly is retained in the codegen JSON files.
The local two-second profile is diagnostic only and intentionally terminates the
worker. It records 1,559 main-thread samples, with 557 self samples in normalization.
This is location evidence, not an acceptance timing or a cross-host comparison.

The qualified focused replay rejects R3: record roundtrips remain 25–34% slower
than main and 46–62% slower than the current PR build. No full performance
matrices or broader no-regression claim are made for this candidate. Source stays
on `codex/json-lazy-canonical-r3`; only evidence is copied to PR #10036.
