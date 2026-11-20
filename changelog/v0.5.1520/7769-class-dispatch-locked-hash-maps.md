### Class dispatch and `instanceof` stop consulting locked hash maps

A scene-graph benchmark (`gc-handoff/apps/shapes.ts` — deep `extends` chains,
virtual dispatch through a base-typed array, `super()` chains, `instanceof`,
getters, statics, a fieldless subclass and a two-level indirect subclass) was
the widest margin a competing compiler held anywhere in the corpus. A
symbolicated profile showed why, and it was not codegen: **the runtime answered
"who is this class's parent?" and "which method is this?" with a process-global
lock plus a SipHash probe, per hop, per call.**

Measured on the pinned quiet mini, `std::hash::random::RandomState` was 1.3% of
runtime and `pthread_mutex_{lock,unlock}` another 2.8% — for what is
semantically an indexed load in a single-threaded program.

#### The parent chain is now a dense mirror, not a `RwLock<HashMap>`

`get_parent_class_id` is the single hottest class-registry read in the runtime:
`instanceof`, vtable dispatch, static-member lookup, `super()` construction,
symbol lookup and the typed-feedback guards all walk the parent chain one hop at
a time, and every hop took `CLASS_REGISTRY.read()` plus a hash probe. Codegen
assigns user class ids from a small sequential counter, so every edge whose
child id fits a 64 K window is mirrored into a flat array of atomics
(`.bss`, zero-fill, only the indexed pages are ever touched). In-window ids
answer from one atomic load; the reserved builtin bands and the high-bit
synthetic ids keep using the map.

The dense slot stores `parent + 1`, which is what lets one word distinguish
"absent" from "registered with parent id 0" — every caller that treats `Some(0)`
as a chain terminator does so explicitly, and a test pins that.

#### Five metadata registries got monotone latches

`Symbol.hasInstance` hooks, `Symbol.toStringTag` hooks, `extends Error`, the
fetch-builtin parent kind, the generic-origin table, the class static-symbol
table, and the timer-id registry are all empty in a program that does not use
those features — but `js_instanceof` probed three of them on every evaluation,
and `class_chain_reaches` probed one on every hop. They now use
`registry_latch::RegistryLatch` (#7755), so an unused feature answers from one
atomic load. The `Symbol.hasInstance` latch also keeps the string-keyed
`well_known_symbol("hasInstance")` interning probe off the path entirely.

#### The dispatch tower caches its own answer

`js_native_call_method` is the virtual-call path for every receiver whose static
type does not pin the callee — which is *every* call through a base-typed
collection, the shape a class hierarchy is written in. Reaching a resolution
cost a `String` allocation for the method name, a `RuntimeHandleScope`, ~900
lines of probes for exotic receiver kinds, a GC-heap `StringHeader` allocation
for the prototype-chain probe, a lock and two SipHash lookups. For
`shape.area()` that is four heap allocations and a lock around a single
multiply.

A per-thread, content-keyed cache now records the tower's OUTCOME for a
`(class_id, method name)` pair, and a fast path at the top of the tower serves
it. Three things about it are load-bearing:

* **Both resolution points populate it.** The first attempt cached only the
  tower's tail vtable arm, which checks the receiver's OWN class vtable — so
  every INHERITED method (`class Square extends Rect` calling `Rect`'s `area`)
  missed forever, and inherited methods are the common case in any real
  hierarchy. The parent-chain walk in `handle_methods` is the other site, and it
  is the one that mattered.
* **It is keyed on the name BYTES, not its address.** The sibling `VTABLE_IC`
  keys on the rodata pointer codegen passes, but `js_native_call_method_str_key`
  reaches the same tower with a name materialised into a *caller-stack* scratch
  buffer, where two different short names genuinely land at the same address in
  successive calls. A sabotage test plants exactly that and asserts a miss.
* **A hit never substitutes for an object-specific check.** Everything the tower
  decides per RECEIVER is re-verified on every hit: pointer classification
  through `gc_pointer_and_type_from_value` (buffers, typed arrays, Sets, Maps,
  RegExps and Symbols are raw allocations with no `GcHeader`, so screening them
  before the header read is a memory-safety requirement, not an optimisation —
  see #5625), `OBJECT_TYPE_REGULAR`, a null `meta` (which rules out both a
  per-instance `[[Prototype]]` override and any own accessor descriptor), the
  own-key scan an own field would win on, and the recorded-prototype probe. The
  `using`/`await using` disposal hooks and the iterator helpers are excluded by
  name, because both branch on per-object state the guard cannot see (a
  Symbol-keyed own property, and "is the receiver an iterator").

Prototype surgery now bumps `VTABLE_GEN`. `invalidate_class_prototype_fast_guards`
is the single latch all three prototype-write entry points funnel through, but
the method-dispatch caches were only retired by class *registration*, so a
`Class.prototype.m = fn` after first dispatch left them serving the pre-surgery
answer.

#### One argument vector instead of two `Vec`s

`call_vtable_method` built a `Vec<f64>` of positional args and then
`call_fn_with_f64_args` built a second `Vec` with `this` prepended — two
`malloc`/`free` round-trips for a zero-argument virtual call. It is now one
buffer, on the stack for every arity that occurs in practice.

#### Thread safety

`perry/thread` spawns real OS threads with independent arenas, so both new
structures had to stay correct off the main thread. The parent mirror is
process-global atomics published (`Release`) *before* the map insert, so no
reader can observe an edge through the map without it also being visible
densely; the latches follow `RegistryLatch`'s arm-before-publish rule, whose
only possible wrong observation ("idle while non-empty") that rule excludes. The
dispatch cache is per-thread and starts empty on every worker, so a worker
populates it from its own tower run rather than inheriting one — pinned by
`test_issue_7769_thread_class_dispatch.ts`, which runs the same hierarchy on the
main thread, through `parallelMap`, and through `spawn`, and compares.

#### Measured (quiet M1 mini, rebased onto `c2a96b638`, absolute seconds)

Both arms built from the same merge-base. The protected benchmarks were then
re-measured **interleaved** (arms alternating inside one window, best of 7),
because a sequential base-then-arm pass showed +0.01-0.02 drift on several rows
that turned out to be host drift moving both arms together, not a regression.

| | base | arm | | | base | arm |
|---|---|---|---|---|---|---|
| **shapes** | **0.29** | **0.24** | | churn | 0.43 | 0.43 |
| iso_miss | 2.46 | 2.45 | | churn_alloc | 0.38 | 0.38 |
| asyncpipe | 0.92 | 0.91 | | push_cls | 0.36 | 0.36 |
| interp | 1.89 | 1.89 | | retain | 0.55 | 0.55 |
| churn_read | 0.02 | 0.02 | | retain_wide | 1.12 | 1.12 |
| push_num | 0.14 | 0.14 | | tree | 1.68 | 1.68 |
| cycles | 0.19 | 0.19 | | tree_wide | 2.17 | 2.17 |
| deeplist | 0.25 | 0.25 | | fib40 | 0.40 | 0.40 |

Every protected benchmark is **identical between the two arms**. `shapes` is the
only row that moves: 0.29 → 0.24, i.e. 8.6x behind scriptc's 0.0272 s, down from
10.7x. Outputs are byte-identical to the baseline arm and to
`node --experimental-strip-types`, verified before timing: `shapes` prints
`1431180 1463160 1176000 320000040000 48000 24000 144000` and `iso_miss` reports
`misses 0`.

The win is marginally larger after the rebase than before it (0.28 → 0.23 on the
old base) because #7762 put a `class_generic_origin` probe inside
`class_prototype_object`, which the parent-chain walk calls on every hop — one
more locked hash probe on main, which this change's latch answers from an atomic
load.

On the `shapes_big` profile (two agreeing 7 s runs, ~5 250 samples each,
measured pre-rebase), the dispatch cluster (`class_registry` +
`instanceof::class_*` + `js_native_call_method` + `native_call_meth*`) falls from
**12.3% to 7.5%**, and `RandomState` + `pthread_mutex_*` from **5.6% to 3.6%**.
Four leaders leave the profile's top ranks: `get_parent_class_id` (3.3% → 0.5%),
`class_chain_reaches` (2.1% → 0.3%), `js_instanceof` (1.0%), and — because the
fast path needs no handle scope — `RuntimeHandle::get_nanbox_u64`, which was the
single hottest symbol in the program at 4.9%.

#### What the remaining lock traffic is, and why it is not this change's

Attributed by walking the profile's call graph: of the `pthread_mutex_lock`
frames, 7 in 8 come from `is_registered_symbol_slow` and the rest from
`is_registered_map`, reached from `js_array_get_f64` (so, every `arr[i]`) and
from the dispatch guard's pointer classification. Those registries are already
latched (#7474, #7755) — the latches are simply **armed**, because something in
startup materialises a well-known Symbol, which turns a free atomic load into a
process-global mutex for every array element read in every program. That is
worth chasing, but it is Map/Set/Symbol registry work, not class dispatch.

The rest of the gap on `shapes.ts` is likewise not dispatch: array element reads
(`js_array_get_f64` + `array_object_flags` + `js_array_length`, ~10%) and the GC
layout tables (~14%) now dominate it.

#### Two pre-existing divergences this change does NOT fix

`Class.prototype.m = fn` after `m`'s first dispatch still resolves to the vtable
method, and `Object.setPrototypeOf(instance, donor)` does not redirect an
already-dispatched method on that instance. Both were re-checked against a
binary built from `c2a96b638` **after** #7762's prototype-sharing work landed:
that change left them exactly as they were, and this one does not touch them
either — the fast path's guard rejects a receiver with a `meta` record, and
prototype surgery now bumps `VTABLE_GEN`, so neither is reached from the cache.
They are called out in the gap test rather than asserted, so the file stays
byte-identical to Node.
