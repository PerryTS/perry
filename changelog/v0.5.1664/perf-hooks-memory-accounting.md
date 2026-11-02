Fix global `performance` value/property lowering so epoch-relative timestamps remain finite in helpers, object literals, and workers. Respect local bindings when lowering performance calls.

Report per-thread buffer backing and malloc-managed storage in `process.memoryUsage().external`, with buffer backing included once in `arrayBuffers`. Header-only aliases and detached stores contribute no backing. Heap counters remain per-thread arena statistics and RSS remains process-wide; arena-resident backing overlaps the arena heap counters.

Add native thread and heap-owner IDs to GC trace/census JSON and identify the thread in heap snapshot metadata, root names, and default filenames. Fix allocation-census startup recursion with allocation-free Rust TLS and resolve Linux executable image bases with `dladdr`.
