# Lazy canonical R2 validation

Source/build `c812afd4841e79970d298f7b4ff9b97c9d8888ab`, version 0.5.1530.
All 298 release JSON tests pass single-threaded (1.62 seconds after compilation).
The matching compiler/runtime-static/stdlib-static build completed in 5m30s.
All 60 source hashes and HEAD match the frozen test/build tree. Three immutable
benchmark objects were linked against those archives; provenance records hashes.

The source proof and number normalization now share one tape traversal. UTF-8
validation runs once for the source. Fixed-width separator comparisons replace
generic slice comparisons, and adjacent token offsets identify string ends.
A new visitor test covers inner-array roots and excludes numbers in strings or
other subtrees; additional cases cover whitespace beside quotes and empty keys.
There are no new managed intermediate values, header changes or GC policy edits.

All 162 candidate comparisons across 18 fixtures and 9 tape/GC modes match Node
26.5.1. The expanded matrix includes 999/1000-deep whitespace arrays, a canonical
deep array and arrays of 33-key objects. Every scheduled arm protects at least
5 retired sets and moves at least 12,607 objects. All 18 main direct controls
pass; 10 main auto controls fail, including the two new deep whitespace cases.

The existing protected scan has 1,323 copying minors and 208,531 moved objects.
Retained outputs survive normal, scheduled and full GC; the Unicode witness
moves 12,608 objects. Recurring stringify malloc counts remain 39,40,40,40 for
both main and candidate. All 14 escaped-record comparisons pass, with 12 negative
failures on the older uncorrected main. Raw-handle (949), address-class, root-holder,
store (419), formatting, file-size and Node-version gates pass. The store gate's
INIT/POINTER_FREE/ROOT/STACK classes remain human audited.

The two-second local location profile is diagnostic only; the worker is
intentionally terminated after sampling. Of 1,458 main-thread samples, 749 are
under combined normalization and 576 are self samples in that function. The
prominent +1388/+1400 offsets map to the scalar quote/backslash byte loop, shown
in `location-profile/hot-tail.txt`. Whole-source UTF-8 validation has 7 samples;
repeated short-string validation and generic separator comparisons no longer
appear on the previous paths. This is location evidence, not an acceptance timing.

Combined normalization has a 4,240-byte symbol span; lazy dispatch is 404 bytes.
The quiet focused replay still rejects R2 on record roundtrip CPU cost. No full
performance matrices or broader no-regression claim are made for this candidate.
