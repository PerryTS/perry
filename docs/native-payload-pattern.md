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
* `native_payload::payload_mut::<T>(this, &FAMILY)` is the receiver check and
  the payload borrow in one: `Ok(&mut T)`, `Err(Closed)` or `Err(Foreign)`.
* `native_payload::close(this, &FAMILY)` is explicit close: it drops the
  `Box<T>` now and releases the bytes. The object stays valid and later reads
  as `Closed`.
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
5. Do not hold the `&mut T` from `payload_mut` across `close` of the same
   object or across a call that can re-enter the family on the same object. If
   the method allocates or calls JS while holding it, root the receiver first
   (`RuntimeHandleScope::root_nanbox_f64`) and return `this` from the root.
6. Decode and validate arguments before borrowing when you can.

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
   re-states bytes when node keeps the object usable afterwards).
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

* Families with no native state (EventEmitter, AsyncLocalStorage, Headers
  metadata): plain fields on an ordinary object, no payload.
* Families whose node objects are streams with JS-visible state
  (`Sign`/`Verify` are `Writable`s with `_events` / `_writableState`): the
  stream object model first, then the payload for the native part.
* Values with a typed-receiver codegen path that passes the handle as an
  `i64` (`StringDecoder`): retarget the codegen rows in the same PR.
