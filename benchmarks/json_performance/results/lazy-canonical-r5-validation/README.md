# Lazy canonical R5 validation

Source/build `211a204c08ecb7825948e2e9242fc0ba9268d7a9`, version 0.5.1530.
All 306 release JSON tests pass single-threaded (1.73 seconds after compilation).
The matched compiler/runtime-static/stdlib-static build completed in 6m50s.
All 62 source hashes and HEAD match the frozen test/build tree. Three immutable
benchmark objects were linked against those archives; provenance records hashes.

R5 adds a native per-object key-length bitset to lazy stringify admission.
An unseen length bucket proves a key is new; every hit still compares exact bytes.
Lengths separated by 64 deliberately collide. The width cutoff and numeric-key
ordering checks remain. The bitset adds eight bytes to each native validation
frame and lives only for this call. Parser source, tape layout, managed object
construction and collector policy are unchanged from R4.

Two new tests cover 1024 key-pair decisions, including empty and multi-byte keys,
length collisions, nesting, numeric ordering, duplicate positions and the 32-key
width boundary. Four new compiled fixtures expand the probe to 22 fixtures and
nine tape/GC modes: all 198 candidate comparisons match Node. All 22 forced-direct
main controls pass; 11 auto controls reproduce inherited canonicalization errors.
The existing 14 escaped-record comparisons pass.

Protected scan survives 1,323 copying minors and 208,531 moved objects. Retained
outputs survive normal, scheduled and full GC; the Unicode witness moves 12,608
objects. Recurring stringify malloc counts remain 39,40,40,40 for both main and
candidate. Raw-handle (949), address-class, root-holder, store (419), formatting,
file-size, registration and Node-version gates pass on the tested source.
Store INIT/POINTER_FREE/ROOT/STACK classes remain human audited.

The five captured tape/parse functions contain 7,007 identical instruction rows
between R4 and R5, including addresses and encoded words. Raw assembly files have
different binary-path headers; compare-parse-codegen.py excludes those headers.
This is a claim about those functions, not the whole binary. Normalization grows
from 4,472 to 4,484 bytes. The full disassembly is retained.

The two-second local profile intentionally terminates the worker and is diagnostic
only. It contains 1,452 main-thread samples and 431 normalization self samples.
Prominent offsets +572/+592 now check compact separators. The independent
[PR scan profile](../pr-lazy-record-scan-profile/README.md) identifies the adaptive
full-reparse path as a larger remaining scan target. Neither profile is CPU
acceptance evidence or proves an isolated speedup.

The expanded focus compares R5, main, the PR build and R4 in four randomized Perry
arms, with Node as the output oracle. All 40 output checks and 288 timing trials
pass under qualified quiet admission. R5 is rejected: roundtrips remain 15–21%
slower than main and 34–46% slower than the PR build. Compared with R4 they change
by -0.377%, +0.074% and -0.080%; this does not justify further full matrices.
Source and evidence stay on codex/json-lazy-canonical-r5. The accepted PR runtime
subsequently landed through train #10037 at 53df2c671; its complete tree matches
PR head 6a5d2ba5e. Neither R4 nor R5 correction source is in that landed runtime.
