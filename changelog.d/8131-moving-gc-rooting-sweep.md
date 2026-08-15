### Fixed

- Root heap values held in Rust locals across calls that can run JS and
  therefore collect. Each was caught by a fault at the exact instruction
  under `PERRY_GC_PROTECT_FROMSPACE=1`, not by inspection:
  - the generic array-like callback helpers (`forEach`, `map`, `filter`,
    `some`, `every`, `find`/`findIndex`/`findLast`/`findLastIndex`,
    `reduce`/`reduceRight`) held the receiver, callback, result under
    construction, and current element across `js_closure_call*`; `map`
    wrote every mapped element through a pre-collection element pointer,
    landing in retired from-space;
  - `Function.prototype.call`/`.apply` held the callee, the explicit
    `this`, and the saved implicit-`this` across the invocation, then read
    the stale callee's header in the native-this alias check;
  - `js_put_value_set` held the receiver and property key across
    `ordinary_set_with_receiver` (which runs user setters) before the
    array-subclass `length` note dereferenced them.

- Keep perry-ext-http's listener dispatch on values the collector can see.
  Ext handle-struct side tables are rewritten by registered scanners, but a
  SNAPSHOT of one in a Rust local is a copy no scanner reaches: a drained
  listener vec went stale after the first callback's collection, and the
  pending-request struct parked in an mpsc channel went stale across the
  microtask-pump safepoint minors that run while the request waits. The
  emit helpers, deferred-listen drain and close callback now root their
  snapshots, and both request dispatchers re-read handler and listener
  lists from the scanner-maintained server handle at dispatch time. The
  orphaned `HttpPendingRequest::check_continue_listeners` field is removed;
  only the routing bit crosses the channel.

- Repair two GC scanner tests that were failing on `main`: they assert a
  root was rewritten, which is only observable if the collection actually
  moved the object, and evacuation is a C4b policy decision that
  legitimately declines under unit-test conditions. Their guards now force
  evacuation for their mutex-serialized lifetime.

### Added

- `perry_ffi::TransientRootScope` — a safe wrapper over a new extern
  surface onto the runtime's transient-handle stack, so ext crates can root
  the table snapshots they hold across JS callbacks.

- Instruments: the whole-heap from-space scan appends a classified payload
  preview to each offender, so the owner identifies itself instead of being
  an anonymous address; `PERRY_GC_STACKMAP_TRACE=1` prints every frame the
  native stack-map walk visits; `PERRY_EH_TRACE=1` prints per-frame
  personality decisions.

### Testing

- A deterministic moving-GC regression for `js_arraylike_map` (a callback
  that runs a copying minor on every invocation), sabotage-verified.
- `action_zero_pad_is_still_a_handler` pins that a zero call-site action is
  still Perry's catch — the shape of every JS `try` under native roots
  (#7982) — so reading the action as "handler vs cleanup" can no longer
  silently disable every statepoint-built catch. Sabotage-verified.
