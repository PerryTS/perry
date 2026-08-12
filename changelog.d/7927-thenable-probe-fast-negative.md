### perf(runtime): answer the promise-assimilation `then` probe without a property lookup

Resolving a promise with an **object** cost **+78.5 % instructions** versus
resolving with a **number** on byte-identical promise/microtask/closure
topology. The only differing counter was `thenable_probe`: 0 vs 24 000. The
cost is ECMA-262 27.2.1.3.2's `Get(resolution, "then")`, which every promise
resolution performs and which — for a plain object — answers `undefined` after
a `setjmp` exception frame, a re-intern of the literal `"then"`
(`js_string_from_bytes` + `core::str::from_utf8`), the fully generic dynamic
getter's preamble, a linear own-key scan with a `js_string_key_matches` per
key, and a recursive `js_object_get_field_by_name` walk into
`Object.prototype` for the miss. Roughly 72 000 times on
`gc-handoff/apps/asyncpipe.ts`, answering `undefined` every time — about 9 % of
that program (`gc-handoff/ASYNC2-NOTES.md` §4).

`crates/perry-runtime/src/promise/then_probe.rs` answers that probe directly
when — and only when — it can prove the answer. Both `get_then_action` (the
`Promise.resolve` / spec-resolve path) and `assimilate_via_then_property` (the
`await` path) consult it first.

**The hard part here is not speed, it is that a wrong answer HANGS.** A fast
path that misses a genuine thenable does not print a wrong value; the awaiting
promise simply never settles, which surfaces as a timed-out CI job someone
reruns. So the module fails CLOSED — every predicate returns "unknown" for
anything it cannot prove — and the proof is in two halves.

**Half 1, the receiver, is re-proved on every single probe and never cached.**
The value must be an arena-classified `GC_TYPE_OBJECT` with
`OBJECT_TYPE_REGULAR`, no `OBJ_FLAG_HAS_DESCRIPTORS` /
`OBJ_FLAG_ARRAY_DESCRIPTORS` / `OBJ_FLAG_TYPED_ARRAY_PROTO`, an admissible
class id, no own `then` key (a direct dense scan of the keys array — this is
the common real thenable and must always be found), and either
`OBJ_FLAG_NULL_PROTO` or no per-instance `setPrototypeOf` / `__proto__`
override. A `then` on the resolution object itself, or a prototype swapped onto
it, is therefore found immediately no matter how long the fast path has been
running.

**Half 2, `Object.prototype`, is cached under a signature** —
`(proto_addr, keys_array_addr, keys_array_len, header_flags, class_id,
PROP_PLAN_EPOCH, VTABLE_GEN)` — and the verdict itself is computed by calling
the REAL lookup, so it cannot disagree with the spec path at compute time.
Only staleness is a hazard, and the signature covers every route:

* `Object.prototype.then = f` (plain assignment, `Reflect.set`,
  `Object.assign`) — a new own data key must land in the keys array, so either
  `keys_len` moves or the array is transitioned to a different address;
* `Object.defineProperty` / `defineProperties` / `Reflect.defineProperty`, and
  `delete` — `prop_plan_epoch_bump()` (`object/descriptor_state.rs`,
  `object/delete_rest.rs`);
* `setPrototypeOf` / `__proto__` — `prop_plan_epoch_bump()`
  (`object/prototype_chain.rs`);
* a vtable getter/method named `then` — `VTABLE_GEN`;
* any garbage collection — the epoch is bumped by the intern-table root scan
  and the dead-owner prune, both of which run in every collection flavour.

Reusing `PROP_PLAN_EPOCH` rather than growing a parallel counter is deliberate:
its mutation hooks are already exercised by the store-plan and read-plan
caches, so this change adds no new invalidation hook that could be forgotten.
`ProtoSignature` derives `PartialEq` and is compared whole, so a route covered
by a new field cannot be dropped at the comparison site — and
`every_signature_field_invalidates_the_verdict` fails if any single field stops
participating.

Four residuals found by an arm-by-arm sweep of the entire `[[Get]]` path are
closed explicitly rather than argued away:

1. An **accessor** `then` on `Object.prototype` whose getter returns
   `undefined` reads as "no then" but is observable. Memoizing that would
   suppress every later invocation. The verdict now records the conservative
   answer whenever `get_accessor_descriptor(Object.prototype, "then")` exists,
   so the getter keeps running on every probe.
2. `js_object_get_field_by_name_f64` forwards a MISSED read to an aliased
   native handle (`http.Server.call(this, …)`), a layer above the lookup being
   modelled. `native_this_alias::alias_active()` now declines the fast path.
3. `subclass_backing_promise`'s `__perry_promise_backing__` probe is an
   INHERITED read that reaches `Object.prototype`, so
   `PROMISE_SUBCLASS_EVER` declines the fast path once any `class X extends
   Promise` has been constructed.
4. Admitting anonymous-**shape** class ids used to rest on two compile-time
   facts nothing checks at runtime. `class_id_admissible` now requires both
   `is_anon_shape_class_id` (positive set membership, which excludes every
   reserved builtin id) **and** `class_registry_inert` — no vtable entry, no
   class prototype object of either flavour, no parent edge, i.e. the entire
   input to the `class_id != 0` arms — memoized per
   `(class_id, VTABLE_GEN, PROP_PLAN_EPOCH)`.

Two instruments ship with it. `PERRY_MT_PROFILE=1` gains
`thenable_fast=N` plus a bucketed `[mt-profile] then_probe:` histogram naming
which gate declined — the first arm of this change measured a flat zero and the
counter is what said why (the objects were anon shapes, not class 0), instead of
the A/B looking like a null result. `PERRY_THENABLE_VERIFY=1` re-runs the full
spec lookup on every fast negative and aborts on disagreement, so a broken
invalidation dies loudly instead of hanging.

`test-files/test_gap_thenable_fastpath_invalidation.ts` is the behavioural
gate: thirteen cases, each of which WARMS the same call site with plain objects
before mutating, so it tests invalidation rather than first-use. It is
byte-identical to `node --experimental-strip-types` on the pristine baseline as
well, so it is a control rather than a self-fulfilling test.
