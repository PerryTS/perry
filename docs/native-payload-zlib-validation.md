# Native-payload zlib validation (#11919)

Final head is rebased onto origin/main `31bbc3da48`. Final current-main
acceptance is **blocked**: both main and the rebased lane fail release builds
and the four-crate test command with the same 15 perry-parser SWC errors.
Tests do not start. The isolated ext-zlib release check passes with Brotli 9.

The functional and performance tables below are historical evidence against
`78e2ab97e7`, not acceptance claims for `31bbc3da48`. Their measured production
implementation was `df52c29508`; final suites included test-only improvements
in `f58706d1e7`. Those commits were rewritten by the final clean rebase.

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

## Last buildable-main comparisons (`78e2ab97e7`)

The release build and the four final crate suites have zero new failures:

| Suite | Main passed / failed / ignored | Head passed / failed / ignored |
|---|---|---|
| codegen | 2042 / 0 / 1 | 2042 / 0 / 1 |
| runtime | 5195 / 1 / 5 | 5201 / 0 / 5 |
| stdlib | 257 / 2 / 0 | 250 / 2 / 0 |
| ext-zlib | 17 / 0 / 0 | 23 / 0 / 1 |

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
a THP-disabled rerun. No primary program RSS delta is near 2 MiB.

| Program | Instructions, billions main → head | Delta | RSS MiB main → head | Full collections main → head | Same-binary instruction range |
|---|---:|---:|---:|---:|---:|
| hello | 0.001354424 → 0.001356098 | +0.1236% | 15.871 → 15.172 | 0 → 0 | 0.0024% |
| tsc | 26.974285 → 26.981391 | +0.0263% | 249.562 → 249.664 | 1 → 1 | 0.0663% |
| Zod x5000 | 14.640727 → 14.642836 | +0.0144% | 51.258 → 50.699 | 0 → 0 | 0.0807% |
| qs parse | 23.404537 → 23.403250 | −0.0055% | 56.258 → 56.203 | 0 → 0 | 0.0304% |
| qs stringify | 59.916768 → 59.902444 | −0.0239% | 54.125 → 54.223 | 0 → 0 | 0.0478% |
| commander | 30.675221 → 30.690643 | +0.0503% | 51.328 → 51.719 | 1 → 1 | 0.0283% |
| fastify | 112.773449 → 112.794518 | +0.0187% | 246.203 → 245.445 | 1 → 1 | 0.1486% |
| Effect | 25.694203 → 25.689824 | −0.0170% | 189.090 → 188.430 | 2 → 2 | 0.2813% |
| buffer_heavy | main fails → 14.673203 | not gated | — → 91.668 | — → 36 | 0.0073% |
| worker_heavy | 2.130656 → 2.117234 | −0.6299% | 221.012 → 221.438 | 42 → 41 | 1.6688% |

The instruction floor is the larger range of the two same-binary control
arms, each with five samples. tsc, Zod, both qs programs, fastify and Effect
are inside that floor. worker_heavy is also inside its 1.6688% control range:
worker scheduling changes the amount of collection work (the control full
counts also vary), so its negative delta is not claimed as a speedup.

Hello is above its floor by 1,674 instructions. Five interleaved diagnostic
runs count instructions at actual primary-binary main/call boundaries:
pre-main increases by 1,704, while main startup/body call scopes are unchanged
apart from single instructions and GC initialization noise. ELF packed relative
relocations increase from 30,550 to 30,666 pointer fixups (1,448 to 1,452 encoded
RELR words), with identical explicit relocation and PLT counts. Linked runtime
pointer metadata accounts for the bounded, once-per-process loader increase;
it is not per-operation codec or GC work. The fix-forward is to reduce linked
runtime metadata and relocation entries with finer section granularity, without
adding caches or family checks. The diagnostic symbol copies have identical
.text and build IDs to the primary binaries.

Commander remains above the initial instruction floor (+0.0503%). Its n=5
THP-disabled rerun is +0.0371%, with 1 full and 124 copying minors in both arms,
zero AnonHugePages, and RSS 25,368 → 25,740 KiB. Its five paired GC traces have
identical copied bytes/objects, layout scan work and root slot counts. Five interleaved primary-binary copying-collection scopes measure
304,867,577 → 304,902,018 instructions, with 124 calls in both arms: only
34,441 extra instructions, far below the program delta. The synchronous full
mark/sweep entry is not called in this workload; the full trace event comes
from the incremental path, so this scope is not claimed to cover all GC.

The shared receiver extractor in node_stream::this_value now calls
ensure_lazy_stream, including from EventEmitter's on/once/emit/listener
methods. Commander repeatedly registers option listeners. The new negative
StreamHooks check and its caller's argument preservation are a bounded
per-emitter-operation cost from implementing lazy state generically. An
n=5 interleaved same-primary-binary diagnostic replaces only this function's
entry with ret in a child process's private text: commander has no lazy native
streams, and every original/patched output agrees with Node. Its medians are
30,693,773,389 → 30,685,279,123 instructions, an 8,494,266-instruction difference
(+0.0277% for the check). This is an attribution estimate, not a new primary
A/B: its original-arm range is 0.0835%, so the number is not claimed as an
exact isolated instruction budget. The check's demonstrated path plus caller
argument preservation explains the bounded architectural cost; subtracting the
estimated check cost from the original 15,421,713-instruction program delta
leaves 6,927,447 (0.0226%), inside the initial 0.0283% same-binary floor. The
fix-forward is to put initialization at stream state accessors and stream
operations, rather than at the shared EventEmitter receiver extractor; keep
one stream state machine, without a cache or family-name guard.

RSS attribution uses separate n=5 interleaved smaps_rollup samples, without
changing either executable. For commander, the sampled peak grows by 460 KiB:
420 KiB is file-backed PSS and 40 KiB anonymous. Zod's sampled peak decreases
380 KiB, with file PSS down 391 KiB and anonymous up 8 KiB. qs parse/stringify
have identical anonymous medians; their file-backed changes are +260/+231 KiB.
These are bounded code/page residency differences from the linked provider and
stream machinery, rather than retained codec workspace or extra collections.
Sampled instantaneous peaks are diagnostic companions, not replacements for
the primary GNU time high-water marks.

The tsc, fastify, Effect and worker primary RSS differences are within their
same-binary controls' RSS ranges (roughly 2.5, 4.1, 2.5 and 16.8 MiB). Their
full counts are unchanged except for worker scheduling variation. Hello is
shorter than the smaps polling interval, so its GNU time RSS difference is
not attributed from incomplete smaps snapshots. Five synchronized samples of
the kernel's VmHWM at its actual main/call boundaries measure 15,756 → 15,808
KiB: +52 KiB, rather than the first pass's −716 KiB. The initial negative RSS
delta is not reproducible and is not claimed as a saving; the synchronized
execution shows a small code/page-footprint difference, zero full collections
and unchanged execution scopes. Diagnostic text breakpoints also make these
kernel samples companions rather than replacement primary measurements.

### Streaming micro witnesses

| Workload | Instructions main → head | RSS KiB main → head | Full collections main → head | Comparison |
|---|---:|---:|---:|---|
| 50k Gzip Transforms | main exits 13 → 126,003,469,409 | — → 52,760 | — → 432 | main leaves top-level await unsettled; no valid A/B |
| 64 MiB Gzip pipeline | main exits 13 → 26,854,768,065 | — → 89,964 | — → 8 | main leaves top-level await unsettled; no valid A/B |
| 64 MiB Hash write/pipe | 262,719,147 → 262,734,308 | 36,896 → 36,944 | 0 → 0 | +0.00577%, within 0.00973% control floor |

All three final head witnesses agree with Node; n=5 interleaved samples (head
only for the two baseline failures). The Hash witness uses the supported
write/digest plus readable pipe interface; pipeline into Hash lacks _transform
in both arms. This lane implements lazy state generically, but does not port
crypto codecs to StreamHooks. Raw rows preserve cycles/task-clock measured on
qb6 measurement cores, hashes, output checks, huge-page samples and companion
collection counts. They are kept with the lane evidence under the approved
scratchpad directory.

## Historical performance acceptance and evidence

The two instruction increases above the initial control floors are explained
and bounded: hello's once-per-process relocation work and commander's generic
lazy-state guard on emitter operations. Both are below 0.5%, with the named
fix-forward plans above. The implementation unifies codec ownership and stream
state, removes handle tables/raw callbacks, and adds no production cache or
resource side table. This lane makes no global GC, allocator or THP change.

Evidence: `/Users/amlug/projects/perry/secret-tests/scratchpad/codex-small/zlib2/`
contains the source checks, crate/gap/witness logs and an `evidence/` copy of
host program hashes, raw primary/control/THP rows, companion collection counts,
profiles and attribution JSON. `final-program-report.json` recomputes medians
from complete raw rows and checks each measured executable's SHA-256. The
symbol copies used for diagnostic profiles have identical .text; the newer
commander shadow's build-id note is aligned with its primary after validating
that equality, because its compile stamp differs. Primary binaries are unchanged.
The compressed bundle is refreshed after each commit milestone.

Unmet literal requirement: the design's total-process bomb RSS limit. The
valid fixture, observed native/queue bounds, Node comparison, cold and slow
consumer RSS, and THP-off diagnostic are recorded above rather than asserting
that a 1 KB → 100 MB gzip or a total-RSS <32 KiB process is possible. The four
remaining gap failures also fail on main. Baseline buffer_heavy and the two
streaming gzip micro workloads fail functionally, so they supply no gated A/B.

## Final rebase and upstream verification block

The final fetch found nine dependency-only main commits after `78e2ab97e7`:
SWC common/AST/parser/codegen major updates, llvm-sys 231, Brotli 9, thiserror 2,
notify 8 and a minor/patch dependency group. The lane rebased cleanly onto
`31bbc3da4814720bcd12fb73044ff3f49362289b`; no project versions were bumped.

| Current-main check | Main `31bbc3da48` | Rebased lane |
|---|---|---|
| Requested four-package release build | FAIL: 15 perry-parser errors | Same 15 errors |
| runtime/stdlib/codegen/ext-zlib test command | Compile fails; no tests run | Same compile failure |
| Isolated ext-zlib release library check | Not used as an acceptance substitute | PASS, including Brotli 9 |
| Final gap/Node program rerun | Blocked by compiler build | Blocked by compiler build |
| Final n=5 current-main A/B | Blocked | Blocked |

`current-main-build-errors.json` compares all fifteen diagnostic codes,
messages and source locations and asserts equality between arms. The parser
source is unchanged by this lane. Failures are E0053 visitor signature type
mismatches, E0308 SWC type mismatches and E0599 missing VisitMutWith methods.
The host has LLVM 21/22 and no LLVM 23; parser failures occur first, so the
missing newer toolchain is not claimed as the observed failure. Logs are
`main-31bbc-build.log`, `head-31bbc-build.log`, `main-31bbc-crates.log`,
`head-31bbc-crates.log` and `head-31bbc-ext-check.log` in the evidence copy.

No dependency rollback or unrelated parser migration is included. Current-main
crate/gap/oracle/performance verification must be rerun after the upstream
SWC dependency set builds; the historical program table must not be used to
ship against the newer dependency baseline. The canonical bundle is rebased
and refreshed against the final origin/main. Owned targets are deleted after
preserving the receipts.

## Landing blockers (2026-10-07, main `75c38e93ff`)

Two blockers remained after the SWC dependency set built on main.

**Thread-exit timers.** `external_native_dispatch_routes_async_gzip_callback`
threw `TypeError: value is not a function` when it ran right after
`external_dispatch_accepts_options_and_honors_level_zero` on a new test
thread. A libtest thread is not a worker, so it acts for the primary agent
while allocating in its own arena. The first test's callback queued an
async-hooks destroy immediate that the test never ran. The entry stayed in
the process-global timer store after the thread exited, and the next
thread's check phase called it. Before #12132 the freed arena still held a
dead closure, which ran. After #12132 the next thread reused the
addresses, so the entry named some other object. `timer/store.rs` now
releases such entries through the thread-exit range hook, like the other
process-global tables that hold arena addresses. The zlib runtime tests run
as their own agents (#11417's pattern), so concurrent tests no longer share
one event loop. A plain-thread regression test turns red when the hook is
removed: the dead thread's callback runs, 2 instead of 1. Workers were
never affected, since `retire_agent` purges their partition. The
Worker one-shot gap test agrees with Node in both arms.

**buffer_heavy.** The default build measured 14.94G instructions against
main's 10.43G. Profiles of both arms attribute the +4.52G as follows:

| Cause | Delta |
|---|---:|
| C zlib deflate (`deflate_slow`, `fill_window`, `longest_match`) vs miniz | +2.05G |
| crc32fast without runtime CPU detection (flate2's `zlib` feature dropped `runtime_detection`) | +0.57G |
| Inherited [[Get]] misses on private `__perry…` keys walking the zlib Transform's prototype chain (~60K instructions per data event) | +1.1G |
| Remaining stream-state property traffic | +0.8G |

The C encoder did not buy Node byte parity either. For a 10 MB tar-like
input, Node, C zlib and miniz produce three different gzip outputs at levels
1, 6 and 9. Only repetitive text matched. The stream encoder now drives the
same pinned miniz_oxide backend as the one-shots and writes the gzip and
zlib wrappers itself. `params()` continues with a fresh compressor after the
runtime's sync flush. Private stream state is read as an own property.

| Program (n=3 medians, instructions:u) | Main | Head | Delta |
|---|---:|---:|---:|
| tsc transpileModule | 9,950,473,517 | 9,948,963,910 | −0.02% |
| Zod ×5000 | 14,647,195,385 | 14,651,398,876 | +0.03% |
| qs parse | 23,445,458,387 | 23,439,335,735 | −0.03% |
| qs stringify | 59,859,550,584 | 59,858,313,454 | −0.00% |
| commander | 7,558,102,085 | 7,559,707,742 | +0.02% |
| hello | 1,208,327 | 1,210,026 | +0.14% |
| fastify inject | 112,770,470,939 | 104,023,302,520 | −7.76% |
| buffer_heavy (default build) | 10,428,514,816 | 11,206,978,973 | +7.46% |
| buffer_heavy (no auto-optimize) | fails (`value is not iterable`) | output = Node | — |
| worker_heavy | 2,095,100,814 | 2,100,664,577 | +0.27% (same-binary range 1.3%) |

All outputs equal Node's. A plain JS `Transform` driven by `for await`
(7,488 16 KiB chunks) falls from 4.36G to 3.36G instructions (−23%).

The remaining +0.78G on buffer_heavy is the runtime stream's per-chunk state
traffic. Main's stdlib zlib handle bypassed this machinery. Profiles put
about 90% of it in named-property access to the stream's private state:
`hidden_key` interning through a SipHash map, own-key lookups on a wide
instance, and stores. That is the same machinery every JS Readable and
Transform uses. The fix-forward is to keep runtime stream state in a native
record reached from the object's descriptor, not in `__perry…` properties.
This is above the 0.5% bound, so it goes to the owner.

| Suite (release) | Main passed / failed / ignored | Head passed / failed / ignored |
|---|---|---|
| ext-zlib, default threading | 17 / 0 / 0 | 24 / 0 / 1 |
| runtime, `--test-threads=1` | 5208 / 0 / 5 | 5213 / 0 / 5 |
| stdlib | 257 / 2 / 0 | 250 / 2 / 0 |
| codegen | 2042 / 0 / 1 | 2042 / 0 / 1 |

The two stdlib failures are the same thread-exit side-table assertions in
both arms. The gap union is 59 cases: 39 pass on main and 53 on head, with
zero main-pass→head-fail regressions. The stream sabotage meta-test (13
children), the codec sabotage test and the worker-finalize sabotage pass,
so every child turns red. The Buffer layout ratchet has zero new sites. The
thread-exit gate reports only main's existing problem.
