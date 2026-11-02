Descriptor installs prove the owner is a Perry GC allocation before writing
its meta edge. A descriptor owner can be any address — a registry handle, or
a native `Box` backing such as an `AsyncResource`'s (#11258) — and the
magnitude-window header reader admits a native allocation whose preceding
bytes happen to decode as a cell type (a Map, in the observed case); the
install then stored an `ObjectMeta` pointer into native memory. The install
twin (`descriptor_summary_meta_ensure`) now gates on
`try_read_tracked_gc_header`, so such an owner keeps its entries in the
descriptor tables only. Lookups are unchanged: installs are rare, and handle
owners already probe the tables directly (bf16bc752).
