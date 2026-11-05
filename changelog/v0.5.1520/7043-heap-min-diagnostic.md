Two GC diagnostics salvaged from the #7022 investigation.

**`js_array_grow`'s below-`HEAP_MIN` stub skip is no longer silent.** Whether a
growth forwarding stub gets installed depends on where the allocator placed the
array — an address-conditional behaviour change with no signal. It now fires a
`debug_assert` and a once-per-process stderr line. That silence already cost real
debugging time: an experiment that replaced arena blocks with `mmap`'d guard-paged
blocks used a NULL hint, landed below the platform floor, silently disabled every
growth stub, and came back falsely clean.

**The from-space scan reports snapshot coverage.** Offending slots are now split
into `not_in_snapshot` (the page was never in this cycle's dirty snapshot, so the
in-cycle remembered-set scan never looked at it) and `in_snapshot` (the scan
looked at the page and still did not rewrite the slot). That split is what
established #7022's failure as in-cycle rather than a dropped-then-restored edge.
