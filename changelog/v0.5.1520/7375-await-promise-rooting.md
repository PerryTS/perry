### Fixed

- **`await` polled a promise that the event-loop pump had already moved.** The
  await loop's `wait` block calls `js_promise_run_microtasks_await_loop`,
  `js_run_stdlib_pump` and `js_await_loop_tick_timers` — all of which allocate
  and can drive an evacuating minor — then branches back to `check`, which
  re-unboxes the *same* SSA value. After one pump the loop polled retired
  from-space, and `js_promise_state` dereferenced it.

  The existing comment there ("unbox the promise in each block that uses it")
  addresses LLVM's dominance requirement, which is a different problem: the box
  itself named the pre-collection promise, so unboxing it again in every block
  reproduced the stale address five times rather than fixing it.

  The promise now takes a temp root once, and every block re-reads the slot the
  collector rewrites instead of reusing the register.

  Closes 4 of the 31 quarantine catches in #7341 — all `obj_type=5`
  (`GC_TYPE_PROMISE`) with frame #1 in generated code.
