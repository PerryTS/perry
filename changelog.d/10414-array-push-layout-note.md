Drop the provably-no-op layout note from the raw-f64 array push.
`layout_note_slot` was 15.0% of a push/pop loop, 96 of its 109 samples from the
single call in `array_numeric_raw_f64_push_inbounds`, where the caller has
already proved the value is a plain number. The mask work is then a no-op in
every layout state the receiver can be in — the same argument already written
out for the codegen-side elision on `array_store_needs_layout_note`'s object
twin. The #7480 element-shape invariant is kept, through the resolved-flags
entry so it reads the header the caller already holds.

Validated on the shape that would expose a mistake: array slots filled with
POINTERS, popped, then refilled with plain numbers, with a retained live graph.
Three seeds under from-space protection, evacuation verification and
PERRY_GC_FROMSPACE_SCAN_ABORT=1 each ran ~270,000 copying minors and ~33,700
from-space scans with dangling=0 and missing_rewrites=0, byte-identical to node.
