**Perry can become substantially faster at JSON by removing avoidable work first, then specializing ordinary data around stable object shapes.** The measurements do not support a single “Perry is X times faster/slower” claim: lazy parse, fully materialized parse, stringify, strings, and wide objects have different bottlenecks.

This report combines measured CPU, elapsed time, resident memory, retained-result experiments, source inspection, and six separate sampling profiles. The intended next step is an optimization campaign; no production runtime or compiler changes were made here.

The benchmark host was an Apple M1 with 8 GiB RAM, macOS 26.5.1. Node was **26.5.1 / V8 14.6.202.34**, Bun **1.3.14**, and Perry **0.5.1520** from a pre-existing development build in a release directory. Compiler and linked libraries were copied and hashed before use. The checkout used to run the original audit identified itself as **0.5.1512** and differed materially in JSON implementation. This implementation branch is based on 0.5.1520; its fresh runtime A/B is documented separately in [FASTPATHS.md](FASTPATHS.md). These numbers must not be attributed to a fresh build of the checkout or an official clean release. The original Cargo build environment is not fully reconstructed. [Artifact and source provenance](results/toolchain.json) records the distinction.

The main and follow-up runs held the shared benchmark lock and passed the start/end quiet-host checks. The main window ran 04:29:39–04:36:10 UTC on 2026-09-06, with one-minute load 1.81 before and 1.77 after. There were **150 successful output comparisons**, **450 default timing trials**, **288 retention trials**, and **96 forced-tape/direct trials**, with no failed processes in those groups. Medians use three fresh processes. [All tables](results/full-tables.md), [machine-readable summary](results/summary.csv), and [the comparison chart](results/cpu-comparison.png) accompany this report.

CPU below means **user + system process CPU during the work loop**, including GC and helper threads. Elapsed time is recorded separately; these differ when Node or Bun performs parallel work. Compilation, process startup, file reading, input preparation, output verification, and printing are excluded from loop timing. Inputs are reused strings; this measures hot JSON processing, not a network/UTF-8 decode pipeline. GC is enabled by default in all engines. Iteration counts are identical across engines within each cell, although different cells use different counts. Warmup is 5,000 calls for small inputs and two calls for larger ones. The small array scan illustrates why short calibration does not establish sustained performance.

*Small values: CPU microseconds per call, lower is better.*

| Operation / input | Perry | Node | Bun |
|---|---:|---:|---:|
| Parse `null` | 0.219 | 0.027 | 0.019 |
| Parse `"a"` | 0.222 | 0.032 | 0.023 |
| Parse `{}` | 0.291 | 0.045 | 0.024 |
| Parse `{"a":1}` | 0.337 | 0.082 | 0.045 |
| Parse 109-byte record | 0.763 | 0.333 | 0.252 |
| Stringify `{"a":1}` | 0.189 | 0.037 | 0.040 |
| Stringify 109-byte record | 0.541 | 0.108 | 0.120 |
| Stringify 1,021-byte object | 1.127 | 0.199 | 0.215 |

Tiny calls expose fixed runtime overhead. The small object parse is approximately **4.1× Node's CPU cost and 7.4× Bun's**; its stringify is approximately **5.1× Node's and 4.7× Bun's**. These are sub-microsecond operations, so the benchmark loop and dispatch remain part of the measurements. The tiny stringify profile nonetheless identifies substantial internal overhead: approximately 13% of main-thread leaf samples in thread-local lookup, 10% in `toJSON` key probing, and 7% in heap-pointer classification. This is a strong reason to build a guarded plain-data path with a compact per-call context.

*Large values: CPU milliseconds per call. Sizes are decimal input MB; fixture filenames contain nominal size labels, while the table uses actual byte counts.*

| Operation / input | Perry | Node | Bun |
|---|---:|---:|---:|
| Parse 0.89 MB record array, unread | 1.903 | 2.893 | 2.176 |
| Parse 0.89 MB object envelope | 4.446 | 2.797 | 2.145 |
| Parse 7.12 MB record array, unread | 16.665 | 30.559 | 21.861 |
| Parse 7.12 MB object envelope | 34.840 | 30.002 | 22.095 |
| Parse 17.77 MB record array | 86.202 | 98.069 | 57.334 |
| Parse 1.05 MB long-string object | 1.438 | 0.371 | 0.069 |
| Parse 0.98 MB / 50,000-key object | 405.696 | 5.677 | 4.118 |
| Stringify 0.89 MB record array | 2.327 | 0.854 | 0.987 |
| Stringify 7.12 MB record array | 19.494 | 6.835 | 8.517 |
| Stringify 17.77 MB record array | 97.269 | 17.411 | 20.758 |
| Stringify 1.05 MB long-string object | 0.979 | 0.107 | 0.098 |
| Stringify 1.07 MB numeric array | 8.000 | 1.934 | 2.958 |
| Stringify 0.99 MB escape-heavy object | 1.860 | 1.862 | 2.100 |
| Stringify 0.98 MB / 50,000-key object | 2.806 | 6.381 | 0.668 |

Perry's advantages are real but specific. It wins the unread array parse cases below its lazy cutoff and these untouched roundtrips. Ordinary record serialization costs about 2.7–2.9× Node's CPU at 0.89–7.12 MB. The 17.77 MB stringify case incurs additional allocation/collection cost and is about 5.6× Node. Escape-heavy stringify is competitive. Wide-object stringify beats Node here, even though wide-object *parse* is the most severe ordinary parser problem.

For the 0.89 MB array, stringify throughput from elapsed time is about 360 MiB/s for Perry, 1,030 MiB/s for Node, and 860 MiB/s for Bun; consult the CSV for exact values. There is no single meaningful aggregate “GB/s” without specifying the payload and operation. A megabyte of one ASCII string exercises far fewer objects than a megabyte of records.

Perry's array parser uses a tape by default when an array-root input is between 1 KiB and 16 MiB. Object roots parse eagerly. The tape validates/indexes input while postponing object construction; it retains the input string, token entries, and element-cache storage. Consequently, `.length`, reading two records, walking every record, and stringify are distinct workloads.

*Same 0.89 MB record array: CPU milliseconds per operation.*

| Work | Perry | Node | Bun |
|---|---:|---:|---:|
| Parse, leave records unread | 1.903 | 2.893 | 2.176 |
| Parse, read first and last IDs | 2.027 | 2.899 | 2.202 |
| Parse, sum all record IDs | 6.368 | 3.043 | 2.299 |
| Parse, stringify without accessing records | 2.700 | 3.629 | 3.125 |
| Stringify an already materialized array | 2.327 | 0.854 | 0.987 |

The untouched roundtrip can reuse source bytes, with number normalization in this build. It must not be presented as normal object serialization speed. Every standalone stringify trial deliberately starts with an eagerly materialized value. Conversely, an unread parse result is useful for applications that really inspect little of a document, even though its representation differs from Node and Bun.

The 16 MiB cutoff is observable: the 17.77 MB array becomes eager by default. Forced tape parsing reduced its CPU from 86.1 to 42.6 ms; forced tape roundtrip reduced 141.7 to 58.3 ms. For full scanning, direct was better: 88.3 ms versus 121.9 ms. These are diagnostic comparisons at matched counts, not a recommendation to enable tape universally. A workload-sensitive decision can outperform a size-only rule.

A **13,197-byte, 120-record array** exposed a sustained-load cliff. At 3,724 parse-and-scan iterations, Perry took a median 48.55 seconds of loop CPU, Node 0.153 seconds, and Bun 0.134 seconds. Perry's median whole-process peak RSS was **3,220 MiB**, compared with about 62 and 77 MiB. Its peak physical footprint was about **3,416 MiB**. RSS varied with residency/compression, while retired instructions and physical footprint reproduced closely.

Separate matched tape/direct trials at 2,245 iterations took **5.710 ms/call with tape versus 0.0688 ms direct**, with peak RSS about **1,672 versus 33 MiB**. That is approximately an **83× CPU difference** for this workload, without changing the collector. This is not a universal parse speedup: it isolates a pathological lazy-materialization route. Short diagnostics show worsening scaling: default scan cost was 0.244 ms/call at 100 iterations and 0.769 ms/call at 1,000; direct stayed around 0.065–0.073 ms/call.

The adaptive scan switch requires a streak of at least `max(64, length/64)` and `cached_count * 2 < length`. For arrays of at most 128 elements, a sequential scan cannot satisfy both conditions. Each element follows the materialization path. The sampling profile then spends roughly **87% of leaf samples in four GC heap-classification/layout-cleanup functions**. The JSON path creates the pressure; collector cleanup amplifies it. Fixing the threshold alone has not been implemented or timed, and the 129/256-element probes still show substantial lazy overhead. An efficient batch materializer is the broader remedy.

RSS is whole-process memory, not the exact size of live JSON. It includes runtime code/data, arenas, JIT/compiler structures, caches, temporary buffers, and uncollected garbage. Peak RSS includes input setup; current RSS was also sampled before and after the timed loop. Heap-used numbers from different collectors would not be comparable, so they are not used as the primary memory metric.

*Retained-result experiments: current RSS after the live batch, MiB. No forced collection.*

| Retained work | Perry | Node | Bun |
|---|---:|---:|---:|
| 200,000 parsed `{"a":1}` objects | 24.2 | 74.7 | 50.2 |
| 100,000 parsed 109-byte records | 55.8 | 85.4 | 52.7 |
| 16 unread parsed 0.89 MB arrays | 40.6 | 87.5 | 55.9 |
| 16 parsed 0.89 MB object envelopes | 65.9 | 87.5 | 55.9 |
| Four parsed 7.12 MB object envelopes | 112.3 | 145.8 | 91.8 |
| 32 retained 1.05 MB stringified outputs | 58.7 | 91.1 | 63.7 |

Perry usually starts with a much smaller process footprint. That does **not** imply the smallest per-record representation. Comparing 100,000 small retained records against one retained record increased RSS by approximately **44.2 MiB in Perry, 33.9 MiB in Node, and 24.4 MiB in Bun**. For sixteen eager 0.89 MB envelopes, the corresponding increase was **50.6 / 30.2 / 24.2 MiB**. These are incremental process-footprint estimates, not exact live-heap sizes: allocator growth and GC timing remain included. Repeated identical input can also benefit each engine's string/shape caches, so unique-message retention is a useful next benchmark.

Perry's tape consumes 12 bytes per token entry in this ABI, plus source retention and cache storage. Laziness can save construction and still have a substantial metadata footprint. The direct parser's “zero-copy” string fast path refers to a temporary borrowed input slice; ordinary non-inline result strings are subsequently copied into Perry-managed strings. The stringify scratch `String` is retained after each call, while the result is copied into a separate managed string. A very large stringify can therefore leave a large reusable buffer behind. These are JSON-specific allocation decisions worth improving independently of GC scheduling.

The path to substantially faster JSON is a sequence of concrete changes:

1. **Remove the clear algorithmic and byte-processing bottlenecks.** Replace the wide-object duplicate-key linear search with a small inline strategy for tiny objects and a hash/index strategy beyond that threshold, preserving first insertion order and last-value-wins semantics. Current parse scales from about 17.9 ms at 10,000 keys to 104.4 ms at 25,000 and 405.7 ms at 50,000; the profile places 97% of samples in `parse_object_untyped`. This is unnecessary quadratic work.

   Fuse nesting tracking into the real parser, or use an iterative parser/structural scan that supplies information the parser reuses. Do not simply delete the depth protection. The current long-ASCII-string parse profile places **87.8%** of main-thread leaf samples in the inlined preliminary depth scan; disassembly confirms the sampled offsets are its byte-at-a-time loop. The old checkout additionally performs a Serde validation pass, but the measured newer build has already removed that pass. A full JSON DOM is already avoided; replacing Serde DOM construction is not a new optimization for this measured build.

   Give string escaping an explicit short-word and long-vector implementation: SWAR for short strings, NEON/SSE for longer strings, copy clean spans in bulk, and handle quotes, backslashes, controls, and lone surrogates correctly. Long-string stringify places **83.7%** of samples in `write_escaped_string`. Its source comment says “SIMD-friendly”; the measured result shows that is insufficient. Keep the existing competitive escape-heavy behavior as a regression check.

   Replace per-number Rust `format!`/temporary `String` construction with a proven shortest-roundtrip formatter that writes to the destination or a stack buffer. Numeric stringify visibly spends its time in Grisu/general formatting, string writes, and allocation/free. Preserve ECMAScript notation thresholds, negative zero, subnormals, rounding, and non-finite handling. Benchmark the full formatter, not only a digit-generation microbenchmark.

2. **Build one guarded plain-data serializer around a shape plan.** A plan should encode ordered eligible keys, pre-escaped `"key":` prefixes, field offsets, likely value kinds, and nested plans. The loop then loads fields and emits bytes. Guards need to cover shape changes, property descriptors, prototype serialization behavior, getters, proxies, and `toJSON`; maintain invalidation information when those things change instead of rediscovering them per field. A fallback must preserve correct side-effect order and cycle behavior. This is especially valuable for tiny messages and homogeneous arrays.

   Perry already has homogeneous-array key-prefix templates and a `toJSON` absence probe. Extend these into a reusable plan with a safe lifetime rather than introducing a competing cache. The existing template cache is cleared at top-level calls and still routes through substantial generic machinery. Consolidating thread-local state into a per-call context also targets the small-value profile. A schema can specialize field operations, but it cannot excuse missing runtime guards.

3. **Extend shape-directed parsing to ordinary and nested data.** Learn or receive the expected shape, compare common keys directly in order, allocate the final known-size layout once, and populate slots without generic property insertion. Allocate arrays in efficient batches and share key/shape metadata. Handle unknown keys, reordered fields, duplicates, unexpected types, and heterogeneous rows through correct fallback behavior.

   Perry already exposes a schema-directed typed-array parsing path; measure and extend it rather than treating typed parsing as a new invention. The stronger AOT opportunity is to compile reusable parse/serialize plans from proven layouts or TypeScript types. Types can supply predictions; arbitrary input still requires validation, and ordinary `JSON.parse` must retain extra fields and preserve JavaScript semantics. A typed API with different validation/filtering behavior must be explicit.

4. **Make lazy and eager parsing share an efficient materializer.** Retain laziness for sparse inspection and valid forwarding cases. On full scans, construct the remaining records with the same shape/batch machinery as eager parsing instead of repeated generic property insertion or paying for a second complete parse. Preserve identity and mutations of already exposed records. Improve the small-array switch and make the 16 MiB decision reflect consumption pattern where evidence is available. None of this requires waiting for a collector rewrite.

5. **Reduce byte copies, temporary storage, and retained capacity.** Investigate bounded scratch reuse, chunked output construction, capacity selection, and direct/adoptable managed output buffers. Choose based on allocation and copy counts as well as wall time: an extra output-size pass can cost more than the copies it saves. Reuse encoding/escape facts discovered while scanning instead of rescanning entire strings. Smaller per-record metadata and shared shape information will also reduce collector work.

   For HTTP/logging/database paths, an additional byte-oriented API can serialize into a destination buffer or stream and avoid constructing an intermediate JavaScript string and re-encoding it to UTF-8. That can improve end-to-end CPU/RSS substantially, but it is a different API result and should be benchmarked separately from `JSON.stringify`.

V8 provides a useful reference for the plain-data architecture: its documented redesign combines guarded side-effect-free serialization, shape information, SWAR/SIMD string scanning, Dragonbox, and segmented temporary buffers. The reported improvement applies to its own benchmark, not to Perry's expected gain. [V8's implementation explanation](https://v8.dev/blog/json-stringify) supports the design choices. JavaScriptCore likewise has dedicated JSON parsing/stringifying infrastructure; Bun's general runtime marketing or nonstandard JSON helpers should not be confused with the built-in operations measured here. [JavaScriptCore serializer](https://github.com/WebKit/WebKit/blob/main/Source/JavaScriptCore/runtime/JSONObject.cpp) and [parser](https://github.com/WebKit/WebKit/blob/main/Source/JavaScriptCore/runtime/LiteralParser.cpp) are the relevant upstream references; they are architectural references, not a claim that current upstream exactly matches Bun 1.3.14.

I would implement the first batch as separate, reviewable changes: **wide-object indexing, vector string scanning, and fused depth handling**, with targeted correctness and CPU/RSS checks. Next come **the plain-data shape plan and allocation-free number formatting**, followed by **shared batch materialization and compiler-generated plans**. Use these benchmarks as gates, add varied real payloads and unique-message retention, and preserve explicit sparse-read/full-scan cases. Full-string JSON remains at least linear in bytes read or written; shape specialization removes avoidable work within that bound. Improvements from different hotspots are not multiplicative, and no whole-suite speedup is established until those changes are implemented and measured.

The sampling profiles used the same compiler/runtime artifacts with debug symbols retained; call-location instrumentation can slightly affect profile shares. They are directional evidence, kept separate from timed results. No performance claim here establishes complete ECMAScript conformance, latency percentiles, server throughput, Linux behavior, or performance of replacers, revivers, proxies, and arbitrary user callbacks. The probes cover the listed ordinary data cases. A small extra pretty-print diagnostic was also recorded, but is not included in the three-trial default standings.
