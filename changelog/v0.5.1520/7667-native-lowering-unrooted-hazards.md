### The reload rule now covers global roots and derived values (native lowering)

`gc-root-dominance-statepoints`' `--max-unrooted` ratchet goes **21 → 7**.

#7663 pointed the root-dominance rule at the **native** root lowering — the one
that ships on every target whose frames the runtime can walk since #7370 — and
reported 21 `unrooted` hazards, enumerated by shape in #7664. Fourteen of them
were shapes `root_reload.rs` (#7280) looked straight through, and for one
reason: its rule is stated over *the load's own register*, and in both shapes
the value at risk lives somewhere else.

**1. The root is a global, not an alloca — 10 hits.** A string literal lowers to
`load double, ptr @<mod>_.str.N.handle`. The handle global **is** a registered
root (`js_gc_register_global_root`, `codegen/string_pool.rs`), so the string is
never swept — and an evacuating cycle **rewrites the global** while a register
loaded beforehand keeps the pre-move address. That is #7240's shape, whose fix
(`OperandProtection::Reload`) covers call *operands* and never reached the ~194
codegen sites that load a handle global directly.

**2. The stale register is DERIVED from the load — 3 of 7 unmasked receivers.**
`this.count++` lowered to a slot load, a `bitcast`, an `and` mask, the property
GET, and then the SET re-using the *pre-GET* mask. The load's only use is the
`bitcast`, which sits **above** the collecting call, so the window was empty and
the function took zero reloads; the value that actually crosses the GET is the
mask. Under the native lowering the same shape reads
`ptrtoint ptr addrspace(1) %s to i64` — LLVM relocates the `addrspace(1)`
pointer and rewrites its uses, but it cannot touch an `i64` copy, so the unmask
is where the value leaves the tracked domain for good. #7280's zod-`clone`
shape and #7240's literal shape, meeting in one function.

**3. `new.target`'s saved previous value — 1 hit.** `lower_call/new.rs` saved
`js_new_target_get()` into a bare SSA register across the **whole constructor
body**. The cell is a registered mutable root
(`scan_current_new_target_root_mut`), so evacuation rewrites it and the restore
publishes a pre-move address back *into* a root the collector scans. The
runtime's own construct paths have always rooted their `prev_new_target`
(`scope.root_nanbox_f64`); generated code did not. This is #7226's `prev_this`
bug for `new.target`, and it is fixed the same way — a
`new_target_save`/`new_target_restore` pair in `crate::rooting`, structurally
`implicit_this_save`/`implicit_this_restore`.

#### The restated rule

> For a value read out of a collector-rewritten location — a shadow slot **or a
> string-handle global** — and any value **derived from it by pure bit ops**,
> every use that a collection point can reach re-materialises the whole
> derivation instead.

A derivation ("recipe") is extended only through instructions that are pure
functions of their operands (`and`/`or`/`xor`, `bitcast`/`ptrtoint`/`inttoptr`/
`trunc`/`zext`/`sext`) **and** whose every register operand is already in the
same single root's recipe. That makes a recipe self-contained — a load plus bit
ops on constants — so it materialises at any point in the function with no
dominance question, and re-executing it is by construction the same function of
the same root evaluated against the address the collector wrote back. A phi, a
call, or an operand from a second root is not extended through.

**The window is anchored at the root load, not at the derived value**, and that
distinction cost a regression during development. A derived value whose
*definition* sits below a store to the root is still governed by the window
since the root load: `main`'s class-object read has the scope-end shadow-slot
clear landing between the load and the mask, and a walk anchored at the mask
never sees the clear, re-materialised `load %slot` at the use, and read a slot
the program had just nulled — turning `(makeAnon(77) as any).v` into
`undefined`. Caught by an A/B against the branch point on
`test_gap_class_expr_identity`, **not** by the dominance checker, which cannot
see a value-correctness bug; pinned now by
`a_derivation_defined_below_a_store_to_its_root_is_not_re_materialised`.

Re-reading a *handle global* is sound for the reason `operand_is_reloadable`
gives: the only writer in generated code is `__perry_init_strings_*`, so a
re-read cannot observe a later assignment. That function is excluded by the
analysis rather than by the argument — the same store side-condition that
protects a reassigned slot covers a stored-to global.

#### Measured

`test_gap_closures.ts`'s `Counter__increment`, native corpus. **Before**, all
three statepoints carried an empty live set: the receiver was in no bundle at
all, so nothing marked or rewrote it, and the SET read the mask computed before
the GET. **After**, every statepoint carries a `"gc-live"` bundle and emits a
`gc.relocate`, and the SET reads a mask re-derived from the relocated pointer
plus a fresh load of the handle global. The re-derivation is what puts the
receiver in the bundle: it extends the tracked pointer's live range past the
safepoints, so `rewrite-statepoints-for-gc` must report and relocate it.

#### What remains

`--max-unrooted 7`, and each remainder is its own slice rather than a widening
of this fix: **4 unmasked** are phi-mediated (no instruction can be inserted
above a phi — the reload has to go in the predecessor, on the edge); **2
global** are `@perry_global_*`, module-level variables the program assigns, so a
re-read can observe a later assignment instead of the value the call was given
(`operand_needs_root`) — that population needs rooting, not reloading; **1
capture** is a `js_closure_get_capture_bits` read held across
`js_number_coerce`. #7664 stays open as the budget's referent.

The `global` exclusion is pinned by `a_module_global_is_not_a_reload_source`, so
widening `is_string_handle_global` to swallow it is a test failure rather than a
silent decision. Six more unit tests cover the two new shapes, the
`__perry_init_strings_*` store side-condition, the store-below-the-load
regression above, the no-collection-point control (the derivation closure must
not turn every mask in the program into three extra instructions), and the
refusal to extend a derivation through a call.

Also corrected: #7664's shape-1 heading says nine hits; its own list has ten,
and the checker reports ten. The census of the 21 is 10 `strhandle` / 7
`unmasked` / 2 `global` / 1 `rootread` / 1 `capture`.
