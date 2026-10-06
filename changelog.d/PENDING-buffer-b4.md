Buffer B4 preparation: root typed-array byte copies through the existing byte API and remove the three obsolete symbol payload screens from runtime brand probes. This is a preparation milestone, not completion of B4a/b/c.

`typed_array_to_array_buffer` now calls `bytes::copy_value`: it roots the source before destination allocation and resolves its bytes afterward. The ArrayBuffer-view constructor roots its source and its length argument across coercion, and resolves the source again after allocating the typed array. Lazy typed-array `.buffer` materialization retains its typed-array receiver until the backing edge is installed. No byte pointer from these paths crosses an allocation without its owner. View registration now distinguishes owned native stores (stable addresses) from borrowed foreign memory (rebindable), preserving the existing resolved-storage contract until the B4c switch.

Persistent symbols already acquired a `GC_TYPE_SYMBOL` prefix in B2c part 1. Buffer and typed-array brand probes now trust that prefix without reading `SYMBOL_MAGIC` from the payload. The former test fabricated a headerless symbol in a raw Box; it has been replaced with a real persistent-symbol factory witness. Both persistent-symbol witnesses have a prefix sabotage. Allocator ownership checks remain where inputs can be arbitrary raw bits.

The typed-array copy wrapper retains its strict typed-array admission before entering the byte API; Buffer-shaped cells are rejected as before. `copy_kind` sabotages that boundary.

The shared `copy_value` implementation has a test-only collecting callback and a missing-root sabotage. Its witness checks the precise root mark before sweeping, runs a full collection between destination allocation and source-byte resolution, and compares every copied byte. A missing root turns the witness red before a stale pointer can be dereferenced. No production cache, latch, side table or environment switch was added.

## Remaining layout decisions

The design fixes `[length:u32, capacity-or-offset:u32, data-or-owner:ptr]` at 16 bytes, with inline data immediately at +16. It does not assign storage for an owner's lazily materialized ArrayBuffer identity edge. That edge is needed to delete `BUFFER_AB_ALIAS` while preserving `.buffer` identity and aliasing; a view's +8 word is already its store-owner edge. The native/resizable extension can hold it, but a one-byte inline owner has no extension slot under the stated layout.

The proposed header state also needs pin storage. ELEM_KIND (4), OWNER (1), PLACEMENT (2), RESIZABLE, DETACHED, LENGTH_TRACKING and SHARED_BACKING require 11 reserved bits. GC survival age uses 3 more, and the current byte pin protocol uses 6 (count plus prior-pin state). Moving pins into owner metadata is necessary. The approved cost is +8 bytes per small owner, so an additional always-present 8-byte owner trailer would need a cost decision.

A concrete option is an 8-byte owner trailer referencing lazily allocated owner metadata (ArrayBuffer edge, pin count/prior-pin state, resize bookkeeping); views remain exactly 16 bytes. It preserves inline bytes at +16 but adds 16 bytes per small owner in total, rather than the approved 8. Native owners can embed the same metadata in their extension. SHARED_BACKING can derive from Shared placement, leaving the layout state in reserved bits 6–15 and preserving GC age and freeze/seal/prevent-extension bits 0–5. Owner tracing must then visit its materialized ArrayBuffer identity edge as well as views tracing their store-owner edge.

These decisions were raised with the owner. No unified layout or emitted access path is committed pending resolution. Ump's placement/allocator/region files, fetch work, runtime node_stream files, ext-zlib implementation, versions, and external application state are untouched.

## Census and deletion status

| Inventory | Before | This milestone |
|---|---:|---:|
| Original design census (production / test rows) | 350 / 230 | Same historical census |
| Explicitly closed production rows in inherited ledger | 73 | 74 |
| Source-layout ratchet debt | 284 | 284 |
| Symbol payload screens in the three runtime brand probes | 3 | 0 |

The additional closed row is the direct `buffer_alloc` in `typed_array_to_array_buffer`, now routed through the byte API. That accessor module is exempt from the layout ratchet, so this lifetime fix does not reduce the ratchet count. The ledger is a list of conversions, not a claim that all other historical rows are current outstanding sites.

| Machinery | Deleted in this milestone | Remaining |
|---|---|---|
| Address tables | None | VIEW_REGISTRY, BACKING_TO_VIEWS, RESIZABLE_BUFFER_MAX, BUFFER_AB_ALIAS, TYPED_ARRAY_VIEW_META |
| Latches | None | RESIZABLE_BUFFER_EVER_MARKED, BUFFER_AB_ALIAS_EVER_SET |
| Address caches | None | PERRY_U8_INLINE_CACHE, PERRY_TA_KIND_CACHE |

## Verification status

Preparation verification completed on qb6, in `/root/codex-lanes/cx-bufb4b`, release profile, CPUs 0–55, at most eight Cargo jobs and single-threaded tests. The starting tree is B2c part-1 `e2f2c84ae1`; refreshed origin/main is `899aecd6f9`. The rebased B2c bundle was absent at the last check and B2c has not appeared on that origin/main. These are preparation checks, not the requested final verification on a rebased main-combined head.

| Contract / check | Witness / sabotage | Status |
|---|---|---|
| Source retained across byte-copy allocation and full GC | Precise root mark, forced full collection, copied-byte comparison; copy_root and copy_kind | PASS / both RED |
| Owned-native resolution | Numeric-kind resolved-slot contract; native_resolution | PASS / RED |
| Persistent-symbol prefix and header brand authority | Three symbol factories and buffer/header_brand_tests; symbol_header | PASS / RED |
| Existing B4 owner checks, detach, view edge, transfer and u32 admission | Inherited buffer_b4 witnesses and child sabotages | Six focused tests PASS; ten runtime sabotage invocations RED |
| Layout ratchet | Nine child-process source sabotages | PASS / nine RED; 284 sites remain |
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

The first full runtime run exposed two resolved-slot failures because the new byte-API copy factory creates a native-backed ArrayBuffer; the owned-versus-borrowed distinction above fixes that representation admission. It also ran the superseded fabricated-headerless-symbol fixture. The corrected final release suite passes 5,169 unit tests (zero failures, five ignored), one integration test, and has eight ignored doc tests. Six focused B4 tests pass and all ten runtime sabotage invocations are RED. Linux and Mac SHA-256 hashes agree for all seven changed runtime/test source files. Reproducible counts and source hashes are in `scripts/fixtures/buffer_b4_preparation_verification.json`; raw logs are retained in the lane hostdir.

The unified header's +8-byte small-buffer RSS effect has not been introduced or measured. There is no B4 performance acceptance claim. Full runtime/stdlib/codegen/ffi/ext-zlib A/B suites, gap-union comparison, all real-program and kernel measurements, zero-debt gate, layout T-tests and the B2c/current-main rebase remain required before a B4 delivery is landable.
