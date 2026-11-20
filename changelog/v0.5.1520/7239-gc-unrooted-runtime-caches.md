**GC: root the ten unrooted runtime-side caches, and close a scanner that walked one of three sibling slots (#7231).**

The class #7226 established — a runtime table holding a GC pointer that is not a
registered root. It is strictly worse than the #7154 stale-register class (it goes
bad at collection #0 and stays bad, rather than needing a collection to land in a
narrow window) and it is **invisible to every static instrument**, because
`gc_root_dominance_check.py` reads emitted LLVM IR and a runtime table is not in it.

★ **`process.env`** is the load-bearing one. `js_process_env_impl` builds it once
with `js_object_alloc` — the nursery — and caches it in a thread-local `Cell<f64>`
that is the *entire* reference graph: `process.env` is a getter CALL, not a field of
the `process` object. The first minor swept or evacuated it, and every later
`process.env.X = v` wrote through a dangling pointer. The observable is
ENUMERATION (`Object.keys`, `for…in`, spread — how `@next/env` and `dotenv` consume
it), because a direct read lowers to `js_getenv` and asks the OS. Measured at
`c9cd73ba5` under `PERRY_GC_MOVING_LOOP_POLLS=1` at compile and run:
**SIGBUS 10/10 before, `bad 0` 10/10 after**, byte-exact against node 26.5.1, clean
on the shipped default both sides. The sibling `PROCESS_FINALIZATION_OBJECT` uses the
same materialize-once idiom and was already rooted — this was an omission, not a
design.

Also rooted: `CACHED_PERMISSION` and `CACHED_REPORT` (same shape; the
`runtime_write_barrier_root_nanbox` beside the first is an incremental *mark*
barrier, not a root registration); `ERROR_CONSTRUCTOR_PTR` (a raw duplicate of a
`globalThis` closure, outside the object graph, so stale after a move);
`tui/input.rs` `INPUT_HANDLER` (the inline `useInput` arrow, which nothing else
refers to); `tty.rs` `RESIZE_CALLBACK` (bypasses the rooted EventEmitter listener
array); `frame.rs` `FRAME_CALLBACKS` (rooted only transiently during registration —
its `unsafe impl Send` SAFETY comment asserted the opposite and is corrected);
`CURRENT_NEW_TARGET`; `ACCESSOR_RECEIVER_OVERRIDE`; and `PENDING_FETCH_SIGNAL`.

**Scanner gap**, the shape #7230 found twice: `worker_threads.rs`'s
`scan_parent_port_event_roots_mut` visited `MESSAGE_EVENT_CALLBACKS` and neither
`MESSAGE_CALLBACK` nor `CLOSE_CALLBACK` — three slots in the same `thread_local!`
block holding the same raw `ClosureHeader*`. `parentPort.on('message')` /
`on('close')` handlers were reclaimed by the next collection inside a worker.

Two further windows closed in `frame.rs` while rooting its queue: `js_frame_tick`
drained into an unrooted local `Vec` and rooted each callback only as it invoked it,
leaving the rest of the batch naked across arbitrary user code (#7230's
staging-buffer shape); and `js_on_frame_callback` held the queue mutex across an
allocating `capture_context()`, which becomes a self-deadlock once a scanner locks
the same mutex.

**Refuted, and worth recording.** All 8 budgeted (FULL, STEP) scanner pairs were
diffed field-by-field: **no drift** — #7230's `IntervalTimer.args` fix reached both
twins. `buffer/header.rs`'s nine address registries are not root gaps
(`GC_TYPE_BUFFER`/`GC_TYPE_TYPED_ARRAY` are `movable: false`; they are identity sets,
not liveness references). `static_plugins.rs` is unreachable — `perry_register_static_plugin`
has no caller in the workspace.

Not closed, and stated: `promise/rejection.rs`'s `internally_handled` needs a
**rekey** rather than a root; the save/restore idiom in `CURRENT_NEW_TARGET` and
`ACCESSOR_RECEIVER_OVERRIDE` still parks the displaced value in a bare Rust local;
and `MODULE_PATH_REGISTRY` is process-global while arenas are per-thread, so a naive
scanner would be unsound.
