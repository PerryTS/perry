Capture-cell queries now reuse the allocator's exact object-start proof for
header validation. Box/scope dispatch and scope serialization classify the cell
once, removing repeated general tracked-header admission and serial brand probes.
The allocation boundary, arena flag, forwarding flag, kind and size remain the
authorities; no cache, registry, generated fast path or GC root is added.

A runtime regression test checks admitted cells without general resolver probes,
forged interior headers, forwarded cells and missing arena flags. Measurement
and verification details are recorded in the tscwall lane report.

Stored property-key equality now compares an inline string's encoded bytes
directly, removing byte-by-byte scratch-buffer unpacking. Both heap-key and
byte-slice entry points share the same comparison. Tests cover lengths zero
through five, embedded NUL and UTF-8 bytes, every differing byte, and exact-sized
inputs ending at a guard page. The key helpers and their tests moved into a
small module, bringing the comparison module back within the file-size cap.

Strict value equality now checks numeric NaN only for identical bit patterns.
Distinct numeric values retain IEEE comparison, which already rejects NaN;
other routes retain their tag checks. This removes duplicate NaN classification
from ordinary equality without changing signed-zero, boxed-int32, string,
BigInt or object identity semantics. Tests cover signed and distinct numeric
NaN payloads and the Node-visible equality/indexOf/includes behavior.

Intrinsic iterator-step admission now returns the function info already proven
by the current prototype shapes. Captured-method validation consumes that
answer, removing a second receiver-edge, family-shape and ConstFn-slot read.
All mutation, accessor, shared-parent, close-protocol and body-identity guards
remain in the single shared admission operation.

The isolated qb6 tsc A/B removes 1.54% of user instructions and 1,792 emitted
text bytes. The clean locked wall ratio remains 2.23x Node, so the under-2x
target is unfinished. GC diagnostics meet decision 137's reconsideration
thresholds; the multi-core collector remains parked. Zod has a bounded
0.095% instruction increase from changed key-helper inlining; a shared bounded
comparison is the fix-forward plan. All four class-micro instruction slopes
remain flat; their linked runtime text changes by at most 192 bytes.
