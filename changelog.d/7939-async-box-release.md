### Fixed

- **A completed `async` activation no longer retains its body locals forever (#7933).**
  The async-to-generator transform boxes every body local of an `async` function
  into a `js_box_alloc_bits` cell, and the runtime's `BOX_REGISTRY` is
  **monotonic by design** — cells are never freed, because perry#4898's
  pointer rejection and #7906's positive pointer cache both rest on "a
  registered address can never become unregistered". `scan_box_roots_mut` marks
  the JSValue inside every registered cell on every collection, and nothing ever
  cleared one, so **every local of every activation the program had ever run
  stayed a live GC root**.

  The state machine now **clears** (never frees) the cells at its terminal
  states, which keeps every address registered and readable — a stale reader
  sees `undefined`, already the defined value of an uninitialised boxed local
  (perry#4926).

  Terminal states of the `was_plain_async` step driver, and there are exactly
  two: the `IterResultGetDone` resolve arm (the body ran to a `return`, which
  `prepend_done_before_returns` pairs with `__gen_done = true`) and the catch
  arm's `isError` branch (an exception escaped with no user `catch` to take it,
  so the activation rejects). Everything else either suspends
  (`AsyncStepChain`) or re-enters the step (`__step_self(e, true)`) and reaches
  one of those two later. A resume that still arrives after the terminal is
  harmless: it writes `__gen_sent` before reading it, then short-circuits on
  `__gen_done` — which is deliberately **not** cleared, since an `undefined`
  there would drop it into the dispatch loop with no matching state.

  Clearing a cell whose value is still reachable would be a *silent* wrong
  answer, not a crash, so a cell is cleared only when no closure can hold its
  address. A box address is never a JS value — it leaves the activation solely
  through a closure capture slot, which codegen fills for exactly the ids in
  `compute_auto_captures(closure) ∩ boxed_vars`. The new
  `generator/box_release.rs` computes a **superset** of that set (explicit
  `captures` *and* `mutable_captures`, plus
  `perry_hir::analysis::collect_local_refs_expr` over the whole closure,
  descending into nested closures), so an id it misses is an id codegen's own
  free-variable walk also misses — no capture slot exists for it and clearing is
  unobservable. Sloppy-mode `with` is the one construct that breaks the
  argument (`Expr::WithGet`/`WithSet` carry a fallback `LocalId` as a leaf field
  the shared walk does not report), and poisons the analysis outright: nothing
  is released for such a body.

  Async generators and sync generators are deliberately untouched — their
  `{next, return, throw}` object is user-visible, so "done" is not the end of
  observability.

  **Effect on `asyncpipe` at 240 batches** (output byte-identical, exit 0):
  young survival at the first copying minor **770 ‰ → 24 ‰**, objects moved
  **172 387 → 6 658**, `freed_bytes` **4.09 MB → 17.38 MB**, and the second
  minor — 201 822 objects *promoted* into old-gen — **no longer happens at
  all**. Instructions retired −31.9 %, peak RSS 94.5 MB → 57.8 MB. At 480
  batches the three minors go 770/943/766 ‰ → 24/9/14 ‰, instructions retired
  −45.5 %, peak RSS 189.1 MB → 91.1 MB.
