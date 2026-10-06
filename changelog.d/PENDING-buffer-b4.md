Buffer B4a/B4b/B4c implementation milestone: Buffer, every TypedArray kind, DataView, ArrayBuffer, SharedArrayBuffer and NativeArena typed views now share a 16-byte cell. This milestone is not yet approved for landing: the remaining census conversions and final A/B verification are in progress.

The GC header remains at p-8. The cell contains element length at p+0, owner capacity or view byteOffset at p+4, and its sole traced link at p+8. Owner bytes start at p+16, inline or through an out-of-line data pointer. The type byte encodes brand and owner/view role; the reserved header bits encode out-of-line/length-tracking, resizable, detach and nested pins. Byte cells are born old and never move.

An owner's link is initially zero. Its first named property attaches a null-prototype ordinary Object bag through one barriered store. The bag's shape and property storage hold named descriptors and the four hidden internal keys for ArrayBuffer identity, a bagged view's owner, pin overflow and custom prototype. Attachment keeps inline bytes in place, including with 31 outstanding pins. Pin 32 uses the bag and decrements cleanly. A real ArrayBuffer view supplies `.buffer`; an ArrayBuffer owner returns itself. No trailer or fixed `.buffer` slot was introduced.

Views flatten their owner and retain only owner/bag, byteOffset and element length. Access computes detach and out-of-bounds from the current owner. NativeArena is an out-of-line owner with brand 18; dispose detaches its store. Its generation counter and three registries are removed, and typed views are ordinary 16-byte view cells. POD views retain their own existing layout/owner contract without cached data or generations. No allocator, placement, runtime arena/region, fetch-body, stream or ext-zlib implementation was edited. The codegen region-loop edits only adapt emitted length reads to the common ABI's independent data and length locations.

Small Buffer copy/from/allocUnsafe/concat allocations use one placement call `(brand, init, len)` and a traced per-agent pool owner. Zeroed Buffer.alloc, Uint8Array and ArrayBuffer do not pool. The pool uses the runtime-settable Buffer.poolSize property and eight-byte alignment, with the strict half-pool threshold. The pinned Node 26.5.1 host reports a default poolSize of 65536; the explicit 8192 pool witness checks offsets, identity, rollover and collection. A retained small pooled view keeps its pool alive. This follows the answers-file pool addendum; the retention cost still needs measurement alongside the cell-size cost.

Emitted typed and byte reads now guard one header word and resolve owner data. Fixed unbagged views add the owner loads; bagged, tracking, resizable, detached, shared and NativeArena views use the runtime resolver. Shared element access retains atomic lane width. Loop parameters hoist data/length while rooting both the receiver and owner, and refresh after collecting calls and loop polls. The loop body remains a bounds check and element load. The separate payload-kind/storage loads, old block-size arithmetic and both address-cache exports are removed. A test-only missing-owner-root sabotage fails the IR rooting invariant.

## Deleted machinery

| Machinery | Replacement |
|---|---|
| VIEW_REGISTRY, BACKING_TO_VIEWS | Fixed view cell and current owner state |
| RESIZABLE_BUFFER_MAX | Owner header bit and capacity/maxByteLength |
| BUFFER_AB_ALIAS | Hidden shaped property containing a real ArrayBuffer view |
| TYPED_ARRAY_VIEW_META, VIEW_META_COUNT | View role, byteOffset and length-tracking bit |
| RESIZABLE_BUFFER_EVER_MARKED, BUFFER_AB_ALIAS_EVER_SET | Owner header/link |
| PERRY_U8_INLINE_CACHE, PERRY_TA_KIND_CACHE | Common emitted header guard |
| Buffer own-props map/latch; TYPED_ARRAY_OWN_PROPS/emitted latch; TYPED_ARRAY_NO_EXTEND | Ordinary bag and freeze/seal/extension header bits |
| NativeArena's three registries, count and generation | Native owner header and traced view owner |
| Lazy TypedArray .buffer copy/resolved-storage state | Aliasing ArrayBuffer view over the original store |
| Residual-prototype table entries for byte cells | Hidden prototype property |

Crypto metadata registries and the foreign-finalizer shutdown inventory stay as explicitly required by the answers file. The active allocation pool has one traced owner root and one numeric cursor; it is neither an address table nor an access cache.

## Census

| Inventory | Inherited | Current milestone |
|---|---:|---:|
| Historical design production/test rows | 350 / 230 | Historical census retained |
| Explicitly closed rows in inherited preparation ledger | 82 | All 580 historical rows now have a disposition in buffer_b4_census_audit.tsv |
| check_buffer_layout.py source debt | 282 on current main / 279 at lane start | 18, all in protected fetch/stream files |
| Requested five address tables / two latches / two caches | 5 / 2 / 2 | 0 / 0 / 0 |

Raw-pointer consumers now use scoped byte slices, retained read leases or pins, including numeric methods, copying, codecs, concatenation, formatting, strict Uint8Array conversion and stdlib zlib. Typed transforms retain receiver/callback/candidate roots. ArrayHeader assignments caught by the original broad Buffer-header regex were corrected as census false positives; these are not claimed as byte-lifetime conversions. The source ratchet is not yet at zero. Protected fetch and stream rows remain for their owning lanes; owned fixtures have scoped/retained conversions; the source ratchet passes at this milestone. Deleted root-holder inventory entries are removed, and the active pool scanner is recognized.

## Tests and sabotages

All results below are intermediate Linux checks, with at most eight Cargo jobs, CPUs 0–55 and single-threaded runtime tests. Final verification must be repeated on the rebased current-main head and coherent release archives.

| Contract | Witness / sabotage | Intermediate result |
|---|---|---|
| Shared cell, header brands, stable named-property attachment | Common-cell and 31-pin witnesses; attach_moves_bytes | PASS / RED |
| Pin overflow | Pin 32, hidden property, complete unpin; pin_overflow | PASS / RED |
| Pool identity, alignment, threshold, rollover and GC root | Real pool owner; pool_identity, pool_root | PASS / both RED |
| Retained source copies and exact typed lanes | Forced full GC and root marks; copy_root, copy_kind, typed_copy_root, shared_lane_copy | PASS / all RED |
| Retained typed predicate receiver/callback/BigInt candidate | Collecting predicate; find_receiver_root | PASS / RED |
| Owner resize/detach, traced view edge, transfer and u32 admission | Owner/view witnesses; owner_check, detach_mark, view_edge, transfer_copy, u32_admission | PASS / all RED |
| Native view owner/offset and persistent symbol prefix | native_resolution, symbol_header | PASS / all RED |
| B4 focused runtime suite | 11 tests; 18 sabotage invocations | PASS / all 18 RED |
| Hoisted owner root | Statepoint-root IR invariant; hoist_owner | PASS / RED |
| Stdlib / FFI / ext-zlib | Current-main merge | 257 PASS + 2 inherited FAIL / 75 PASS / 17 PASS |
| Whole codegen unit suite | Development build, latest emitted ABI | 2045 PASS, 0 FAIL, 1 ignored |
| Runtime suite | Current-main merge, two independently reproduced crashing main tests excluded | 5200 PASS, 2 FAIL, 4 ignored; both failures reproduce on current main (5194 PASS, the same 2 FAIL, 4 ignored) |
| Source ratchet sabotage | Ten planted source couplings | PASS / all ten RED |
| GC header constants / root-holder inventory | Executable gates | PASS at intermediate snapshot |
| View owner live across forced collection in emitted loop | test_buffer_b4_hoisted_view_gc.ts | Uint32 parameter/local witness PASS: output 2080 twice, 32 collections including 16 full; Buffer parameter/subarray variants added |
| Every pointer-tagged value has p-8 header, whole module | Existing native/proxy registry IDs remain pointer-tagged nonheap words | Not established; broader representation gap recorded below |

The residual-prototype relocation test SIGSEGV reproduces on pristine current origin/main in the separate main-target build. The duplicate-key JSON fixture omitted production's shape forwarding scanner after its isolation guard cleared the registry. Its stale family index was retired when the old keys address was recycled into bytes whose type became recognizable in the expanded type range. Registering the existing shape forwarding scanner restores the production root set and the fixture passes on the rebased head; no production shape machinery changed. Other failures from the earlier release run included stale common-kind/length expectations, a cross-thread SAB fixture retaining a dead agent wrapper, a thread-transfer diagnostic expectation, and a cwd-dependent URL fixture. These have source fixes or a corrected execution environment and need the fresh full rerun. No failure is waived as a passing result.

The whole-module pointer-header requirement is stronger than the current NaN-box representation: value/addr_class explicitly reserves pointer-tagged bands for proxy and native-resource/fetch registry IDs with no addressable p-8 header. Persistent Box-leaked symbols now have a real leaf prefix, and every byte family uses the common GC allocation prefix, but those facts do not establish the whole-module invariant. No fabricated subset test is reported as proving it.

## Program and kernel acceptance

| Required real programs | Output == Node | n=5 interleaved instructions:u, RSS, full collections |
|---|---|---|
| tsc, Zod x5000, qs parse/stringify, commander, hello, fastify | Pending coherent release builds | Pending |
| Effect | Policy driver copied; effect@4.0.0-beta.83 installed; Node passes | Pending |
| buffer_heavy, worker_heavy | Current-main status pending; known #12091/#12092 baseline exceptions | Pending |

| Hard-gate kernel | Output parity | Flat-or-better |
|---|---|---|
| matmul | Pending | Pending |
| prime_sieve | Pending | Pending |
| bench_buffer_readwrite | Pending | Pending |
| ECS u32 | Pending | Pending |

The approved +8 B per small owner is implemented; its RSS effect has not yet been measured. Full runtime/stdlib/codegen/ffi/ext-zlib tests, the buffer|typed|dataview|arraybuffer|zlib|crypto|tls|net|http|fs gap comparison, full program measurements, typed-kernel hard gate, THP-off checks where indicated, and every material delta's mechanism remain required. There is no performance acceptance or landable-head claim in this milestone.

Milestone compile check: `cargo check -p perry-runtime -p perry-codegen -p perry-stdlib --tests` passes on the Linux host. The last obsolete test-only byte-cache declaration was then removed. No version fields changed.

Follow-up lifetime audit: tracked locals now explicitly retain receiver and owner starts across safepoints, including paths where the boxed binding is otherwise consumed only through a hoisted data slot. Their independent IR sabotage is added. Empty reserved owners preserve the real pin address rather than a zero-length slice sentinel; the large shrink/regrow witness now passes. The canonical DataView byte-allocation brand creates an ArrayBuffer owner plus a 16-byte view. Four formerly incidentally covered scalar/native-tape inventory entries have researched non-GC-edge verdicts. The separate current-main coherent release build passes.

Census follow-up: the historical ledger has 310 creation rows reserved for B3 placement policy and 270 other rows: 21 unified emission, 122 scoped consumers, 61 canonical byte operations/fixtures, 29 canonical FFI copies, four already scoped protected zlib consumers, eight layout-authority rows, 13 current owner-extent/ABI assertions, five generic-array/arena false positives, one owned transcode copy, one leaf raw ABI export with no emitted callers, and five protected-owner debt rows. The current executable source ratchet covers 18 protected fetch/stream sites; it is not zero and none are hidden behind new exemptions.

Follow-up compiler validation: the 2,045 unit tests pass with retained local owners, including immutable aliases and the existing last-use raw-call sabotage. Integration IR readers now recognize the canonical data resolver and keep their unrelated-pointer and unchecked-access controls. Test artifacts are held stable during full runtime runs; a replaced-executable run was stopped rather than counted. Main reproduces the stream class fixture's debug-assert abort at class id 0xc0000001.

Final-verification progress: all current-main codegen integrations pass (including 47 buffer proof and 292 regression tests); both stdlib failures reproduce on main. Main and head runtime failures are the same z8 churn RSS threshold and perex program-cell padding comparison. Initial n=5 kernels: matmul −0.000035%, prime_sieve +0.990946%, bench_buffer_readwrite −0.001291%, ECS u32 +230.990027%; the hard gate is RED. The common-cell region length adapter introduced helper calls on sealed views and blocked loop hoisting; it now loads the canonical independent length slot for sealed views. The prime loop assembly is unchanged; sparse header admission adds metadata loads in runtime classification. A compile-time dense prefix derived from the existing descriptor authority is being verified. No cache or side table is restored. Initial hello is +296 instructions (+0.021367%); explanation and final measurement remain pending.

Current-main real-program outputs pass for all required programs, including tsc, Effect, buffer_heavy and worker_heavy. Both heavy drivers are included in the final gate; their earlier baseline exceptions have cleared. Main tsc passes with eight unit jobs and a 45-minute compile limit. Head outputs have passed for tsc, Zod x5000, qs parse/stringify and commander; the final compiler must repeat these checks.

Parameter/kernel milestone: declared Buffer parameters use the common-cell admission and exact receiver/owner roots instead of the legacy raw data-pointer preheader. Their numeric readers and integer element accesses consume the retained data/length slots, with normal dispatch on unsupported headers and named-method shadows. Writes also check the receiver's current frozen bit. The old bounded-parameter IR test incorrectly required the whole function to have no cold fallback; it now proves that both admitted read and store blocks contain no calls and that the common hoist supplies them. All 2,047 codegen unit tests and 47 Buffer-proof integrations pass, including the positive numeric-reader witness and its missing-owner-root sabotage. The forced-GC typed/Buffer/subarray/bagged-view driver passes with 48 collections, 24 full, on the preceding release compiler; it must repeat after the final compiler build.

The first head gap run was discarded because rsync removed output directories under src while it ran, allowing empty comparisons. The driver now writes outside the synced source tree and requires the complete selected case count. A subsequent valid run was stopped explicitly by its own PID tree before the final parameter compiler update; neither partial run is counted as final evidence. Final gap comparison and the corrected kernel/RSS measurements remain pending.

Owner-retention milestone: specialized typed-array callees bind their existing exact parameter root and retain it across collecting calls; the witness and independent `spec_owner` sabotage pass under both lowerings. The latest codegen suite is 2,049 PASS, zero FAIL, one ignored, with all 47 integration artifacts passing. The latest runtime run is 5,204 PASS, one inherited FAIL, four ignored, with two independently reproduced main crashes excluded. The remaining failure is the perex program-cell padding comparison; the z8 RSS check passes on this run. Stdlib has 257 PASS and the same two failures as main; FFI has 75 PASS and ext-zlib 17 PASS. This establishes zero new failures, rather than an all-tests-pass claim.

The inline pinned-detach page witness and its forced-decommit sabotage pass. The focused B4 suite now has 12 tests and 19 RED sabotages. AddressSanitizer passes 35 selected byte/backing/header tests, including the 19 B4, eight B1 byte-lifetime and one header-admission sabotages. ASan uses the system allocator so native backing frees are visible; leak scanning is disabled for the runtime's immortal allocation scope. Its separate target was cleaned after saving results. The real streaming-zlib callback witness matches Node: detaching the owner during the output callback leaves the result valid and the input length zero.

The THP-off retained-small-owner measurement has five interleaved trials, zero huge pages and one full collection in each nonzero arm. Increasing the retained count from 131,072 to 262,144 increases the head-minus-main RSS difference by exactly 1,024 KiB: 8.00 B per added owner. Absolute differences are +508 KiB and +1,532 KiB respectively; the fixed offset is still being investigated. This confirms the incremental approved header cost, but does not yet explain every absolute RSS term. Final kernel/program acceptance and the complete final gap comparison remain pending.
