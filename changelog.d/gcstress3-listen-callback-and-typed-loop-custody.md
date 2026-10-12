fix(gc): two runtime-held values that crossed a collection unrooted — the `listen(cb)` callback across the bind, and the `%TypedArray%` callback loops' receiver/callback/result

Two gc-stress matrix failures on `main` (runs 38254496708 and 38244340549).
Both had causes other than #12462.

**1. `test_gap_gc_http2_pending_event_callback_rooting`: `value is not a
function` in 11 evacuating cells.** `PERRY_GC_PROTECT_FROMSPACE=1` faults in
`js_closure_call0` under `deferred_events::call_deferred_callbacks`, on a
retired closure (`obj_type=4`) from minor #0. That frame is the `'listening'`
drain, so the stale value was the `server.listen(0, cb)` callback. Every bind
path (`listen_http_server`, `listen_https_server`, `listen_http2_server`) took
`ListenArgs.callback` into a Rust local. It then ran the options
`JSON.stringify` (`extract_port`/`extract_host`) and the bind itself, and the
bind allocates the listener's JS objects. Only after that did it push the
callback onto the server. A copying minor in that window moved the closure,
and the server ended up storing its pre-move address. The posted-bind route
already registered the callback first, so the bug was only on the direct
route.

The fix moves custody onto the holder. `ListenArgs`' fields are now private,
and `ListenArgs::settle` is the only way to consume it. `settle` hands the
callback to a `park` closure first; every caller then registers it on the
server's scanned `listeners`/`deferred_listen_cbs` (`register_listen_callback`).
Only after that does `settle` resolve `opts`, with ONE stringify: the old
two-call `extract_port` + `extract_host` would pass the second call a
pre-collection copy of an options object. `queue_deferred_listening_emit` lost
its callback parameter, so no path can carry a raw listen callback past the
bind any more. A failed bind now withdraws the callback on every route
(`withdraw_listen_callbacks`), as the posted route already did.

**2. `test_gap_gc_uint8_buffer_callback_rooting`: `protoMap 1` in the 5
non-generational cells (`gen_gc_off`, `wb_off`, `gen_off_verify`,
`wb_off_force`, `all_four`). Nothing moves in these cells.** The problem is
reproducible from a single call: `Uint8Array.prototype.map.call(new
Uint8Array([3,1,4]), v => { churn(); return v + 1 })` under
`PERRY_GEN_GC=0 PERRY_GC_HEAP_LIMIT=8` returns `[object Object]`. A hardware
watchpoint on the result's header shows the following. The thunk reaches
`js_typed_array_map` (the registered-TypedArray arm, not the Buffer
dispatcher). The result is created there with `store_alloc`. Then
`IncrementalSweepState::step` → `defer_old_unregister` clears it during the
user callback's `js_gc_loop_safepoint`, and the caller is handed a swept cell.
`typedarray/iterate.rs` kept the receiver, the callback and `map`'s fresh
result in Rust locals across every callback. It relied on the argument that
TENURED TypedArrays do not move. That is true for relocation, but a full
mark-sweep still reclaims what nothing marks, and a closure both moves and
dies. All nine loops (map/filter/every/some/forEach/find/findIndex/reduce/
reduceRight) now take that custody from one type, `CallbackLoop`. It roots the
receiver for liveness and keeps its non-moving raw address for element reads.
It roots the callback and re-reads it before every call. `keep()` roots a
result for the rest of the loop. The callback is no longer passed to the loop
bodies.

Negative controls:
- `gc::tests::buffer_b4::typed_callback_loops_retain_receiver_callback_and_result`
  runs a full mark-sweep inside each callback. It asserts that the receiver and
  callback are marked, and that `map`'s result comes back as the same live
  Int32Array with the right values. A new `PERRY_B4_SABOTAGE=typed_loop_root`
  arm drops the loop roots, and `each_compatible_b4_sabotage_turns_its_witness_red`
  requires the witness to go red under it.
- Both gap tests are red on `main` in exactly the cells named above. With the
  fix they match node in every cell, including seeds 1–5 under
  `PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_VERIFY_EVACUATION=1`.
