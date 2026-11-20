### Fixed

- **`headers.forEach` handed the collector's from-space to `js_closure_call2` — the
  residual #8163 holder, and the one that is production-visible under the DEFAULT GC
  (#8217).**

  `js_headers_for_each` stripped the NaN-box off its `callback` argument into a bare
  `*const ClosureHeader` **before** the loop and never re-read it. That copy lives only
  in a native Rust frame — the precise root map does not cover Rust frames, and
  production resolves the conservative stack scan to `SkipDisabled` — so the first
  copying minor inside the loop left it naming retired from-space. Every iteration
  allocates twice (`js_string_from_bytes` for the value and the key) and then runs
  arbitrary user JS, so the window is wide open.

  The consumer is Next's `pipeToNodeResponse` header copy, which the production App
  Route fixture runs once per response: `c.headers.forEach((a, c) => … b.appendHeader(c,
  d) …)`. `js_closure_call2` → `get_valid_func_ptr` reads `CLOSURE_MAGIC` at
  `closure + CLOSURE_TYPE_TAG_OFFSET` (12). Once the retired block has been recycled into
  Eden and partly overwritten the magic no longer matches, `get_valid_func_ptr` returns
  null, and `dispatch_proxy_callee_or_throw` falls through to `throw_not_callable()` —
  `TypeError: value is not a function`, thrown mid-header-copy, so Next never writes the
  body (`E180 failed to pipe response`) and the client sees an **empty body**. That is
  every discriminator #8163 recorded, including why the failure is never a wrong
  id/header/cookie: the response content was already correct, only the write was
  abandoned. It also explains the ~10× gap between stale derefs and visible failures — a
  from-space block is recycled into Eden and bump-allocated over gradually, so most stale
  derefs still read an intact magic and call the right function.

  `js_headers_keys` / `_values` / `_entries` / `_get_set_cookie` hold `arr` (and
  `entries` also `k_ptr` and `pair`) across the same allocations: the
  `arr = push(arr, …)` idiom handles the array *growing*, not the collector *moving* it.
  `js_form_data_for_each` is a verbatim copy of the `for_each` defect. All six now root
  through `RuntimeHandleScope`, the idiom this file already uses on the `Headers`
  constructor path, and re-read after every call that can collect.

  **Attribution.** `PERRY_GC_PROTECT_FROMSPACE=1` under the *default* GC — no forced
  evacuation, no seeded schedule — faults at `js_headers_for_each + 200` on a retired
  `obj_type=4` (GC_TYPE_CLOSURE), `size=48`, at `user_ptr + 12`. Twelve protect runs on
  the unfixed binary produced 8 faults over 51 copying minors, **every one at that single
  site**; `PERRY_GC_PROTECT_FROMSPACE_HOLDERS=1` reported `(none in 90262 objects — the
  holder is outside the arena: a runtime side table, an FFI structure, or a register/stack
  slot)`. It is the stack slot.

  **Measured**, matched A/B differing only by this patch, identical app dylib: protect
  arm 8 faults / 51 minors → **0 faults / 60 minors**; default GC with no knobs 3 failed
  batches / 300 passes → **0 / 300**; default GC under `PERRY_GC_HEAP_LIMIT=8` 1 failure
  over 6,334 copying minors → **0 over 6,339**; forced+seeded (`SEED=8036`) unchanged at
  30/30 with ~1.04 M objects copied per arm and zero evacuation-verify panics. No second
  holder emerged in 360 protected passes.

  **Why nothing caught it.** The holder is a native Rust frame slot, invisible to all
  three static instruments by construction: `scripts/gc_runtime_root_holders.py` audits
  `static`/`thread_local!` declarations, `scripts/gc_root_dominance_check.py` reads
  emitted LLVM IR from compiled JS, and `scripts/raw_handle_debt.py` counts bare reads out
  of handles that *already exist* and only inside `crates/perry-runtime/src`. A site that
  never rooted anything, in `perry-stdlib`, is outside all of them.
