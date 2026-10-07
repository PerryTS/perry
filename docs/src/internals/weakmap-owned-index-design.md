# WeakMap lookup and owned identity storage

Status: owned-storage and conditional-ephemeron implementation is complete;
performance acceptance is blocked by TypeScript cycles. See
[implementation validation](weakmap-owned-index-validation.md) for the fresh
qb6 program matrix, floors, GC counts, and THP-disabled check. Rebased onto
`68e41faa0f9145d8b8ab67ecb0cf414eaf13420b`, including c85c2db93 (#12105).
The coordinator authorized the redesign on 2026-10-06 and filed the
conditional tracing defect as #12087.
The original design and diagnostic baseline below were recorded against
`4c2e7fa2b53e8721ec01aa0d1a7a0921148ca6b4`; they are historical evidence,
not acceptance measurements for the implementation.

## Implemented representation

WeakMap and WeakSet constructors add their internal brand to the Shape's
private brand facts. A spare Shape flag summarizes those facts without
changing the Shape record or ordinary object header size. Transitions retain
the brand. The object owns a tagged `GC_TYPE_WEAK_STORAGE` edge through
`ObjectMeta.native_state`; empty maps allocate that metadata lazily.

The storage cell holds key/value pairs, twice-capacity open-addressed bucket
offsets, and a reverse bucket offset per entry. The reverse offsets make
delete and budgeted weak clearing constant work after lookup. Empty entries
reuse their value word as a scalar free-list offset. No owner-address index,
last-key cache, or extra copies of key pointer bits survive outside the cell.
Growth discards the lookup view, roots all call arguments, reloads after
allocation, and publishes the replacement through the existing meta barrier.

Collectors discover the cell through its owner edge. Conditional closure
traces values only for independently live keys, including cross-map and Proxy
chains. The copying collector repairs authoritative pairs and rebuilds
buckets before returning to the mutator. Budgeted non-moving closure and
clearing charge entries individually. Weak pair publication maintains old to
young coverage without unconditionally shading copied keys or values.
Mark verification checks enabled values; rewrite and coverage verification
continue to inspect both words.

## Measured baseline and verification

Fresh Linux release build: `perry`, `perry-runtime-static` and
`perry-stdlib-static`, in this lane's `target-main`. All 35,590 tracked source
files matched the local baseline before building and after the diagnostic
test was removed. Compiler and archive hashes are retained with the evidence.
The host is the shared Ryzen 7 7700X, Node 26.5.1 and Bun 1.3.14.

These are **full-static diagnostic baselines**, compiled with
`PERRY_KEEP_SYMBOLS=1 --no-cache --no-auto-optimize`. This mode differs from
the auto-specialized baseline in LEVERS.md, especially in startup and RSS.
It is not an A/B gate for a proposed performance patch. The exact
`import { Schema } from "effect"` source passes on this baseline, with
`ok=20000`; the direct-import substitute was unnecessary.

Each main row is the median of ten runs, interleaved with ten runs of the
identical binary, ASLR disabled and CPU 2 pinned. Performance counters and
peak RSS are uninstrumented. GC counts beside RSS come from three separate
trace runs and were identical across those runs. CPU is perf task-clock in
milliseconds; **cycles**, not task-clock, are the owner's CPU gate axis.

| Program | Main instructions:u, M | Main cycles:u, M | Main CPU, ms | Main RSS, MiB | Minor/full GC | Fix |
|---|---:|---:|---:|---:|---:|---|
| weakmap-get, 1M | 831.579 | 282.587 | 61.11 | 24.473 | 0/0 | N/A |
| weakmap-hit-pair, 1M | 1625.579 | 554.027 | 116.97 | 24.340 | 0/0 | N/A |
| Effect, exact source | 29081.772 | 17622.809 | 3589.69 | 194.000 | 18/2 | N/A |
| hello | 1.317 | 2.574 | 2.99 | 14.547 | 0/0 | N/A |
| 07_object_create | 33.112 | 21.591 | 7.67 | 26.996 | 0/0 | N/A |
| 09_method_calls | 61.479 | 124.527 | 28.29 | 23.691 | 0/0 | N/A |
| 14_closure | 251.326 | 156.094 | 34.14 | 14.660 | 0/0 | N/A |
| bench_object_property | 181.586 | 75.679 | 19.19 | 34.695 | 1/0 | N/A |
| Zod | 5359.568 | 3236.034 | 659.80 | 55.082 | 27/0 | N/A |

TypeScript, qs parse/stringify, commander and fastify are **unmeasured** in
this design fallback. The first TypeScript setup accidentally omitted the
captures harness's explicit `typescript` AOT setting, selected the native
shim and failed reading `ScriptTarget.ES2022`; that run is invalid, not a
TypeScript-source baseline failure. The explicit setting was restored; the
real-source TypeScript build was not run. The copied captures harness
contained no qs/commander/fastify drivers, and no replacement workload was
substituted. The full real-program gate remains required for implementation.
`bench_object_property.ts`, absent from this snapshot's benchmark directory,
was copied unchanged from the captures lane along with its Zod/TypeScript
drivers.

Identical-binary floor/main median deltas exceeded +0.5% for Effect cycles
(+1.185%) and RSS (+0.600%), and hello cycles (+0.982%). Instruction deltas
were below 0.01% on every measured program. These controls expose host noise;
they do not expand the owner's tolerance or establish a passing gate.

Three-run instruction slopes at 1M and 2M iterations are **830.0 per get**
and **1624.0 per has/get pair**. Node's get slope at 100M/200M is 122.9;
Bun's is 60.6. These include loop/key-selection/checksum scaffolding.
Exact Effect three-run references are Node 3727.945M user instructions,
773.44ms CPU, 204.773MiB RSS, and Bun 3276.357M, 684.84ms, 158.598MiB.
The full-static Perry/Node instruction ratio is 7.80x; Perry/Bun is 8.88x.

DWARF captures used `-F 1999 --call-graph dwarf,8192` for time and
`-e instructions:u -c 100003 --call-graph dwarf,8192` for instructions.
No lost samples were reported. Caller extraction uses `perf script
--no-inline` to avoid expensive addr2line expansion; sampled instruction
weights are checked against perf's reported event total. Inline attribution
and truncated deep stacks are not claimed as complete ancestry.

The standalone instruction profile puts 28.92% in `js_array_length`, 22.22%
in `js_array_get_f64`, 18.63% in `js_weakmap_get` self and 17.83% in
`weakref::index::find` self. The get ancestry contains both
`js_array_get_f64 -> index::find -> js_weakmap_get` and the second
`js_array_get_f64 -> js_weakmap_get` extraction. Within exact Effect, these
three validated-array/index leaves account for 181.805M (`find`), 141.004M
(`array_get`) and 104.603M (`array_length`) sampled user instructions under
WeakMap calls. The memoizer ancestry is directly recovered:

```text
js_array_length / js_array_get_f64
weakref::index::find
js_weakmap_has
object::weakref_proto_thunks::try_weak_method_dispatch
object::native_call_method::native_call_method_tower
js_method_site_miss
perry_closure_node_modules_effect_src_Function_ts__38
perry_fn_node_modules_effect_src_SchemaParser_ts__makeEffect
```

All recovered WeakMap ancestry, including insertion/allocation descendants,
is 3.748% of sampled Effect instructions and 3.307% of time samples. This is
an inclusive observation, not an exclusive saving budget. Never interpret
LLVM-merged hash symbols literally without checking their callers.

Verification:

- Pristine `cargo test --release -p perry-runtime -- --test-threads=1`:
  5,118 passed, five ignored; integration test passed; eight doctests ignored.
- Six of seven existing `*weak*.ts` files match Node stdout and exits.
  `test_issue_2656_weakref_finalization_gc.ts` already differs on main:
  Perry clears two WeakRefs while Node keeps them live. No runtime code was
  changed in this lane, so no previously matching test was altered.
- The new diagnostic behavior matrix matches Node byte-for-byte: primitives,
  registered/fresh/well-known symbols, delete/re-set, buffer, typed array,
  URLSearchParams, Proxy identity, function/class keys, subclasses, borrowed
  methods and forged/Proxy receiver brand rejection. Existing index tests
  cover many keys and movement.
- The added Rust ephemeron witness **fails on baseline**, after its actual
  copying-minor assertion passes: entry count is 1, expected 0. A separate C
  fixture linked against the pristine archives independently reports
  `holder_moved=1 control_dead=1 value_backedge_key_dead=0`; its trace copied
  13 objects. The Rust witness was then removed and all source hashes
  rechecked. The failing witness is retained as evidence, not committed as a
  passing or ignored test.
- `cargo fmt --all -- --check` already fails on pristine main at
  `buffer/view_tests.rs:262`. Node-version consistency passes. Codegen tests
  and GC call-effect checks are inapplicable: no codegen, FFI signature or
  runtime effect changed. Conflict-marker and patch whitespace checks are
  empty.

Evidence and the proposed Rust/C witnesses are retained locally under
`/Users/amlug/projects/perry/secret-tests/scratchpad/codex-small/weakmap/`.
Raw perf captures and benchmark binaries remain under
`/root/codex-lanes/cx-weakmap/`. Build target directories are removed after
collection, as required by the lane brief.

## Why the widened request needs a redesign

The current successful get path does the following:

1. Validate the receiver through `weak_wrapper_class_id`.
2. Root the receiver and key and obtain its entries array.
3. Read generic array length in `index::find`.
4. Find the owner in `WEAK_COLLECTION_INDEXES`, a thread-local hash table.
5. Read generic array length again in `with_index`.
6. Probe the per-owner key hash table.
7. Read the entry through `js_array_get_f64` and verify its key.
8. Return its integer slot, then read that same entry through
   `js_array_get_f64` again in `find_entry`.
9. Run the weak read barriers for key and value.

`has` repeats the same lookup and keeps the key barrier. An overwriting `set`
and a successful `delete` repeat the entry extraction too. Insertion calls
`find`, `take_free`, allocation and `inserted`; allocation can move every
relevant object and discard the index. A view valid before allocation cannot
be carried into the publication step.

The duplicate loads are removable on a callback-free path. However, just
returning the validated entry pointer and bypassing generic array access
would retain the address-keyed owner registry in `weakref/index.rs`. That
does not satisfy the widened requirement that the collection own all of its
storage and object facts live in Shapes.

The existing ordinary object header has no owned-index field. Subclasses
retain their own class IDs, use parent-class registry traversal for weak
branding, and find `__perry_wk_entries` through a potentially allocating
by-name property access. Native payloads are also not a drop-in replacement:
`native_payload.rs` and `docs/native-payload-pattern.md` forbid JS values,
NaN-boxed bits and GC pointers in `Box<T>`. Nothing traces or rewrites those
contents. Moving the current raw key index there would invalidate its
relocation and address-reuse safety argument.

## Proposed ownership

The Shape must carry the actual internal collection brand, storage location
and entry layout. This is initialized by the builtin constructor or
`super()`, not inferred from names or a prototype chain. Preserve these facts
through own-property changes, dictionary transitions, representation changes
and `Object.setPrototypeOf`. Inheriting `WeakMap.prototype` alone must never
install the brand. A Proxy over a WeakMap does not acquire its internal slots.

Keep the user-visible object and its existing header layout. Add an owned,
GC-managed storage cell reachable through a dedicated internal edge. Decide
the exact attachment representation together with the Shape schema: an
internal slot must not become a writable user property or depend on a
reserved spelling. Do not add a word or allocation to ordinary objects.
`ObjectMeta.native_state` is a candidate attachment point, but using it needs
an explicit typed-storage contract and audit of its current owners; it is
not permission to violate the native payload contract.

The storage cell owns:

- The authoritative entry storage and its capacity/live/free counts.
- Open-addressed buckets containing entry offsets and empty/tombstone
  scalars, not extra copies of GC pointer bits.
- Reusable entry offsets, with no owner-address lookup.

Each occupied bucket selects an authoritative entry. Compare that entry's
current key to resolve collisions and recognize tombstones. No separate
last-key cache or memo is needed. Key and value words remain GC-visible
under their weak/conditional semantics. Dead owners release all storage by
ordinary reachability, without a persistent owner registry or cleanup latch.

The existing Shape kind uses all eight codes in its three-bit encoding.
Collection branding must be a deliberate Shape identity extension, not an
unreviewed ninth kind, a numeric collision or a parallel brand map. Prefer a
sparse extension for collection Shapes; measure its cost and preservation
through transitions before choosing its final encoding.

## One operation, one proof

Build a scoped `WeakCollectionView` from one receiver classification and
Shape lookup. The view carries a resolved storage pointer, entries pointer,
length/capacity and the exact layout needed for direct bounded loads.
Resolve forwarding before constructing it. It lives only within a proven
non-collecting, callback-free region.

An internal brand does not prove which method a JS call selected. Own and
prototype overrides, accessors and their invalidation remain ordinary
dispatch. Construct the fast view only after the genuine intrinsic was
selected; borrowed intrinsic methods still perform the internal brand check.

Classify the key once into a valid weak identity or an invalid primitive.
Retain non-registered and well-known symbol keys, and reject registered
symbols. Preserve every currently supported object representation:
ordinary objects, closures, class references, native objects, stream/async
resources, typed arrays, buffers and Proxy identities. Primitive insertion
throws TypeError; primitive `get`, `has` and `delete` return undefined/false.
Avoid probing symbol registries for receivers whose own header proves that
they are another kind.

Probe once and return a validated entry/key/value result, not an offset that
the caller must independently revalidate. Keep collision equality, bounds,
key liveness and the existing key/value weak read barriers. A pending weak
slice may have tombstoned a bucket's entry since the previous JS operation;
that validation is necessary even when length has not changed.

`has(a)` and `get(a)` remain two independent JS operations. Each pays one
lookup. There is no cross-call proof or cached pointer surviving a safepoint.
Rooted slow paths remain for allocation, storage growth and foreign receiver
delegation. Insertion reloads roots and reconstructs its view after every
collecting operation before publishing the new bucket.

## Collector work required

Moving GC must repair authoritative live keys and values and then rehash
address-dependent buckets before the mutator resumes. Rebuild in place from
entry offsets using the existing capacity; do not allocate a second index or
retain stale address bits until a later call. Integrate this ordering with
copying nursery, promotion, full relocation and evacuation verification.
Weak processing between incremental slices must tombstone both entry words;
subsequent reads still validate the entry and reclaim its bucket/free slot.

Discover collection storage through traced Shape-owned internal edges.
Collector-local pending work can hold the storage during a cycle; it must
not become a persistent mutator-side collection registry. Remove WeakMap/
WeakSet entry dependence on `WEAK_HOLDERS` as part of this migration while
preserving WeakRef and FinalizationRegistry behavior outside this lane.

There is a confirmed correctness gap to resolve before claiming
ephemeron preservation. `is_weak_branded_target_trace_slot` excludes entry
field 0 (the key), but field 1 (the value) is visited as a strong edge.
`process_weak_entry_after_mark` clears that value only after key liveness has
been decided. A value-to-key backedge preserves its
own key on baseline. The retained diagnostic witness tests `wm.set(key, key)` with only
the map rooted and asserts an actual copying collection before checking that
the entry cleared. Its measured outcome is recorded with the lane evidence.

A correct ephemeron collector traces the value only once its key is
independently live. Pending ephemerons need a fixed point: a value can keep
another collection's key alive. Cover full tracing, copying minors, old-key/
young-value entries, remembered-set scanning, incremental writes and reads,
remark, and deletion. Simply making both words weak would lose live values;
tracing values unconditionally would keep dead key cycles alive. This work
changes the collector protocol and cannot be hidden in an array accessor
optimization.

## Implementation and acceptance sequence

1. Add deterministic GC witnesses for direct/indirect value-to-key cycles,
   multi-map ephemeron chains, strongly live keys and movement. Assert that
   the tested collector actually copied or completed the relevant weak phase.
2. Establish Shape-owned internal branding and storage attachment, including
   subclasses, transition preservation, Proxy brand rejection and reflection.
3. Implement owned buckets and the collector trace/rewrite/rehash protocol.
   Delete the address-keyed index and its clearing hooks in the same change.
4. Implement the scoped view and validated-hit return for get/has/overwrite/
   delete, then the rooted insertion path. Keep foreign receiver dispatch.
5. Run Node 26.5.1 parity for invalid keys, fresh/registered/well-known
   symbols, identity, deletion, re-set, many keys, subclasses, borrowed
   methods, forged prototypes and exotic receiver/key cases.
6. Run the runtime suite single-threaded and all scoped gap tests. Run
   codegen tests if codegen changes, formatting, Node-version consistency and
   the current GC effect checker if FFI signatures or effects change.
7. Build pristine main and fix in separate targets with identical package
   sets. Verify source and archive hashes. Compile the exact Effect barrel
   benchmark when it works on main; otherwise retain the diagnostic import
   and clearly report the exact-source blocker.
8. Interleave at least ten runs per arm with ASLR disabled and CPU affinity
   fixed. Include identical-binary controls, uninstrumented instruction/cycle/
   RSS measurements and separate GC counts. Required matrix: weakmap-get,
   Effect, hello, object-create, method-calls, closure, object-property, Zod,
   TypeScript, qs parse/stringify, commander and fastify. Reuse the real-program
   harness, not a replacement workload with different semantics.

The owner-approved PERF_POLICY_2026-10-06.md replaces the old fixed
percentage/byte gates. The 2026-10-07 coordinator requests five interleaved
runs per label and ten for TypeScript, on qb6 measurement cores under the
shared lock, with ASLR disabled. Real-program deltas beyond same-binary
floors need named mechanisms; a larger real-program regression goes to the
owner. The current validation records the unresolved TypeScript CPU issue.
Historical design measurements above do not establish an implementation pass.
