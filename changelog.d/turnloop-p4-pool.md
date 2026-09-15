### turnloop P4 — blocking and CPU-bound work on turnloop's shared pool, and the perry-ffi async ABI v2

Perry's "work that must not run on the JS thread" ran on four different
mechanisms, and two of them did not run off the JS thread at all:

- tokio's blocking pool, for `bcrypt` and every `perry_ffi::spawn_blocking` caller;
- one fresh `std::thread` **per queued N-API async work item**;
- nothing — `argon2.hash`, `crypto.pbkdf2`, `crypto.scrypt` and the `zlib`
  one-shots derived/compressed **inline on the thread that owns the JS heap**
  and deferred only the *callback*, so the API looked asynchronous while a
  two-million-iteration `pbkdf2` froze every timer, socket and immediate in the
  process for the whole derivation. Node runs all of these on libuv's
  threadpool; `test-files/test_gap_turnloop_p4_pool.ts` pins the difference.

`crates/perry-runtime/src/turnloop_pool/` replaces all of them with one
mechanism: an owned `Send` closure on turnloop's process-wide bounded pool,
whose result arrives as an ordinary turnloop completion on the submitting
thread. The split is a trait bound rather than a convention — `work` is `Send`
so it cannot touch the JS heap (#1824), `deliver` is not `Send` and is where
JSValues are built. Exactly one delivery per accepted job, including cancel,
panic and loop shutdown (turnloop DESIGN D4); a submission the driver refuses
delivers nothing and the caller keeps its own fallback.

**perry-ffi async ABI v2** (`perry_ffi::pool`, backed by
`perry_ffi_pool_submit` / `_cancel` / `_turn`): `submit`, `submit_or_run_inline`,
`run`, `cancel`, `turn`. `perry_ffi_run_pending` becomes a v1 shim that takes a
bounded turnloop turn before driving tokio. `spawn_blocking`,
`spawn_blocking_with_reactor` and `spawn_async` stay on tokio deliberately —
their remaining callers hold a thread for the lifetime of a *connection*, which
a fixed-size pool cannot host; P5–P7 rewrite them and P8 deletes them.

Moved: `bcrypt` and `argon2` (stdlib and the ext crates), `sharp`,
`crypto.pbkdf2` / `crypto.scrypt`, the `zlib` one-shot codecs, and
`napi_queue_async_work`. `perry-ext-bcrypt` and `perry-ext-argon2` also stop
allocating their result string on the worker thread, which was the #1824 hazard
perry-stdlib's copies had already worked around.

`PERRY_LOOP_STATS=1` gains a `[perry-loop] p4` line with the pool's lifetime
totals, including `refused=` — a nonzero value means the caller's own fallback
ran and the pool was *not* the transport.
