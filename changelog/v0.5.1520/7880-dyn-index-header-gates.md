### `perf(runtime)`: gate dynamic-index collection probes by the GC header (#7865)

`js_dyn_index_get` and `js_dyn_index_set` used to consult both the `Map` and
`Set` side registries on every dynamic index operation once either registry had
been armed. They now read the receiver's already-required `GcHeader` first and
consult only the registry selected by its object type. Registry membership
remains the authoritative ownership check; the header is only a prefilter.

Dedicated probe counters lock in the structural saving: ordinary array indexing
touches neither collection registry, while `Map` and `Set` receivers still reach
exactly their own registry. On the quiet M1 mini, an amplified `interp` run was
about 0.3% faster but within noise, and a targeted 20-million-iteration dynamic
index workload was exactly unchanged. This lands for the eliminated registry
lookups and the regression coverage, not for a claimed wall-clock speedup.
