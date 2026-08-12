# REALM-NOTES — per-realm state under `perry/thread` (#7988, #6763)

Working notes for the "what is process-global that should be per-realm?" thread.
Written incrementally; the PR is #7994.

## The shape

`perry/thread` gives each OS thread its **own realm** (`THREAD_GLOBAL_THIS`,
`crates/perry-runtime/src/object/global_this/fetch_globals.rs:13`) and its **own
arena** (`ARENA` / `OLD_ARENA` / `LONGLIVED_ARENA`, all `thread_local!`). A
process-global `static` that holds either

* a **raw heap address** — it names an object in one thread's arena, which the
  owning thread's collector may sweep or move and whose blocks are `dealloc`'d
  at that thread's exit; or
* **per-realm identity** — "which object *is* this realm's `Object.prototype` /
  `C.prototype` / `localStorage`",

is a bug with three faces:

1. **Wrong identity** — agent B compares its own objects against A's.
2. **Unattributed dereference** — B reads a `GcHeader` in A's arena.
3. **Cross-thread root rewrite** — B's collector stores its own to-space address
   into a cell that names A's heap. This is the dangerous one.

A **sticky "somebody somewhere patched X" flag is NOT this bug** — it is a safe
over-approximation (every thread takes the slow path; nobody gets a wrong
answer). `ARRAY_PROTO_HAS_INDEX` / `OBJECT_PROTO_HAS_INDEX` /
`PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED` are deliberately left process-global
for exactly that reason.

Three members of the family were found in one day, all as side effects of
chasing test flakes: #7954 (a promotion veto read from a process-global), #7981
(a class-parent edge taken from a shape stamp), #7988 (this one).

## #7988 — what was fixed

`crates/perry-runtime/src/array/prototype_addr.rs`: `ARRAY_PROTO_ADDR` and
`OBJECT_PROTO_ADDR` were `static AtomicUsize`, holding raw addresses of
`Array.prototype` / `Object.prototype`. `js_get_global_this` bootstraps once per
*thread*; `resolve_prototype_addr` missed once per *process*. First thread wins,
for the life of the process.

Now: one `crate::perry_thread_local!` holding `[Cell<usize>; 2]`, indexed
positionally against `PROTOTYPE_ADDR_BUILTINS`. The root scanner iterates that
array, so the #6981 invariant ("every cell an accessor reads is a cell the
collector rewrites") survives by construction.

### The recorded obstacle was stale

#7988 and #7955 both record the objection as *"the accessor is on
`note_array_index_write`, the not-forwarded case must stay call-free, and Darwin
has no local-exec TLS"*. That is true of `std::thread_local!` — on Darwin every
access is an out-of-line `_tlv_get_addr` call in `libdyld`.

It is **not** true of `crate::perry_thread_local!` (#7469,
`crates/perry-runtime/src/tls_hot.rs`). That macro publishes the value's address
into this thread's `HotTls` cache, which on Apple aarch64 is reached through the
pthread TSD array directly from `TPIDRRO_EL0`: an `mrs` plus two loads that LLVM
CSEs across the whole enclosing function. `tls_hot.rs`'s own module docs say the
declaration form is the **default** and there is nothing to wire.

**Anyone hitting "we can't make this per-thread, Darwin TLS is a call" in this
runtime should check `tls_hot.rs` first — that objection has been obsolete since
#7469.** It is currently repeated verbatim in at least two issues.

## The multi-agent probe

`test-files/test_issue_7988_thread_realm_prototype.ts`. The **main thread warms
both intrinsics before any agent starts** — without that the worker might be the
thread that fills the shared cell and the probe passes on the broken tree. Then
an agent pollutes its own realm (`Object.prototype[7]`, `Array.prototype[8]`)
and reads `[1,2,3][7]` / `[1,2,3][8]` through the chain.

Neither the gap suite nor the compile corpus exercises `perry/thread` at all, so
**both are vacuous gates for this class of change**. Do not cite corpus-green as
evidence for a `perry/thread` fix.

## Inventory: other process-globals that should be per-realm

Ranked. Every entry is a `static` (not a `thread_local!`) holding an address or
a per-realm identity. Not yet fixed — filed for follow-up.

| # | Declaration | File | Hazard | Symptom for agent B |
|---|---|---|---|---|
| 1 | `CLASS_PROTOTYPE_OBJECTS`, `CLASS_DECL_PROTOTYPE_OBJECTS`, `CLASS_PARENT_CLOSURES` | `object/class_registry/state.rs:316,325,381` | all three | `class_id` is a compile-time constant shared by every thread; the **value** is `C.prototype`'s address in one thread's arena. `prototype_objects.rs:34` is the first-thread-wins gate. `new C()` on B embeds A's `C.prototype` into a live object B just built, and `class_gc_roots.rs:44` rewrites the shared map from every thread's collector. Worse than #7988: a foreign pointer *stored into the heap graph*, not merely compared. |
| 2 | `GLOBAL_THIS_PTR` / `GLOBAL_THIS_READY` | `object/mod.rs:252` | 1, 3 | The realm root itself. `THREAD_GLOBAL_THIS` already shields the common read path (and its doc comment states the hazard), but the process-global slot is still written by every thread's first `js_get_global_this` and is a registered scanner target (`object/mod.rs:1151`). |
| 3 | `ITERATOR_PROTOTYPE_PTR` + 5 siblings | `object/iterator_prototypes.rs:41` | 1, 2 | `ensure_iterator_prototypes()` is a once-per-**process** gate; `attach_iterator_prototype` then chains every new iterator on every thread to whichever thread built the tower. Hit by any `for..of` / spread. |
| 4 | generator / async-generator intrinsic tower (6 cells) | `object/mod.rs:280-285` | 1, 2 | same once-per-process gate in `global_this/generator.rs:716`. #7251 fixed the *test* isolation via `per_test_global!`, which expands to a plain `static` in production. |
| 5 | `TYPED_ARRAY_INTRINSIC_PTR` / `..._PROTO_PTR` | `object/mod.rs:256` | 1, 2 | `global_this/typed_array.rs:418`; its own comment asserts "single-threaded under the singleton CAS", which stops being true the moment a second thread runs `populate_global_this_builtins`. |
| 6 | `HTTP_METHODS_CACHE`, `FS_CONSTANTS_CACHE`, 5 `OS_CONSTANTS_*` | `object/mod.rs:245-251` | 2 | built with `*_longlived` allocators, and `LONGLIVED_ARENA` is thread-local (`arena/block.rs:992`). B's first `require('http').METHODS` hands back A's array; guaranteed dangling once A exits. |
| 7 | `LOCAL_STORAGE_PTR` / `SESSION_STORAGE_PTR` | `object/mod.rs:287` | 1 | not even `per_test_global!`. `web_storage.rs:375` brand-checks by pointer equality, and `web_storage.rs:220` overwrites both on every thread's bootstrap — so A's own valid `localStorage` call starts failing its brand check after B starts. |
| 8 | `FUNCTION_CLASS_IDS` | `object/class_registry/state.rs:309` | 1 | keyed by NaN-boxed closure **heap pointer** bits. Narrower (needs an address collision across two arenas) but the same shape. |
| 9 | `SYMBOL_PROPERTIES` / `SYMBOL_PROPERTY_ATTRS` / `CLASS_STATIC_SYMBOLS` | `symbol.rs:499,506,819` | leak only | already documented in-file at `symbol.rs:508`: cross-thread owners are only reclaimed by the owning thread. Bounded leak, not corruption. |

Not fully triaged, same neighbourhood as #1 and worth the same treatment:
`CLASS_VTABLE_REGISTRY`, `CLASS_STATIC_METHODS`, `CLASS_STATIC_ACCESSORS`,
`CLASS_SYMBOL_METHODS`, `CLASS_SYMBOL_ACCESSORS`, `CLASS_DYNAMIC_PARENT_VALUE`
(a `u64` that looks like it could be a NaN-boxed JSValue, i.e. a heap pointer),
`CLASS_OBJECT_VALUES` — all `object/class_registry/state.rs:241-407`.

### Checked and ruled out (this half matters as much)

* `SYMBOL_REGISTRY` / `WELL_KNOWN_SYMBOLS` / `REGISTERED_SYMBOL_DESCRIPTIONS`
  (`symbol.rs:112,270,132`) — `Symbol.for()` identity is spec-mandated to be
  shared, and the descriptions are deliberately Rust-owned off-arena
  (`symbol.rs:119-141` reasons the arena hazard through explicitly). Correct as
  designed.
* `CLASS_REGISTRY` / `PARENT_DENSE` (`object/class_meta_registry.rs:11,47`) —
  both key and value are codegen-assigned `u32` class ids. A static program
  fact, identical in every realm.
* `TYPED_ARRAY_VIEW_META` (`typedarray_view.rs:156`), `ITER_RESULT_KEYS`
  (`iter_result.rs:79`) — already `thread_local!`. `ITER_RESULT_KEYS`'s doc
  comment is the positive control: *"each `perry/thread` worker has its own
  arena, so a pointer interned from worker A's arena is a cross-arena read from
  worker B."*
* `DECLARED_FIELD_NAME_HASHES` / `PROTO_DESCRIPTOR_KEY_HASHES`
  (`object/descriptor_state.rs:215,222`) — FNV hashes driving a one-way
  "disable this fast path" latch. Conservative over-approximation.
* `ELEMENT_SHAPE_EPOCH` / `CLASS_SHAPE_GENERATION`
  (`array/element_shape.rs:167,170`) — the shape table itself is thread-local;
  the global counter is a "maybe stale" signal, so a false positive costs a
  re-proof.
* `GC_UNSAFE_ZONES` and the `OnceLock` policy caches across `gc/*` — env-var and
  OS-config derived, identical for every thread by construction.

## #6763 (`node:worker_threads`, 167 failing node-suite tests) — scoping

**Almost none of #6763 is downstream of #7988.** Sampling the failure list, the
failures are overwhelmingly *missing surface*, not realm aliasing:

* `BroadcastChannel` — missing `ERR_MISSING_ARGS` throws, missing
  `ERR_INVALID_THIS` brand checks, missing `DataCloneError`, `once` listener
  semantics, `MessageEvent` metadata, structured-clone of typed arrays.
* `MessagePort` / `MessageChannel` — missing `ERR_CLOSED_MESSAGE_PORT`,
  close-callback ordering, transfer-list validation.
* `environment-data` / `direct-message` — `getEnvironmentData` /
  `setEnvironmentData` and `postMessageToThread` are largely absent.
* `main-thread/prototype-surface` — `Worker.prototype.postMessage` is not on the
  prototype at all.

That is a **structured-clone + error-taxonomy + Web-ish-API project**, not a GC
or realm-isolation project. `node:worker_threads` is also a *different*
threading surface from `perry/thread`: it needs per-worker realms with an
explicit message port, whereas `perry/thread` is closure-shipping with
deep-copy. The realm-isolation work in this note is a **precondition** for
`node:worker_threads` behaving sanely once the surface exists — a `Worker` that
runs on an OS thread inherits every entry in the inventory table above — but it
closes approximately zero of the 167.

Recommendation: keep #6763 as an umbrella and split it by subsystem
(`BroadcastChannel`, `MessagePort`/`MessageChannel` + transfer lists,
`environment-data`, `Worker` surface/events, structured clone), per the repo's
"granular per-gap issues, not umbrellas" convention. It is not a task to
half-start alongside a runtime isolation fix.
