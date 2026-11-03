### Fixed

Event/listener dispatch loops in several runtime and stdlib sites cloned
listener callbacks and call arguments into plain Rust locals, then called
into user code that could allocate and trigger a moving minor collection,
then reused those unrooted copies for the next listener. A `class X extends
EventEmitter` with an allocation-heavy listener could segfault dereferencing
a later listener's stale closure pointer — reproducibly, on a plain default
build with no GC environment knobs. `PERRY_GC_DIAG=1
PERRY_GC_PROTECT_FROMSPACE=1` isolated the fault to a retired-from-space
dereference of a `GC_TYPE_CLOSURE` object, and the crash disappeared under
`PERRY_GEN_GC=0` (full mark-sweep, non-moving), confirming the moving
collector as the cause.

The primary site is `crates/perry-runtime/src/node_stream_event_emitter.rs`
(`emit_stream_event`/`call_listener_args`) — the path every `class X extends
EventEmitter` subclass and every Node stream class (`Readable`, `Writable`,
`Duplex`, `Transform`) actually dispatches through. The same pattern is also
fixed in `perry-stdlib`'s `events.rs` (`js_event_emitter_emit`/`emit0`,
`dispatch_error_monitor`, `emit_meta_event`), `domain.rs`
(`emit_domain_event`), `worker_threads/worker_surface.rs`
(`stream_emit_event`), and `events/warnings.rs` (`emit_warning`). Every
listener snapshot and call-argument copy that must stay live across a
dispatch loop is now rooted through a `RuntimeHandleScope`, re-reading each
value's current (possibly relocated) address before every call instead of
trusting the pre-call copy.

Reconciled with #10490, which landed on `main` after this change was written
and fixes a different hazard in three of the same loops (`domain.rs`'s
`emit_domain_event`, `events/warnings.rs`'s `emit_warning`, and
`worker_threads/worker_surface.rs`'s `stream_emit_event`): the *displaced*
implicit `this`, saved across a listener call, was itself an unrooted local.
The two fixes are unioned rather than either superseding the other — taking
one side alone reintroduces the other's use-after-free. Each of those loops
now takes a single `RuntimeHandleScope` that roots the displaced `this` once,
before the first `js_implicit_this_set` and outside the loop, and restores it
from that root after every call, while the listener and argument copies are
rooted in the same scope and the arguments re-read immediately before each
dispatch. `events.rs`'s `call_emitter_listener` already carried #10490's
rooting and is unchanged by this reconciliation.
