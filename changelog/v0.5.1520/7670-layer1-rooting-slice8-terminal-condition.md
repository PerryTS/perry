### GC — Layer 1 rooting, slice 8: the raw API becomes unreachable (#7615)

The Layer 1 emitter-migration campaign reaches its **terminal condition**. The
plan stated it as "`expr/temp_root.rs` going `pub(in crate::rooting)` — the raw
accessor unreachable, not merely uncounted".

**As literally spelled, that is not expressible in Rust.** `pub(in path)`
requires `path` to be an ANCESTOR module of the item (E0742), and
`crate::rooting` is not an ancestor of `crate::expr::temp_root`. So the file
moved: the raw API is now `crate::rooting::temp_root`, declared with a
**private** `mod temp_root;` and with `pub(in crate::rooting)` on every
accessor. Either alone would suffice; both are here because the module
declaration is one keyword away from re-widening twenty-five items at once.

A raw call planted in a migrated module now fails to compile — `error[E0603]:
module temp_root is private` — which is the difference between this and a
ledger line. Both belts are sabotage-verified (ten arms: eight textual, two
visibility).

Two items keep `pub(crate)` and are re-exported from `rooting/mod.rs`. Neither
is an accessor and neither can be called in the wrong order: `TempRootPool`
(the compile-time slot bookkeeping `FnCtx` owns) and `expr_is_inert_primitive`
(the "can evaluating this run user code?" predicate `crate::loop_purity`
shares).

**Fourteen entry points were deleted, not narrowed**, because the migration
left them with no caller: `lower_exprs_rooted`, `lower_operand_pair_rooted`,
`any_later_ref_may_trigger_gc`, `RootedOperands::is_rooted`, the whole
`StoreOperandGuard` family (`guard_store_operand`, `guard_store_operand_across`,
`reread_store_operand`, `release_store_operand`), the whole `RootedHandle`
family (`rooted_handle_begin`/`_get`/`_release`) and
`temp_root_scope_begin`/`_end`. CLAUDE.md's kill-policy: the losing mode should
stop compiling.

#### One live bug: `Expr::ArrayMap`

`arr.map(cb)` lowered the receiver, lowered the callback, and only THEN unboxed
the receiver — so `unbox_to_i64` sat **below its own window** and masked a stale
box rather than repairing it (#7280 taxonomy (c), an operand-to-operand
window). The callback's lowering allocates a closure at minimum.

The window is real for receivers with **no slot to rematerialise from**,
verified on emitted IR against a `main` baseline built in a separate target
dir. For a module global the pre-fix IR is:

```llvm
%r1 = load double, ptr @perry_global_m_ts__0     ; receiver
%r2 = call i64 @js_closure_alloc_singleton(...)  ; the window
%r5 = bitcast double %r1 to i64                  ; STALE
%r8 = call i64 @js_array_map(i64 %r6, ...)
```

and the same shape appears for a class-field read and for a closure capture
(`js_closure_get_capture_bits`, a raw `i64` — taxonomy (a), which `root_reload`
structurally cannot repair). For an array-typed **local** there was no window:
codegen's `ptr addrspace(1)` retype pass rematerialises the load from the
local's own root slot at the use site, so the pre-fix code re-read the receiver
by accident. That distinction is recorded in the tests, because the first
version of them used a local receiver and the sabotage arm came back green.

#### Modules migrated

Eight, seven load-bearing on the committed source: `expr/binary.rs`,
`expr/math_simple.rs`, `expr/static_field_meta.rs`, `expr/dyn_extern_i18n.rs`,
`lower_string_method.rs`, `lower_string_concat.rs`, `lower_call/new.rs`, and
`lower_call/new_alloc.rs` (vacuous, listed anyway so an unlisted sibling of a
listed module cannot become the place a raw push goes).

Nine further files mention the raw API and make **no rooting decision**, so
they are deliberately not listed — a ledger line on a module that never had a
decision to make looks substantive and asserts nothing: `expr/mod.rs` (a module
declaration and a field type, both gone with the move), the four `FnCtx`
constructors (`TempRootPool::default()`), `stmt/loops.rs` (one purity
predicate), `loop_purity.rs` (a doc link only) and `root_reload.rs` /
`gc_call_effects.rs` / `runtime_decls/arrays.rs` plus five test files, whose
`js_gc_temp_root_*` occurrences are runtime SYMBOL NAMES.

#### Slice 7's three unverified leads

* **`static_field_meta.rs`'s `caps_arr` — a real accumulator shape with a
  provably empty window.** The `__perry_ctor_caps` array held the only
  reference to everything pushed so far while the next element was lowered
  (#6951's shape) in a bare SSA register. But `captured_args` is built at one
  site (`lower/lower_expr/arm_class.rs`) as
  `ids.iter().map(|id| Expr::LocalGet(*id))`, and `expr_may_trigger_gc` answers
  `false` for every `LocalGet`. It is now a `with_rooted_accumulator` whose
  `protect` is **computed rather than assumed**: today that is `false` and the
  emitted IR is byte for byte unchanged; the day a non-inert expression reaches
  the list it is rooted by construction.
* **`math_simple.rs`'s `ArrayMap` — confirmed live**, above.
* **`dyn_extern_i18n.rs`'s `path_handle` — dismissed, and the lead's premise is
  wrong about the CFG.** It says the raw handle is reused across a compare loop
  "that runs module `__init` bodies". It does not: each `<prefix>__init()` is
  emitted into that iteration's MATCH block, which branches straight to the
  join, so no `__init` dominates any later use of `path_handle`. Along the
  fallthrough chain the only emissions between the handle's production and its
  last use are `js_get_string_pointer_unified` and `js_string_equals` — neither
  re-enters user code nor enumerates an object, which is the standard
  `with_operands_rooted_across_call`'s doc sets for an emitted step (#7198).
  What that module DID have was the namespace-object accumulator (#7280's
  269-member zod case), which is migrated.

#### Two file splits, each its own commit

Both were blockers rather than tidying: the migration ADDS lines (a combinator
owns the body it re-indents) and both files sat against
`scripts/check_file_size.sh`'s 2,000-line cap.

* `lower_call/new.rs` 1,988 → 1,501; `lower_call/new_alloc.rs` 531 (the
  field-count computation and the three-arm instance allocation, verbatim).
* `lower_string_method.rs` 1,957 → 1,368; `lower_string_concat.rs` 646 (the
  boundary the file already had: method dispatch above, `a + b` / `s += x`
  below).

#### API

`RootedGroup::adopt_emitted` gains a `protect` flag — the **window**, not the
strategy; `Reload` and `Reuse` remain unavailable for an emitted value on
principle. `RootedGroup::is_rooted` reports whether a slot exists (never which
one), which is what lets `MapSet` keep its eager unbox, and therefore its exact
register numbering, on the unprotected path.

#### ★ A trap worth recording: a sabotage harness that restores a `.bak`

`cp f f.bak; <patch f>; cargo test; mv f.bak f` leaves `f` with an **older**
mtime than the sabotaged build, so cargo keeps the sabotaged binary and every
subsequent run measures the sabotage. It presented as an intermittent
lowering-test failure (1 run in 10, then 20 in 20) with byte-identical IR
between the green and red runs — which reads exactly like the process-global
sinks #7665 fixed, and is not. `touch` after the restore; and diagnose from the
wrong VALUE, which here said "the producer is one line above the window", i.e.
precisely the pre-fix lowering.
