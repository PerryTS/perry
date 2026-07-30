### Fixed

- **`typed_feedback_trace_dump_runs_before_entry_return` has been red on `main`.**
  Codegen was never wrong: `add_pre_return_void_call` splices
  `js_typed_feedback_maybe_dump_trace()` in front of every `ret` in `main`, and
  it still does. The test was measuring the wrong thing. It compared
  `ir.rfind("call void @js_typed_feedback_maybe_dump_trace()")` against
  `ir.rfind("ret i32 0")` over the whole module text. `main` has more than one
  return: the host-return early exit returns a literal `i32 0`, the event-loop
  exit returns the pending exit code. So the two `rfind`s landed on unrelated
  sites — the last dump sat after the first return, and the assertion failed
  while the property it names held.

  The test now slices the IR to `main`'s body and checks every return site: each
  `ret` must be immediately preceded by the dump call. Removing the
  `add_pre_return_void_call` in `crates/perry-codegen/src/codegen/entry.rs`
  makes it fail, which the old shape could not guarantee.

  The PR workflow now uses diff-based selection in `e2e-scoped`, so changing
  this integration test pulls its suite into the per-PR run.
