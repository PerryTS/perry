# Lazy canonical R1 validation

Source/build `96df018b1e436d52aefc0eca8a55fcb5df5e2ee4`, version 0.5.1530.
All 297 release JSON tests passed single-threaded (1.65 seconds after compilation).
The matching compiler/runtime-static/stdlib-static build completed in 5m30s.
All 60 frozen source hashes and HEAD matched before linking the three immutable
benchmark objects. Provenance records source, archive and executable hashes.
GC policy and managed-header layouts are unchanged.

Six new tests cover source spelling, duplicates, integer-key ordering, nested
frames, native-only admission, Unicode/escapes, invalid raw WTF-8, number
normalization, malformed ranges and the 32-key admission bound. Escaped keys,
Unicode escapes and wider objects conservatively materialize.

The compiled canonical matrix has 14 fixtures × 9 tape/GC modes: all 126 candidate
comparisons match Node 26.5.1. Each scheduled arm protects at least 5 retired sets
and moves at least 12,607 objects. All 28 main controls are retained: all 14 direct
controls pass; 8 auto controls fail, including the original 5 cases. The additional
failures involve escaped duplicates, nested duplicates and an index key after a
string key. These distinguish inherited defects from the correction.

The existing protected scan has 1,323 copying minors and 208,531 moved objects.
Retained outputs survive normal, scheduled and full GC. The 1,000-parse Unicode
witness moves 12,608 objects; recurring stringify malloc counts remain 39,40,40,40
on main and candidate. All 14 escaped-record comparisons pass, with 12 negative
failures on the older uncorrected main. Raw-handle (949), address-class, root-holder,
store (419), formatting, file-size and Node-version gates pass. The store gate's
INIT/POINTER_FREE/ROOT/STACK classes remain human audited.

`location-profile` is a two-second local stack-location diagnostic with an
intentionally terminated million-roundtrip worker, not an acceptance timing.
801 of 1,454 main-thread samples lie under the new source proof. Repeated UTF-8
validation, generic separator comparisons and the second number-token walk are
concrete next targets. The separate quiet-host focused run rejects R1 on CPU cost.

Main-vs-R2 assembly accompanies the separate eager-stringify concern. Equal
spans/counts do not establish identical code or explain the slowdown. No result
from masking instruction addresses is used as an equality proof.
