# Tiny parse call-tree profiles

Three-second `sample` captures on the quiet benchmark M1, using the preceding primitive-array worker (`a72f64a78`; SHA recorded in `provenance.json`) and default GC. Each process runs 100,000,000 parse calls after eight warmups. These instrumented executions are diagnostics, not additions to the CPU/RSS standings. Both completed successfully; the owner-token lock and quiet gate passed.

For `null`, 1,424 of 2,251 main-thread samples are under the pre-parse `gc_check_trigger` call and 392 are under `gc_bump_malloc_trigger`: together about 81%. The `DirectParser::parse_value` call has 14 samples. This shows that collection-policy bookkeeping, including repeated TLS and arena-pressure queries, dominates this allocation-free parse workload; it does not mean 81% is spent actually collecting garbage.

For `{"a":1}`, the corresponding call sites have 1,015 and 232 of 2,281 samples (about 55%), while the parser call has 649. Unlike `null`, this case must allocate a fresh output object.

The next parse work should distinguish allocation-free scalar results from allocating object results. A fast scalar path could avoid the object-building setup, while allocating parses must preserve pending-collection and input/output-rooting requirements. These profiles alone do not authorize deleting boundary collection work from allocating parses. Collector policy and parse-boundary source are unchanged in the direct-output stringify candidate.
