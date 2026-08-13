### perf(json): right-size tape-materialized object spill buffers (#7267)

JSON tape materialization previously allocated every object at the two-slot
inline floor, then let the first overflowing property create a general-purpose
array with 16 slots of growth headroom. Five-field records therefore carried a
16-slot side allocation even though the tape already knew their final width.

Both the recursive lazy materializer and the iterative deep-input materializer
now reserve an exact-width spill buffer before storing fields. The recursive
path counts only the current object's keys by hopping across nested container
links; the iterative path reuses the key count it has already collected. The
primary object deliberately remains at `INLINE_SLOT_FLOOR`: sizing its inline
allocation to every key was benchmarked in #7267 and regressed the named field
access workload.

On `benchmarks/json_polyglot/bench_field_access.ts`, eight interleaved
`perry-dev` runs with `PERRY_NO_AUTO_OPTIMIZE=1` reduced median time from
1266.5 ms to 1138.5 ms (-10.1%) and peak RSS from 309.3 MiB to 294.3 MiB
(-4.8%), with identical checksums. A direct-parser control was unchanged
(934.5 ms vs 935.5 ms median), isolating the improvement to tape-backed object
materialization.
