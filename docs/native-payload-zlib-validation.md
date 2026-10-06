# Native-payload zlib validation (#11919)

Functional acceptance record; final performance measurement and attribution
are still running. The code baseline is origin/main `78e2ab97e7`; the current
implementation is `df52c29508`, which contains that main revision.

The eleven codecs now use one runtime Transform state machine. Codecs own only
native workspace and bounded scratch behind PayloadVTable/StreamHooks, with
`links_owner: false`. Input is borrowed through B1 for one step, output is an
exact-size B1 Buffer, and destroy drops native workspace before queuing events.
Callback one-shots are traced runtime closure jobs. There is no new production
cache, family-name check, latch, or resource side table.

Deleted: stdlib `zlib.rs` (1,983 lines), `zlib/tables.rs`, its duplicate byte
output tests, zlib method/property dispatch and event-pump arms; the ext
provider's private stream/listener maps, pending events and raw callback queue,
agent ALL/MINE lifecycle, `scan_zlib_roots`, and handle dispatcher. The default
build links ext-zlib. The unused eager synchronous iterator conversion is also
deleted: Readable.from drives synchronous and asynchronous iterator sources
through its existing bounded, traced next() path.

## Census and source checks

The corrected pre-migration detector from `32be0a8ed1` scans current main; the
post-migration detector scans head. Keep the old factory recognizer when
measuring the old tree: the cleanup detector intentionally removes it.

| Source census | Current main | Head | Removed |
|---|---:|---:|---:|
| Native handle tables | 197 | 195 | 2 |
| Native handle producers | 183 | 161 | 22 |

The checked-in main ceilings are 196/183, rather than the corrected source
census. The ratchet and its sabotage self-test pass at 195/161. The Buffer
layout ratchet is 272 existing sites and zero new sites; its nine sabotages
turn red. The native result ledger passes at 214 rows and 193 providers.
Native ABI checking reports zero native class/width/arity/return mismatches
and zero unclassified signatures. Existing address-classification and file
size gate failures are compared against main, not represented as clean gates.

## Functional witnesses

| Witness | Evidence so far | Sabotage |
|---|---|---|
| Z1: 64 MiB random, slow pipe and pipeline | Final bytes/CRC/queue bound agree with Node; head false/drain counts 4096/4095; Node pipe 4096/4096, pipeline 4095/4095 | shared never_park: RED |
| Iterator source credit | Constructor does not exhaust source; paused source stops at HWM; collection retains queued byte owner | eager_iterator_from: RED |
| Z2: valid 97,222-byte gzip expanding to 100 MB | Clean 50 ms-per-chunk consumer: Node +11,587,584 RSS bytes, head +16,723,968; bytes/CRC/queue PASS | unbounded_output and exhaust_before_park: RED |
| Z3: write, pipe, pipeline, async iteration | 1,346 compressed bytes, CRC 2,811,409,573; decompressed CRC 311,501,110; each path's event trace agrees with Node | pipe_bypass_writing: RED |
| Z4: destroy during Brotli data | Native bytes become zero before GC, no later data, one delayed close and late listener | finalizer-only release and synchronous close: RED |
| Z5: six corrupt decoders | Node messages/code/errno/write callback and error→close ordering | actual codec error_as_eof and shared error_as_end: RED |
| Z6: eleven constructors | Identity, all stream instanceof relations, closed reset error and null handle | keep_handle_field: RED |
| Z7: keys, JSON and prototypes | Eleven families, zstd field order and Gzip subclass agree with Node | actual own_codec_methods and shared own_methods/enumerable_state: RED |
| Z8: 50k per codec plus 50k destroys | Final clean run: 600,000 created=dropped; warm RSS 55,042,048–56,832,000 bytes; diagnostic companion 43,978 full and 869 copying collections | finalizer-only release, skip_release_autodestroy and keep_step_closure: RED |
| Z9: moving collections in real codecs | Final seeds 1/7/11919: 11/12/17 copying minors and 17,024/17,263/17,497 moved objects; Node output agrees | hold_slice_across_push: RED |
| Full-size source under moving GC | Final 64 MiB pipeline passes with 9,182 copying minors and 145,135 moved objects after replacing eager source draining | source-credit sabotage above |
| Z10: real worker codec with a step queued | Native drop occurs during retired heap cleanup; no JS and main remains usable | drain_after_teardown/finalize: RED |
| Z11: subclass super._transform/_flush and JS Transform | Both Node-diffed; subclass transform called three times | hooks_first: RED |
| Z12: one-shots | 10k callbacks survive collection before execution; sync/callback byte oracles and throwing callback uncaught path agree with Node | raw_one_shot_callback: RED |
| LazyTransform flag | Existing mock family initializes once on first method/state access | eager_lazy_init: RED |

The 100 MB fixture is produced outside the consumer process, so its producer
allocation cannot hide decompressor RSS growth. The Rust bomb/churn fixtures
announce the generated-code write-barrier contract, because their JS stores
are runtime helpers that emit barriers. Both require copying minors to have
actually run. This changes no production GC or allocator policy.

The literal design fixture “1 KB gzip → 100 MB” is not representable with
DEFLATE's maximum expansion. The design's total-RSS bound below twice a 16 KiB
HWM also cannot include Node/Perry's runtime, collector and allocator footprint.
The standalone tight-loop Rust witness also exceeds its former 32 MiB
RSS assertion (final cold +63,741,952 bytes, three full and three copying
collections; the previous THP-off witness
reported +52 MB). That allocator-dependent assertion is replaced with per-data queue
and native-workspace checks, while keeping cold RSS in the evidence. The clean
slow-consumer TS witness is measured independently. Queue/workspace bounds
and clean-process RSS are measured separately; no global
memory-policy workaround or masked producer baseline is used.

## Current-main comparisons

The release build and the four final crate suites have zero new failures:

| Suite | Main passed / failed / ignored | Head passed / failed / ignored |
|---|---|---|
| codegen | 2042 / 0 / 1 | 2042 / 0 / 1 |
| runtime | 5195 / 1 / 5 | 5201 / 0 / 5 |
| stdlib | 257 / 2 / 0 | 250 / 2 / 0 |
| ext-zlib | Original provider fixtures are superseded | 23 / 0 / 1 |

The two stdlib failures are the existing Symbol and closure side-table teardown
assertions, identical on main. Main's RSS witness passes in a clean standalone
child (3,493,888-byte warm growth), but fails after the full suite's retained
~4.3 GB heap (4,333,568-byte growth). Head's full runtime suite passes that
witness. All thirteen shared stream sabotage children and all nine real-codec
sabotage children turn red. Lazy initialization, eager source exhaustion and
worker post-finalize draining are also rejected by their dedicated children.
The final test-only stale-slice fault holds a moving string's actual interior
pointer, instead of relying on reuse of a freed materialization scratch block.

Production program binaries are frozen at `df52c29508`; the subsequent changes
are test-only fault/bounds improvements and a manifest comment correction.
The final release rebuild and unit suites include those changes. No production
GC, allocator, codec or stream behavior changed after the program builds.

The final gap union has 58 cases, 38 main passes and 54 head passes, with
zero main-pass→head-fail regressions. #11620 passes. The final rerun includes
the bounded synchronous-source fix and a ±1 drain oracle that prints the
actual counters when requested.

## Programs and measurement

Main program binaries are built separately in `base/target`; head uses
`target`. Node is pinned to 26.5.1. Main agrees for hello, worker_heavy, Zod
x5000, qs parse/stringify, commander, Effect, tsc transpileModule and fastify.
Main buffer_heavy exits 1 at its gunzip pipe (#12091). Final head hello,
worker_heavy, buffer_heavy, Zod, qs parse/stringify, commander, Effect, tsc
and fastify agree with Node. Only Effect's printed
wall-clock fields are normalized; its result is checked.

Performance runs use n=5 interleaved arms on qb6 CPUs 56–63, ASLR off, under
MEASURE.lock, with a same-binary control for noise. Primary instructions and
RSS samples have tracing off. Full-collection counts are from paired diagnostic
companions, explicitly not the primary processes. Every primary sample also
checks Node output and records its binary SHA-256. RSS steps near 2 MiB require
a THP-disabled rerun. Final medians, noise floors and delta explanations are
pending; no old-main performance measurements are acceptance claims.
