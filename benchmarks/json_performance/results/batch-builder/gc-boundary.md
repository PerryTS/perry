# Where collection happens, and what deferral changes

The batch builder changes construction work, not collection scheduling. Current
ordinary eager parse has these phases:

1. Root the input. Pending work from previous calls and the entry trigger may
   collect **before construction**, within the JSON API call.
2. Suppress collection. Construct final objects, strings and arrays. Finish
   layouts and remembered pages before publishing containers. This metadata is
   necessary for a valid heap; it is not a mark/copy traversal of the graph.
3. Root the result and unsuppress collection. The existing trigger bump can
   request/run work on its tiny-pressure path; the parse-boundary hook schedules
   later work. Thus collection is not categorically forbidden before return.
4. Return the result. The caller can use or mutate it. Its loop GC safepoint can
   collect afterward, and the next parse boundary can consume pending work.

Relevant implementations are `json/parse_api.rs` (ordinary eager path around
lines 476–501) and `gc/policy.rs` (`gc_bump_malloc_trigger`,
`gc_collect_pending_suppressed_parse`,
`gc_schedule_parse_boundary_collection_if_pressure`). No scheduler change is
included in this experiment.

The earlier **record-bytes** wide-parse profile had 1,763 of 2,213 main-thread
samples beneath the caller's moving-GC safepoint, after the JSON call. The parse
branch had 449 samples. These are sampled stack counts from the parent, not
exact duration measurements or a newly measured breakdown of the batch build.
The evidence is in `../record-bytes/remaining-profiles/wide_1m-parse.sample.txt`.

The benchmark measures `run(iterations)` with process CPU and elapsed timers.
Inside each iteration it stores the result in `last`, retained for validation.
Consequently, the latest graph is live at a loop safepoint. GC work there is
charged to amortized CPU per parse even though that call has returned.

## Deferred and concurrent collection

**Deferred collection** can let the function return first and let collection
run at a later safe point, with a byte/pressure budget limiting accumulation.
If a graph dies before the collector examines it, this can avoid tracing or
copying that transient graph, depending on its generation and the collector's
reclamation path. Retained graphs still require tracing. Merely suppressing
collection for an arbitrarily long loop would accumulate garbage and increase
RSS; it would not establish an end-to-end CPU improvement.

Perry already schedules some work for the next parse boundary specifically so
previous temporary roots have time to clear. The benchmark's retained `last`
value is a real root that this scheduler cannot simply ignore.

**Concurrent collection** runs collector work alongside application code on
another thread. It requires a protocol for mutations, root discovery, and any
object relocation. Perry's current arena/collector state is thread-local; this
cannot be implemented by sending its current collector function to a worker.
Existing incremental marking and its birth/store barriers remain relevant but
do not by themselves provide a concurrent moving collector.

A useful boundary contract is: finish metadata, publish the graph, record
allocation debt, and return; the collector owns when that debt is serviced.
Changing current entry/post-construction collection policy belongs in the
separate GC scheduling work. Its validation should distinguish API latency,
end-to-end CPU including debt service, peak/retained RSS, and parse/use/discard
versus retained-output lifetimes. The fixed 38-row baseline must remain intact.
