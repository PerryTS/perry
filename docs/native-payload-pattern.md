# Native payload pattern: an ordinary object that owns native state

Tracking: #11919 (item P0). Code: `crates/perry-runtime/src/native_payload.rs`
(the API and its rules), `native_handle.rs` (the payload cell),
`native_class_ids.rs` (family ids). First families: crypto `Hash`, `Hmac`,
`Cipheriv`, `Decipheriv` (`perry-stdlib/src/crypto/hash_handles.rs`,
`cipher.rs`).

## The shape

A family's JS value is an ordinary `GC_TYPE_OBJECT`:

| Part | Where | Gives |
|---|---|---|
| family class id | `ObjectHeader.class_id`, from `native_class_ids.rs` | `instanceof`, receiver checks, the worker-transfer guard |
| prototype | one per family per realm, built on first use, rooted by the runtime | methods, `constructor`, `typeof` / `Object.keys` / `JSON` / identity of a plain object |
| own properties | ordinary fields set at allocation | node's enumerable own properties (`_options`, `_decoder`, ...) |
| payload | `ObjectMeta.native_state` = POINTER_TAG-boxed `GC_TYPE_NATIVE_HANDLE` cell owning a `Box<T>` | the native state, traced as a child edge of the meta record |
| JS state | hidden own key `#<perry:native-payload-js-state>` holding an ordinary object | every JS value the family keeps (listeners, pipe targets, options) |

There is no id, no registry, no address-keyed table and no root scanner. The
object is the only owner of its payload, and the collector sees that edge.

## Lifetime

* `native_payload::alloc(&FAMILY, payload, external_bytes, own_props)` creates
  the object and its cell and reports `external_bytes` to GC pacing
  (`gc_note_external_side_alloc`).
* `native_payload::alloc_closed(&FAMILY, own_props)` creates the same object
  and cell in CLOSED state. `lifecycle` distinguishes Open, Closing and Closed.
* `native_payload::payload_mut::<T>(this, &FAMILY)` is the receiver check and
  the payload borrow in one: `Ok(&mut T)`, `Err(Closed)` or `Err(Foreign)`.
* `native_payload::close(this, &FAMILY)` is explicit close: it drops the
  `Box<T>` now (or after the outermost active C call) and releases the bytes. The object stays valid and later reads
  as `Closed`. Release keeps `finalized = 0` and the owner edge traced.
  Only sweep and worker teardown finalize the cell. `attach` reopens the
  same cell; object identity, properties, prototype and links stay intact.
* If nothing closes it, the sweep that finds the object (and so the cell)
  dead drops the payload; a worker's teardown drops what is left. The drop
  runs exactly once on every path.
* `native_payload::set_external_bytes` re-states the retained bytes after a
  buffer grows or is released.

## Rules for a payload type `T`

1. Plain Rust data only: no JS values, no NaN-boxed bits, no GC pointers.
   Anything JS goes on the object (own property, or `native_payload::js_state`).
2. One payload type per family.
3. `Drop for T` allocates nothing on the GC heap, calls no JS and touches no
   thread-locals (it runs inside a collection or during thread teardown).
4. `external_bytes` counts memory the payload really retains (heap buffers it
   owns), re-stated when that changes. Do not count transient work buffers or
   bytes that were already handed to JS (#11549).
   A payload's working buffers come from `PayloadBuffer` (below), whose
   owner counts them exactly; add `owner.bytes()` into the stated bytes.
5. Do not hold the `&mut T` from `payload_mut` across `close` or `attach` of the same
   object or across a call that can re-enter the family on the same object. If
   the method allocates or calls JS while holding it, root the receiver first
   (`RuntimeHandleScope::root_nanbox_f64`) and return `this` from the root.
6. Decode and validate arguments before borrowing when you can.

## Working buffers (`PayloadBuffer`)

Code: `perry-runtime/src/native_payload_buffer.rs`; binding crates use
perry-ffi `native_payload::buffer`. A codec's large working memory (a
window, hash tables, an output scratch) is raw bytes owned by the payload:
never traced, never a JS value or a GC pointer.

* `PayloadBuffer::alloc(owner, len) -> (ptr, len)`, `grow`, `shrink`,
  `release`. Blocks are zeroed and 16-byte aligned. The
  `PayloadBufferOwner` lives in `T` and counts the exact bytes of its live
  buffers; the family adds `owner.bytes()` into every external-bytes
  statement (`alloc`, `set_external_bytes`, `StepOut::external_bytes`).
  perry-ffi's `BufferOwner` keeps the owner behind an `Rc`, and its
  `PayloadBuffer` frees itself on drop.
* `Drop for T` frees the buffers. `close` (a stream's `destroy()`) drops
  `T` at once, so the bytes return before any collection; the sweep or
  worker teardown is the backstop for a payload nobody closed.
* A C codec with a custom allocator hook gets the owner as
  `alloc(opaque, size)` / `free(opaque, ptr)` (`BufferOwner::hook()`,
  brotli's `CAllocator` shape; the size rides in a 16-byte prefix). Keep an
  owner clone in the codec state so `opaque` outlives every block.
* Every byte comes from one internal function, `backing`, today the global
  allocator (mimalloc on 64-bit). The region page-run allocator replaces
  that function only.
* zlib (`perry-ext-zlib`): brotli's state and every brotli allocation (both
  directions), the inflate state with its 32 KiB window, and the output
  scratch are buffers. The deflate compressor's arrays are boxed inside
  miniz_oxide and zstd allocates in C without a hook here, so those stay
  on their own allocators with their sizes stated as before.

## Per-family conversion checklist (Codex lanes)

For family `X` (currently `register_handle` / `register_reclaimable_handle`
plus dispatch-hub arms):

1. Measure node first: `Object.keys(x)`, `JSON.stringify(x)`,
   `x.constructor.name`, `Object.getOwnPropertyNames(Object.getPrototypeOf(x))`,
   `x instanceof <export>`. Write them into the gap test.
2. Take the next id in `native_class_ids.rs`: add the `pub const`, append it to
   `ALL`, move `NATIVE_BACKED_LAST`, extend the transfer-guard test. Keep the
   prototype-slot assertion in `native_payload.rs` green (raise
   `PROTOTYPE_SLOTS` if needed).
3. Turn the handle struct into the payload `T`: drop `Mutex`es (the payload is
   single-threaded), move every JS value (closure bits, NaN-box words,
   `Vec<i64>` listeners) out to the JS-state object or an own property.
4. Declare `static X_FAMILY: NativePayloadFamily` with `install_prototype`
   installing each method via `PayloadPrototype::method(name,
   fn_info!(thunk, n; with_declared(n), with_flags(FN_BUILTIN)), n)`. Each
   thunk is `extern "C" fn(*const ClosureHeader, JsThis, f64 x n) -> f64`;
   strip trailing `undefined`s if the old code looked at `args.len()`.
5. The creator returns `native_payload::alloc(&X_FAMILY, payload, bytes,
   &[(b"own", value), ...])`.
6. Explicit close / end / final / digest calls `native_payload::close` (or
   re-states bytes when node keeps the object usable afterwards). Reopen is
   `lifecycle` → open the C resource → `attach` into the same CLOSED cell.
   `AttachMiss::{Open, Closing, Finalized, Foreign}` reject the install and
   drop its input; never replace a live/busy payload or revive a finalized cell.
   Reopenable families stamp `next_open_serial()` in the payload, every child
   and resource completion; check `OpenSerial` equality before resource use.
7. Add `("module", "Export") => class id` to
   `native_payload::export_class_id` so `instanceof` answers.
8. Delete, in the same PR: the `register_*handle` producer, the dispatch-hub
   method arm and property arm, `retain_strongly` calls, the family's root
   scanner and thread-exit payload releaser, async pump hooks that only served
   handle ids, `native_table` rows that pass the id, and any
   `registry_lifetime_allowlist.json` entry the deletion makes stale.
9. Run `python3 scripts/native_handle_ledger.py --update` (counts may only go
   down) and regenerate `gc_effects` tables if a creator now allocates.
10. Tests: a gap test with node's expected output (shape, identity, `Map` /
    `WeakMap` keys, `instanceof`, errors after close), a churn test (200k
    create/use/close cycles flat in RSS), and run it with
    `PERRY_GC_SCHEDULE_SEED` (seeded moving GC).

## Not this shape

* Families with no native state (EventEmitter, Headers
  metadata): plain fields on an ordinary object, no payload.
* Families whose node objects are streams with JS-visible state
  (`Sign`/`Verify` are `Writable`s with `_events` / `_writableState`): the
  stream object model first, then the payload for the native part.
* Values with a typed-receiver codegen path that passes the handle as an
  `i64` (`StringDecoder`): retarget the codegen rows in the same PR.

## Families whose native side calls JS

Classify the family before conversion. The shape and object remain the
source of truth: no owner registries, address maps or per-family latches.
`NativePayloadFamily::links_owner` opts into one traced, NaN-boxed owner slot
on the stable malloc cell. Both marking and relocation visit it. The owner store uses
`runtime_write_barrier_external_slot`, the exact-slot barrier that recognizes
malloc parents (`runtime_write_barrier_slot` only remembers old arena parents). The cell also has a traced `callbacks` slot, mirroring the runtime-owned
JS-state callbacks array. Both edges use the exact malloc-parent barrier and
are visited for marking and relocation. Callback families allocate a traced
16-byte extension after the unchanged 136-byte header (on 64-bit targets):
one traced array word and one native catch-token pointer. The token contains
no JS values and is installed only for the duration of `NativeCallGuard::call`.
Legacy and N cells keep their layout and size; the owner stays zero for N.

| Class | Native callback lifetime | Examples | Owner edge |
|---|---|---|---|
| S | Registered synchronous C callback, through userdata | node:sqlite DatabaseSync UDF, aggregate/window, authorizer | `links_owner: true` |
| A | Completion delivered later by the event pump | net/tls, http/ws, zlib/fs streams, fs.watch, child_process, readline, MessagePort | `links_owner: true` |
| C | Callback arguments live for one call | sqlite Session/applyChangeset filter and onConflict | stack userdata with runtime handles |
| N | No JS callbacks | crypto, StringDecoder, sync zlib, rustls verifier; bun:sqlite/better-sqlite3 today | `links_owner: false` |

Statements, SQLTagStore and iterators are S entries through their database
owner, held in their own JS state. A one-shot zlib callback has no payload
owner; its parked root callback keeps its existing lifetime.

For every class, keep JS values in own properties or the hidden JS state,
never in `Box<T>` or C memory. Registered callbacks use `set_callback` with
a fixed index into `callbacks`. Per-invocation JS values (aggregate
accumulators) live in JS-state arrays; C stores only a `u32` slot index.

S checklist:

1. Set `links_owner: true`. Obtain `owner_link` and pass
   `sites.site(link, index)` as userdata. Never pass a runtime handle, moving
   object address, or a Box registered in a set. Declare the C resource
   before `CallbackSites`, so it closes first. C xDestroy does not free sites.
2. A trampoline calls `link_owner`; `None` returns the C error or no-op
   without JS. Root the owner immediately, and read it only through that
   handle after allocation or JS. `callback_from_link` reads the traced
   callbacks slot after the last argument allocation, without owner-key
   lookup. A trampoline whose only JS is one registered callback with
   arguments that allocate nothing calls `call_callback(link, index, args)`:
   one owner check (open, this thread, nothing pending), the slot read and
   the call. Use `set_callback`, `state_set`, or `state_set_memo` to replace or
   clear the callbacks array; these keep the state field and cell slot in sync.
3. Call JS only through `call_from_native` / `call_from_link` /
   `call_callback`. A native call (`NativeCallGuard::call`) captures one
   handler at its first callback and reuses it for the rest; between
   callbacks only runtime handle scopes may differ (a trampoline's argument
   roots), never shadow frames or other managed stacks. It catches a throw, parks the
   exact value in `pendingException`, sets PENDING and returns `Err(())`.
   Further callbacks return the error without running JS: the first throw
   wins. Never throw through C or use throwing thread-validation helpers.
   Park validation errors with `set_pending_exception`; it never calls JS and
   preserves the first pending value.
4. Root the receiver, copy the raw C resource out of the payload, and release
   the borrow before calling C. Hold neither `&mut T` nor a mutex over a call
   that can re-enter JS. Bracket every callback-capable call (step, exec,
   prepare, close, backup, changeset_apply) with `enter` and `finish`.
5. A loop may obtain `owner_link` once while its owner stays rooted, then
   use `enter_link` per C call. This checks OPEN and creator-thread affinity
   on every entry without validating the moving JS receiver again.
   `NativeCallGuard::owner_link` exposes the link already validated at entry.
   Prefer unsafe `guard.call(|| native_call())`: it finishes immediately and
   shares a lazily captured catch savepoint across the call's callbacks.
   Each callback still arms its own C trampoline, so a throw never jumps
   across the native library. Between callbacks the trap is inactive; the
   cached new.target remains traced and rewritten. Root depths are refreshed
   per callback, including argument-conversion scopes. Native helpers within
   this span may not change other managed stacks. `enter`/`finish` remains
   available with ordinary per-callback capture for existing callers.
   Finish immediately when C returns, before result conversion or anything
   that can throw. `CallEnd::Threw(value)` throws that exact value outside C;
   `CallEnd::Closed` throws the family's closed error without converting or
   returning partial results. A callback throw takes priority over close.
   The guard is explicitly finished, not Drop
   based. Nested entries consume their pending throw before returning to the
   outer callback's JS.
6. `close` returns `Closed`, `Deferred`, `AlreadyClosed` or `Foreign`.
   While busy, it sets CLOSING; payload access and `link_owner` see closed
   immediately. The outermost finish drops the resource. Finalization marks
   the cell finalized **before** invoking its drop thunk, so C destruction
   callbacks cannot read a dead owner or call JS. Explicit release instead
   keeps CLOSING set throughout its drop thunk. Clear the JS-state callbacks
   array at close, so old registrations do not survive reopen. Every child
   checks its `OpenSerial` against the current payload before entry.

C checklist:

1. Use per-call stack userdata whose JS values are `RuntimeHandle`s, covered
   by the catch savepoint. No current-owner TLS stack or latch.
2. Use the owning payload's catch/pending/finish protocol for callbacks; keep
   decoding, conversion and the eventual rethrow outside the C guard span.
3. Release payload borrows and locks before calling C, as for S. Handle
   `CallEnd::Closed` before result conversion, as for S.

A checklist:

1. Set `links_owner: true`. Queues carry `OwnerLink`, never an id. Native
   workers only send inert links; the owner thread dereferences them.
2. Call `link_ref` once per queued item from queueing until dispatch, plus
   once per outstanding operation and once per ref'ed native handle. Its 0→1 transition pins the **cell** via `pin_object_non_young`;
   the pin traces the owner without arming the young-pin latch. Balance every
   ref with `link_unref`, including after explicit close. The last unref
   unpins the cell. A bare link is not a root.
3. Terminal events use `link_event_owner` (OPEN, CLOSING or CLOSED) and root
   its result. Read listeners from JS state at dispatch time, including
   listeners added after destroy. Resource events also require OPEN and a
   matching `OpenSerial`. `None` or a stale event drops the item; always unref
   afterwards, including a throw through the pump's normal uncaught path.
4. Before close, release the keep-alive ref recorded in T. Queue each terminal
   end/error/close item under its own ref, then release the payload. For async
   fd close, move the raw fd from T into the job; release T immediately and
   deliver the job's completion under its own ref.
5. Keep cells alive until queued links are unref'ed. Worker teardown stops
   the pump, drops queued plain items without running them or unrefing from
   Drop, then finalizes every cell including pinned ones. Never dispatch or
   dereference queued links after heap teardown. Pending refs die with it.

N checklist:

1. Set `links_owner: false`; leave the cell's owner zero.
2. Use the ordinary payload conversion checklist. Add no callback sites,
   keep-alive pins or pending-exception machinery.

The first S family is node:sqlite (`perry-stdlib/src/sqlite/database_sync.rs`,
`statement_sync.rs`, `sqlite_callbacks.rs`, `tag_store.rs`, `session.rs`):

* `DatabaseSync` owns the `sqlite3*`; its payload exists only while open.
  Everything `open()` needs to reopen (path, flags, limits) is plain data
  kept in the JS state, so `close()` / `open()` keep the same object.
* A statement owns no C resource: it keeps its SQL and options and compiles
  them on its database per run, entering through the database. Children
  carry the database's `OpenSerial`.
* Each `sqlite3_step` is one guard; rows are converted after `finish`
  with non-throwing converters, and an error finalizes the statement before
  the throw. Resets that can run `xFinal` stay inside the guard, so a
  pending exception keeps them out of JS.
* The payload's `Drop` deletes sessions, finalizes any statement still on
  the connection (the outer call of a deferred close), then
  `sqlite3_close_v2`; the outer frame sees the database closed and forgets
  its statement. A C child that must die before the connection (a
  `sqlite3_session`) is shared through one `Rc` cell by the database
  payload and the child payload: whichever goes first deletes it.
* A statement owns the `sqlite3_stmt` its `prepare()` compiled, as node's
  does (the authorizer runs once per `prepare()`; `expandedSQL` reads the
  last bindings). Every run clears and rebinds it and leaves it reset, so
  finalizing it never calls back. The connection's close finalizes it; the
  open's `live` token (an `Rc<Cell<bool>>` shared with the database
  payload) tells the statement's drop whether it still must.
* node's own accessors (`stmt.sourceSQL`, `SQLTagStore#size`) are born with
  the instance: `alloc_with_state(.., accessors, &BIRTH_MEMO)` installs them
  at the site's first birth and records the final shapes; later births are
  allocated in them, holding the family's shared accessor pairs. One-off
  objects (`db.isOpen`, `db.limits.*`) still use
  `native_payload::define_own_accessor`; a constructor export materializes
  its prototype when the export is created (`native_payload::prototype`).

In each family conversion PR, delete its callback id registries and scanners,
`js_write_barrier_root_nanbox` callback "rooting", and listener/pipe tables
keyed by id. For sqlite this includes NODE_SQLITE_CUSTOM_FUNCTIONS,
NODE_SQLITE_CUSTOM_AGGREGATES, NODE_SQLITE_ACTIVE_AGGREGATES,
scan_node_sqlite_roots_mut and release_node_sqlite_authorizer_in_freed_ranges.
These deletions belong to the family lanes; the runtime API adds none of them.

## Stream families (STREAM-PAYLOAD-DESIGN)

A family whose node objects are streams (zlib's `Gzip`…, crypto's `Hash`,
`Cipheriv`, `Sign`…) is an ordinary runtime Transform/Writable whose payload
holds only the codec. There is one stream state machine: the runtime's
writable buffer, readable queue, listeners and pipes.

* **Hooks.** The payload cell's `finalizer` word names a static
  `PayloadVTable { drop, stream }`; a stream family's vtable carries its
  `StreamHooks { kind, timing, lazy, step, error, release }`
  (`node_stream/native_hooks.rs`). The runtime reaches them from the object:
  `meta.native_state` -> cell -> vtable. In-tree: `impl StreamPayload for T
  { const HOOKS = &T_HOOKS; }` and `native_payload::alloc_stream` /
  `attach_stream_to_object`. Binding crates: perry-ffi `native_stream`
  (`payload_vtable::<T>(Some(&HOOKS))`, layout-checked by
  `js_perry_stream_abi_layout`).
* **Construction** mirrors node's constructor body: allocate with the family
  prototype (whose chain reaches `Transform.prototype` / `Writable.prototype`)
  and node's pre-fields, then `node_stream::init_transform_in_place(obj,
  opts)` (or `init_writable_payload_in_place`), then the post-fields. The
  instance owns no methods; its runtime state is non-enumerable.
* **Steps.** Every write reaches `step(payload, &StepIn, &mut StepOut)`
  through the writable machinery (serialized writes, the buffered-write
  array). `step` calls no JS, allocates nothing on the GC heap and keeps no
  pointer into `StepIn::input` (borrowed from the traced chunk for that step
  only). Its output (in the payload's scratch) is copied into an
  exact-length Buffer and pushed through the runtime's `push`. `MORE` steps
  again, `NEED_INPUT` completes the write, `ENDED` (Final only) ends the
  readable side, `ERROR` destroys with `error(owner, code)`.
  `StepOut::external_bytes` restates the cell's external bytes after every
  step. The runner parks while the readable side is at its highWaterMark
  (node's Transform `kCallback`) and resumes when it drains.
* **Timing.** `DEFERRED` (zlib) runs the steps from an immediate (`data`
  never fires inside `write()`); `INLINE` (crypto) runs them inside `write()`.
* **Release.** `destroy()` and autoDestroy call `release(owner)` (close the
  payload; zlib also sets `_handle = null`) synchronously, before any event
  is queued. Deferred jobs hold the stream as a traced capture, so a stream
  family is `links_owner: false`.
* **Overrides.** A JS `_transform`/`_flush` captured at init (an option, or a
  subclass override) takes precedence over the hooks; the family prototype's
  builtin `_transform` runs the same step for `super._transform(...)`.
