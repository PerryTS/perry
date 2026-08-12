Fixed two receiver boundaries found during review of #7897's header-gated
property-miss probes.

A possible Symbol header is now read only after the candidate passes the
canonical plausible-heap address check. This preserves both Symbol storage
classes — GC-backed `Symbol()` and Box-leaked `Symbol.for()` — while rejecting
tag remnants, handles, and out-of-range garbage before the four-byte
`SYMBOL_MAGIC` dereference. The regression test uses the exact upper-bound
address `0x8000_0000_0000`; deleting the guard turns a safe `undefined` result
back into a fault.

An authoritative Set-registry hit now returns a value only for `.size`, known
method values, and own exotic expandos. An unknown key continues to the shared
Map/Set receiver path, which owns prototype data-property lookup and the final
`undefined` fallback. Before the fix, a test that installed
`Set.prototype.perryReviewMarker = 7867` read `undefined`; it now reaches the
prototype property. Symbol receiver construction also uses the canonical
NaN-box pointer helper instead of duplicating its tag bits.

The final reviewed binary retained #7897's performance result. Thirty
alternating `pipeline_big` pairs on the locked quiet M1 mini measured
1.693756 s base median versus 1.688867 s fixed median: **-0.343% paired
geomean**, bootstrap 95% CI -0.389% to -0.299%. Both arms exited zero with the
exact oracle output. The full serialized runtime suite passed 2,154 tests with
4 ignored; focused probe coverage, doc tests, address-class, test-registration,
thread-local, formatting, whitespace, and file-size gates also passed.
