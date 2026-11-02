### Fixed

- **`codegen`: every argument of a cross-module direct call is now protected
  across the lowering of the arguments that follow it** (#7154). An argument
  list is evaluated left to right and each finished value sits in a bare SSA
  register while the later ones are lowered. `lower_call/extern_func.rs`'s
  generic `perry_fn_<src>__<name>` path lowered the whole list in a plain
  `for a in args` loop with no protection at all, so
  `f(A, B, {…}, Schema.array(), body => …)` leaves `A` and `B` naming
  pre-collection addresses the moment an evacuating minor lands in argument 3,
  4 or 5 — and it does: argument 3 allocates an object, argument 4 runs user
  code with its own loop back-edge polls, argument 5 allocates a closure.

  This is the residual #7227 measured and named rather than fixed. In the
  `sfw-registry` reproducer it is `src/lib/api/alerts.ts`'s module init calling
  `defineApiCall(url, method, {…}, SocketAlert.array(), body => …)` across the
  module boundary. The fault surfaces one frame down, inside `js_regexp_test`
  called from `defineApiCall + 428`, because the stale `url` argument is what
  `/\[[a-zA-Z]+\]/.test(url)` hands to it:

  ```asm
  ldp  d9, d10, [x24, #0x18]   ; url + method, loaded from their
                               ; __perry_init_strings_* handle globals
  bl   js_object_alloc_class_inline_keys   ; argument 3  -- ALLOCATES
  bl   perry_fn_…zod…                      ; argument 4  -- USER CODE
  bl   js_closure_alloc_singleton          ; argument 5  -- ALLOCATES
  fmov d0, d9                              ; STALE
  fmov d1, d10                             ; STALE
  bl   perry_fn_src_lib_api_shared_ts__defineApiCall
  ```

  The diagnosis is a measurement rather than a reading of the disassembly. At
  the fault the `__perry_init_strings_*` handle global held `0x…76561xxx` — the
  post-move address evacuation wrote back — while `defineApiCall`'s shadow slot
  (and the register it was stored from) held `0x…74eb5d58`, inside the
  quarantined from-space block the reporter named. Root rewritten, register not.

  A string-literal argument therefore takes `OperandProtection::Reload`: its
  handle global is a registered root, so the string is never *swept*, and the
  fix is to emit the load again below the collection point — no runtime call at
  all. Non-literal arguments take a real temp root, as
  `temp_root::lower_exprs_rooted` already does for the `new C(…)` argument list
  (#6969). Each argument is gated on `any_later_ref_may_trigger_gc`, so an
  argument list with nothing allocating after it emits exactly the IR it did
  before.

  **Why `scripts/gc_root_dominance_check.py` reports nothing here**, which is
  the part worth carrying forward: the checker classifies a heap-value SOURCE
  as an `ALLOC_RE` call or a shadow-slot load. A load of a string-literal
  handle global is neither, so the register it defines is never tracked as a
  heap value and no stale use can be attributed to it. That is a third shape of
  the same blind spot `js_implicit_this_set` (#7226) and `js_regexp_new` (#7227)
  each cost a round for — and unlike those two it is not fixed by adding a name
  to a pattern, because the source is a `load`, not a `call`.

### Added

- `test-files/test_gap_gc_call_argument_rooting.ts` (+ its cross-module fixture
  `test-files/fixtures/gc_call_arg_rooting_pkg/callee.ts`). It has to be two
  files: the defect is in the cross-module lowering, and a same-file callee
  compiles through `func_ref.rs` instead. Both protections are exercised — one
  call passes two string literals (`Reload`), the other passes a local holding
  a freshly-allocated string plus a literal (`Root` + `Reload`).
