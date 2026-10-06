Buffer B4 preparation: root typed-array byte copies through the existing byte API and remove the three obsolete symbol payload screens from runtime brand probes. This is a preparation milestone, not completion of B4a/b/c.

`typed_array_to_array_buffer` now calls `bytes::copy_value`: it roots the source before destination allocation and resolves its bytes afterward. The ArrayBuffer-view constructor roots its source and its length argument across coercion, and resolves the source again after allocating the typed array. Lazy typed-array `.buffer` materialization retains its typed-array receiver until the backing edge is installed. No byte pointer from these paths crosses an allocation without its owner. View registration now distinguishes owned native stores (stable addresses) from borrowed foreign memory (rebindable), preserving the existing resolved-storage contract until the B4c switch.

Persistent symbols already acquired a `GC_TYPE_SYMBOL` prefix in B2c part 1. Buffer and typed-array brand probes now trust that prefix without reading `SYMBOL_MAGIC` from the payload. The former test fabricated a headerless symbol in a raw Box; it has been replaced with a real persistent-symbol factory witness. Both persistent-symbol witnesses have a prefix sabotage. Allocator ownership checks remain where inputs can be arbitrary raw bits.

The typed-array copy wrapper retains its strict typed-array admission before entering the byte API; Buffer-shaped cells are rejected as before. `copy_kind` sabotages that boundary.

The shared `copy_value` implementation has a test-only collecting callback and a missing-root sabotage. Its witness checks the precise root mark before sweeping, runs a full collection between destination allocation and source-byte resolution, and compares every copied byte. A missing root turns the witness red before a stale pointer can be dereferenced. No production cache, latch, side table or environment switch was added.

## Decision 88 and layout coordination

Decision 88 settles the cost and metadata policy: the cell remains 16 bytes, with no owner trailer. A materialized ArrayBuffer identity is an ordinary hidden internal-key property in the owner's shape and property storage. Nested pins retain B1's header protocol, process-shared owners remain permanently pinned, and count overflow uses hidden property storage. None of these fields may move to an address table.

The current buffer expando implementation is still `buffer::own_props`' address-keyed map; it is not ordinary shaped property storage and cannot be used as the replacement for `BUFFER_AB_ALIAS`. The attachment needs to be introduced with the cell layout. The attachment protocol must keep the fixed cell at 16 bytes and charge additional shape storage only to materialized properties. Simply externalizing inline data on the first property write is insufficient: existing pins may already expose that data, including all 31 header-counted pins when an overflow property is needed. An attachment must preserve those addresses and the aliasing store while retaining a traced property-storage edge. No such attachment has been implemented.

The existing `NativeTypedViewHeader` in `native_arena.rs` duplicates the old TypedArray prefix (kind/element size/storage/flags at +8, owner at +16, cached data at +24). B4 must replace it with the shared byte-cell prefix and owner resolution; preserving that duplicate would fail the one-layout contract. This file is in ump's protected region scope. Prefix coordination has been raised with the coordinator, and no edit to it or to allocator/placement/fetch files has been made. The source layout and all emitted paths will switch together after this cross-owner interface is settled. The concrete interface must keep the common length/capacity-or-offset words and owner/data pointer at +8, stamp element kind and owner/view state in the GC header, and publish disposal in owner header state before releasing native bytes. Native views must resolve the owner at access time instead of using +24 cached data. NativeArena generation and POD-view semantics still belong to that lane; B4 must not silently remove them.

Coordination requested: the protected owner lane must provide a pin-stable ordinary shape/property attachment, and either convert the NativeArena prefix or authorize the narrow shared-prefix conversion here. This request follows the user’s instruction to stay out of ump’s region and allocator files. It does not request a cost increase or reopen decision 88. A fixed trailer, the existing property address map, a cached view pointer, or silently leaving NativeArena on the old prefix would each violate the approved design.

B2c will land separately on origin/main; this lane continues on its existing base and will rebase when that happens. The bundle is no longer a dependency to wait for.

## Ranged copies and retained typed transforms

The next preparation milestone removes `Buffer.copyBytesFrom`'s borrowed slice/raw receiver across destination allocation. The byte API now accepts a clamped range, roots the source first, pins the destination, and resolves the source again in `no_gc` after allocation. Offsets and lengths remain element counts for typed input; copying preserves raw bytes of all twelve kinds.

Typed-array reversal and default sorted copies use a typed byte allocation capability and the same retained range-copy rule. Reversal copies lanes without boxing BigInts and preserves Float64 NaN payloads and signed zero. Shared-source copies retain the existing unordered atomic read at the element width, then write those raw bits into private destination storage; they do not replace shared lane reads with memcpy. Array materialization retains its source, result and one reused candidate root. The numeric comparator-sort path retains its receiver across user callbacks and uses bounds-checked write-back. `with` retains its source and replacement across coercion and allocation, and pins the result while BigInt reads may allocate. `findLast` and `findLastIndex` retain their receiver, callback and candidate, and re-read roots after each predicate call.

The new witnesses force full collection between allocation and byte resolution, compare ranged copies of every typed kind (private and shared stores), and preserve a NaN payload through reversal. A predicate witness performs three full collections while checking that its typed receiver, callback and boxed BigInt candidate were marked by precise roots; a missing receiver root fails before sweeping. No new production cache, latch, name check or side table is introduced.

## Census and deletion status

| Inventory | Before | This milestone |
|---|---:|---:|
| Original design census (production / test rows) | 350 / 230 | Same historical census |
| Explicitly closed production rows in inherited ledger | 73 | 82 |
| Source-layout ratchet debt | 284 | 279 |
| Symbol payload screens in the three runtime brand probes | 3 | 0 |

The first preparation closed `typed_array_to_array_buffer`'s direct allocation. This milestone additionally closes all five historical `copy_bytes.rs` creation/length-write rows and three typed-transform creation rows. Five source-ratchet entries disappear, including the array-materialization write matched by the original broad result-header pattern. The ledger records conversions; it is not a claim that all other historical rows are current outstanding sites.

| Machinery | Deleted in this milestone | Remaining |
|---|---|---|
| Address tables | None | VIEW_REGISTRY, BACKING_TO_VIEWS, RESIZABLE_BUFFER_MAX, BUFFER_AB_ALIAS, TYPED_ARRAY_VIEW_META |
| Latches | None | RESIZABLE_BUFFER_EVER_MARKED, BUFFER_AB_ALIAS_EVER_SET |
| Address caches | None | PERRY_U8_INLINE_CACHE, PERRY_TA_KIND_CACHE |

## Verification status

Preparation verification completed on qb6, in `/root/codex-lanes/cx-bufb4b`, release profile, CPUs 0–55, at most eight Cargo jobs and single-threaded tests. The starting tree is B2c part-1 `e2f2c84ae1`; refreshed origin/main is `e87628eb18`. B2c has not appeared on that origin/main. These are preparation checks, not the requested final verification on a rebased main-combined head. The complete retained-transform milestone passes 5,173 runtime unit tests (zero failures, five ignored), one integration test, and has eight ignored doc tests. Ten focused B4 tests pass, with all fourteen runtime sabotage invocations RED. Mac and Linux SHA-256 hashes agree for all ten changed runtime/test sources.

| Contract / check | Witness / sabotage | Status |
|---|---|---|
| Source retained across byte-copy allocation and full GC | Precise root mark, forced full collection, copied-byte comparison; copy_root and copy_kind | PASS / both RED |
| Owned-native resolution | Numeric-kind resolved-slot contract; native_resolution | PASS / RED |
| Persistent-symbol prefix and header brand authority | Three symbol factories and buffer/header_brand_tests; symbol_header | PASS / RED |
| Retained typed copies and predicates, atomic lane width | Full-collection root marks, raw bits, callback candidate; typed_copy_root, find_receiver_root, shared_lane_copy | PASS / all RED |
| Existing B4 owner checks, detach, view edge, transfer and u32 admission | Inherited buffer_b4 witnesses and child sabotages | PASS / all RED; total ten focused tests and fourteen runtime sabotage invocations |
| Layout ratchet | Nine child-process source sabotages | PASS / nine RED; 279 sites remain |
| Runtime root-holder inventory | Inventory gate | PASS |
| GC header constants | Gate and one-bit sabotage self-test | PASS / RED |
| Node-version consistency | All registered restatements and exemptions | PASS |
| Whole-module pointer-header invariant; hoisted-loop owner liveness | Required B4c witnesses | Pending unified layout/codegen |

| Required program set | Output-vs-Node / n=5 interleaved instructions, RSS and full collections |
|---|---|
| tsc, Zod x5000, qs parse, qs stringify, commander, hello, fastify, Effect | Not run for this milestone |
| buffer_heavy, worker_heavy | Current-main status not re-established in this lane |

| Hard-gate kernel | Parity / flat-or-better status |
|---|---|
| matmul | Not measured |
| prime_sieve | Not measured |
| bench_buffer_readwrite | Not measured |
| ECS u32 | Not measured |

The first full runtime run exposed two resolved-slot failures because the new byte-API copy factory creates a native-backed ArrayBuffer; the owned-versus-borrowed distinction above fixes that representation admission. It also ran the superseded fabricated-headerless-symbol fixture. The corrected first-preparation release suite passed 5,169 unit tests (zero failures, five ignored), one integration test, and has eight ignored doc tests. That first preparation passed six focused B4 tests and all ten runtime sabotage invocations were RED. Linux and Mac SHA-256 hashes agreed for all seven changed runtime/test source files. Reproducible counts and source hashes are in `scripts/fixtures/buffer_b4_preparation_verification.json`; raw logs are retained in the lane hostdir. The latest retained-transform verification is recorded separately in `scripts/fixtures/buffer_b4_lifetime_verification.json` with its ten-source hashes, 5,173-unit-test result and fourteen sabotage invocations.

The unified header's +8-byte small-buffer RSS effect has not been introduced or measured. There is no B4 performance acceptance claim. Full runtime/stdlib/codegen/ffi/ext-zlib A/B suites, gap-union comparison, all real-program and kernel measurements, zero-debt gate, layout T-tests and the B2c/current-main rebase remain required before a B4 delivery is landable.
