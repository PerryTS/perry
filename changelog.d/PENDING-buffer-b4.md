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

Production head 493a683ed7 is merged with origin/main 78e2ab97e7. Linux checks use CPUs 0–55, at most eight Cargo jobs and one runtime test thread. Full evidence is in scripts/fixtures/buffer_b4_final_tests.json. All 56 Cargo-reported test artifacts were compared; the latest codegen rerun supersedes the earlier compiler artifact.

| Suite | Main | Head | New failures |
|---|---|---|---:|
| Codegen units | 2,042 PASS, 1 ignored | 2,049 PASS, 1 ignored | 0 |
| Codegen integrations | All 47 artifacts PASS | All 47 artifacts PASS | 0 |
| Runtime units | 5,194 PASS, 2 FAIL, 4 ignored | 5,204 PASS, 1 FAIL, 4 ignored | 0 |
| Stdlib units | 257 PASS, 2 FAIL | 257 PASS, same 2 FAIL | 0 |
| FFI units | 75 PASS | 75 PASS | 0 |
| Ext-zlib units | 17 PASS | 17 PASS | 0 |
| Runtime Android/TLS integration | PASS | PASS | 0 |
| AddressSanitizer byte/backing/header suites | — | 35 PASS | 0 |

Both arms exclude two crashing runtime tests independently reproduced on pristine main: residual_prototype_relocation::test_residual_prototype_owners_of_every_movable_kind_survive_a_copying_minor (SIGSEGV), and node_stream::state_tests::stream_dynamic_instanceof_follows_node_stream_inheritance (class-id debug-assert abort). The remaining runtime failure is regex::perex_owner::program_cell_tests::program_cell_bytes_do_not_carry_the_previous_occupant, a padding comparison that fails on both arms. Main's z8 churn RSS check failed on that run and passes on the latest head run. Stdlib's thread-exit symbol/closure side-entry failures reproduce on main. These are zero-new-failure results, not a claim that every test passes.

| Contract | Witness / sabotage | Result |
|---|---|---|
| Shared cell, brands, named properties on 31-pin owner | attach_moves_bytes | PASS / RED |
| Pin overflow through hidden shaped property | pin_overflow | PASS / RED |
| Pinned inline detach defers page decommit through last unpin | inline_detach_decommit | PASS / RED |
| Pool identity, alignment, threshold, rollover and traced owner | pool_identity, pool_root | PASS / both RED |
| Retained source copies and exact typed lanes | copy_root, copy_kind, typed_copy_root, shared_lane_copy | PASS / all RED |
| Collecting typed predicate keeps receiver, callback and BigInt candidate | find_receiver_root | PASS / RED |
| Resize/detach, view edge, transfer and u32 admission | owner_check, detach_mark, view_edge, transfer_copy, u32_admission | PASS / all RED |
| Native owner/offset and persistent symbol prefix | native_resolution, symbol_header | PASS / both RED |
| B4 focused runtime suite | 12 tests; 19 planted sabotages | PASS / all 19 RED |
| Existing B1 byte lifetime/borrow contracts | 8 tests; 8 planted sabotages | PASS / all 8 RED |
| Header admission | All 256 type bytes; accept retired type | PASS / RED |
| Ordinary and local hoisted owner roots | Independent IR live-root controls | PASS / RED |
| Specialized typed callee retains its exact owner parameter | spec_owner, both lowerings | PASS / RED |
| Source layout couplings | Ten planted couplings | PASS / all ten RED |
| Compiler/runtime header constants | 25 restatements; one-bit move | PASS / RED |
| Runtime root-holder inventory | Executable gate | PASS |
| Forced full collection in hoisted typed/Buffer/subarray/bagged loop | test_buffer_b4_hoisted_view_gc.ts | Output equals Node; 24 full collections |
| Streaming zlib callback detaches input owner | test_buffer_b4_zlib_detach.ts | Output equals Node; valid 1 MiB round trip, detached input length zero |
| Whole-module pointer-tagged p-8 invariant | Native/proxy/fetch registry IDs remain nonheap pointer-tagged values | Not established |

ASan uses a separate target and the system allocator so native Backing frees are instrumented, preserving other default runtime features. Leak scanning is disabled for immortal allocations. Its 35 selected tests include the 19 B4, eight B1 and one header-admission sabotages; all turn red, with no ASan failure in the positive runs. The ASan target was cleaned after preserving results. The streaming-zlib Node witness uses one output window: a smaller multi-window version also errors in Node after input detachment and is not claimed as passing parity. Byte cells are born old and nonmoving; this does not test a future B6 young-byte allocation mode.

The whole-module pointer-header requirement exceeds the current NaN-box representation: value/addr_class reserves pointer-tagged bands for proxy and native-resource/fetch registry IDs without addressable p-8 headers. Persistent Box-leaked symbols have real leaf prefixes and every byte family has the common GC prefix, but this does not establish the whole-module invariant. Converting those ID representations would require broader producer/consumer changes, including protected fetch code. No subset proof is substituted.

## Program and kernel acceptance

All ten required real programs pass against Node on current main, including buffer_heavy and worker_heavy: their old baseline exceptions have cleared and both are included in the final gate. Final head program compilation and the complete 186-case gap comparison are running. The final compiler already matches Node on all four kernels and hello; The first final n=5 interleaved kernel batch is complete and still fails the hard gate; subsequent compiler corrections are being tested.

| Required programs | Final head output == Node | Instructions:u, RSS, full collections |
|---|---|---|
| tsc, Zod x5000, qs parse/stringify, commander, fastify | Final compilation pending | Pending |
| hello | PASS | Final measurement pending |
| Effect, buffer_heavy, worker_heavy | Final compilation pending | Pending |

| Hard-gate kernel | Final compiler output parity | Final flat-or-better gate |
|---|---|---|
| matmul | PASS | −0.000069%, PASS |
| prime_sieve | PASS | +0.990336%, RED |
| bench_buffer_readwrite | PASS | −0.001291%, PASS |
| ECS u32 | PASS | +65.702271%, RED |

An earlier diagnostic compiler measured prime_sieve +0.990946% and ECS +230.990027%, so that snapshot failed the hard gate. The ECS length adapter called a runtime helper on each sealed-view check, obstructing loop hoisting; it now reads the canonical independent length slot. Prime's emitted loop assembly is unchanged. The compile-time dense admission prefix did not remove its +0.99% regression; runtime-call profiling is queued and the earlier attribution to sparse metadata was not sufficient. ECS assembly shows repeated duplicate register moves from its owner-lifetime uses. Consolidating the same exact roots into one empty assembly use, marked as touching no memory, is being tested together with equivalent masked brand predicates. No cache, side table or latch is restored. This batch measured hello +310 instructions (+0.022378%); its explanation and the complete program gate remain pending.

## Header-size RSS

The first THP-off retained-small-owner measurement has five interleaved trials, zero huge pages and one full collection in both nonzero arms. At 131,072 retained owners, RSS is 21,192 KiB main and 21,700 KiB head (+508 KiB); at 262,144, it is 30,920 and 32,452 KiB (+1,532 KiB). The difference increases by 1,024 KiB for 131,072 more owners: exactly 8.00 B per added owner, matching the approved header increase. Absolute totals contain a fixed offset of −516 KiB, including −132 KiB at empty startup; the remainder is still being investigated with separate census diagnostics. A repeat with the final compiler is queued. Diagnostic census allocations are excluded from accepted RSS/instruction trials.

## Remaining acceptance work

The final gap and real-program comparisons, final kernel hard gate, material delta explanations, pool-retention RSS and final header RSS attribution remain pending. Source debt is 18 protected fetch/stream sites, with no new exemptions; the requested zero-debt result is not reached. The global pointer-tagged-header invariant remains a broader representation gap. This milestone makes no landable-head or performance-acceptance claim.

Invalid earlier gap runs are excluded: one wrote inside the synced source tree and lost output files during rsync, and a subsequent valid partial run was stopped by its own PID tree before the final parameter update. Outputs now live outside src and the driver requires the complete selected case count. Artifact mutation and measurement are serialized with a lane-local lock as well as the shared host measurement lock. No version fields changed.
