**`structuredClone` preserves graph identity.**

The recursion-only cycle guard is replaced by a GC-visible source-to-clone memo, so
cycles and repeated references keep their identity across objects, arrays, maps,
sets, regexps and buffers — a value reached twice now clones once and both edges
point at the same clone, as the structured-clone algorithm requires. Clone failures
raise `DataCloneError` as a `DOMException`, which let the `perf_hooks` `TypeError`
workaround go, and traversal roots are cleared on both the success and failure paths
so a completed clone does not retain the graph it walked.

The memo's entries are visited as mutable roots (`scan_structured_clone_memo_roots_mut`)
and its address-keyed lookup is rebuilt after visitation, because a copying collection
relocates either side. Registering that scanner is what the map clone test now does
explicitly: the GC test harness clears `MUTABLE_ROOT_SCANNERS` so a collection sees
exactly the roots a test installs, so without it the memo is decorative under test and
a clone's address is never rewritten across a forced copying minor.

Closes #8232.
