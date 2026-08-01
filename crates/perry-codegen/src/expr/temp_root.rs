//! Precise rooting for expression temporaries (#6951).
//!
//! The shadow stack roots named locals. It has no slot for the values that
//! only exist between two instructions, and an LLVM SSA register is not a GC
//! root — so an accumulator array, or an already-evaluated operand waiting for
//! its sibling, dies if the sibling's evaluation collects. Conservative native
//! stack scanning hid that (see `perry-runtime/src/gc/roots/temp_roots.rs`);
//! with `PERRY_CONSERVATIVE_STACK_SCAN=off` it is a live use-after-free.
//!
//! The emission contract, in the order it must appear:
//!
//! ```text
//! %idx = call i32 @js_gc_temp_root_push(i64 <bits>)   ; before the collection point
//! ...                                                  ; anything that may collect
//! %v   = call i64 @js_gc_temp_root_get(i32 %idx)       ; ALWAYS re-read
//!        call void @js_gc_temp_root_truncate(i32 %idx) ; after the last use
//! ```
//!
//! Re-reading is mandatory, not defensive: the slot is a *mutable* root, so an
//! evacuating cycle rewrites it and the register pushed beforehand is stale.
//! That is also why this is preferable to widening conservative scanning —
//! conservative roots have to pin, precise ones can move.
//!
//! # The invariant (#7114)
//!
//! **No operand register may outlive a collection point. After the last thing
//! that can collect, every operand is either re-read from a root the collector
//! rewrote or re-derived from immutable storage — never reused.**
//!
//! A root buys three things, and they are not the same thing:
//!
//!  1. **liveness** — the object is marked instead of swept;
//!  2. **a rewritten location** — a slot evacuation updates to the new address;
//!  3. **the value the consuming call observes** — which is (2) only if the
//!     code that resumes after the safepoint *reads that location again*.
//!
//! #7114 is what dropping (3) on its own looks like. A string literal is a load
//! from a `__perry_init_strings_*` handle global that
//! `js_gc_register_global_root` registered, so it has (1) and (2) for free —
//! and `console.log("acc:" + run(1e7))` still printed an empty line, because
//! the register loaded *before* `run` held the pre-move address. Exit code 0,
//! no diagnostic, no crash.
//!
//! [`operand_protection`] is the single place that decides which of the three
//! strategies an operand needs. Both helper families in this module route
//! through it; before #7114 they answered it separately and disagreed.

use perry_hir::types::Type as HirType;
use perry_hir::Expr;

use crate::types::{DOUBLE, I32, I64};

use super::FnCtx;

/// Push `value_i64` (a bare heap pointer or NaN-boxed bits) and return the
/// slot-index register.
pub(crate) fn temp_root_push_i64(ctx: &mut FnCtx<'_>, value_i64: &str) -> String {
    ctx.block()
        .call(I32, "js_gc_temp_root_push", &[(I64, value_i64)])
}

/// Push a NaN-boxed `double` temporary and return the slot-index register.
pub(crate) fn temp_root_push_double(ctx: &mut FnCtx<'_>, value: &str) -> String {
    let bits = ctx.block().bitcast_double_to_i64(value);
    temp_root_push_i64(ctx, &bits)
}

/// Re-read slot `idx` as a raw `i64`.
pub(crate) fn temp_root_get_i64(ctx: &mut FnCtx<'_>, idx: &str) -> String {
    ctx.block().call(I64, "js_gc_temp_root_get", &[(I32, idx)])
}

/// Re-read slot `idx` as a NaN-boxed `double`.
pub(crate) fn temp_root_get_double(ctx: &mut FnCtx<'_>, idx: &str) -> String {
    let bits = temp_root_get_i64(ctx, idx);
    ctx.block().bitcast_i64_to_double(&bits)
}

/// Overwrite slot `idx` with a new raw `i64`.
///
/// For producers that hand back a *different* address each round — the
/// `concat` accumulator (#6971), where every `js_string_concat` yields a new
/// string and the old one stops being the value that must stay alive.
pub(crate) fn temp_root_set_i64(ctx: &mut FnCtx<'_>, idx: &str, value_i64: &str) {
    ctx.block()
        .call_void("js_gc_temp_root_set", &[(I32, idx), (I64, value_i64)]);
}

/// Drop slot `idx` and everything pushed above it.
pub(crate) fn temp_root_truncate(ctx: &mut FnCtx<'_>, idx: &str) {
    ctx.block()
        .call_void("js_gc_temp_root_truncate", &[(I32, idx)]);
}

/// Push `value` onto the array held in temp-root slot `idx`, writing the
/// possibly-reallocated array pointer back into the slot.
pub(crate) fn temp_rooted_array_push(ctx: &mut FnCtx<'_>, idx: &str, value: &str) {
    ctx.block().call_void(
        "js_array_push_f64_temp_rooted",
        &[(I32, idx), (DOUBLE, value)],
    );
}

/// Allocate an argument-accumulator array and root it, returning the
/// temp-root slot index.
///
/// This is the shape behind every variadic / spread / rest argument list:
/// `js_array_alloc(n)`, then one `js_array_push_f64` per argument, with the
/// accumulator threaded through in an SSA register. That register held the
/// only reference to everything pushed so far — including argument 0 — across
/// the evaluation of argument 1, which is exactly the #6951 repro
/// (`console.log("label", allocatingCall())`).
///
/// Pair with [`temp_rooted_array_push`] per argument, then
/// [`rooted_array_read`] and [`temp_root_truncate`] — in that order, so the
/// array stays rooted across the call that consumes it.
pub(crate) fn rooted_array_begin(ctx: &mut FnCtx<'_>, cap: &str) -> String {
    let arr = ctx.block().call(I64, "js_array_alloc", &[(I32, cap)]);
    temp_root_push_i64(ctx, &arr)
}

/// Read the accumulator back out of its temp-root slot. Does NOT truncate:
/// callers truncate after the consuming call, so the array is still rooted
/// while the consumer runs (formatting an argument list allocates).
pub(crate) fn rooted_array_read(ctx: &mut FnCtx<'_>, idx: &str) -> String {
    temp_root_get_i64(ctx, idx)
}

/// Can lowering `expr` reach a collection point?
///
/// Deliberately one-sided: `false` must mean "provably allocates nothing", and
/// everything unrecognized answers `true`. A wrong `false` is a
/// use-after-free; a wrong `true` costs two runtime calls on a cold path.
pub(crate) fn expr_may_trigger_gc(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
    match expr {
        // Immediates and plain slot reads. `LocalGet` reads an alloca,
        // `GlobalGet` a module global — neither allocates. (Reading an
        // object-typed local is still just a load; it is the *operators* below
        // that can coerce it and run user code.)
        Expr::Undefined
        | Expr::Null
        | Expr::Bool(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::LocalGet(_)
        | Expr::GlobalGet(_) => false,
        // A string literal is materialized once into a module-global handle by
        // `__perry_init_strings_*` and registered as a GC root there; the use
        // site is a load.
        Expr::String(_) => false,
        // Coercing operators. `-o`, `o < x`, `o == x`, `o * 2` all run
        // ToPrimitive / ToNumber on their operands, and a user-defined
        // `Symbol.toPrimitive` / `valueOf` / `toString` is arbitrary JS: it
        // allocates, and it collects. Recursing into the operands is NOT
        // enough — `a < b` over two plain `LocalGet`s recurses to `false`
        // while the comparison itself can call into user code. So these are
        // GC-capable unless every operand is a proven inert primitive.
        Expr::Unary { .. } | Expr::Compare { .. } | Expr::Binary { .. } => {
            !expr_is_inert_primitive(ctx, expr)
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            expr_may_trigger_gc(ctx, condition)
                || expr_may_trigger_gc(ctx, then_expr)
                || expr_may_trigger_gc(ctx, else_expr)
        }
        Expr::Sequence(exprs) => exprs.iter().any(|e| expr_may_trigger_gc(ctx, e)),
        _ => true,
    }
}

/// Is `expr` a value whose evaluation *and coercion* provably cannot run user
/// code or allocate?
///
/// This is the inner half of [`expr_may_trigger_gc`]'s one-sidedness: only
/// literals and locals the type analysis proved to be numbers / booleans /
/// null / undefined qualify, plus operator trees built entirely out of those.
/// A local carrying an object — or one with a reserved shadow slot, which
/// means it is pointer-possible regardless of its refined type — is not inert,
/// because `ToPrimitive` on it dispatches to whatever the object defines.
///
/// Also the whitelist behind the loop back-edge poll
/// (`crate::loop_purity::loop_may_allocate`): "can evaluating this run user
/// code or allocate?" is the same question there, so the answer comes from
/// here rather than from a second copy that drifts.
pub(crate) fn expr_is_inert_primitive(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
    match expr {
        Expr::Undefined | Expr::Null | Expr::Bool(_) | Expr::Number(_) | Expr::Integer(_) => true,
        // A heap value, but ToPrimitive on a string is the identity: no user
        // code, no allocation. (`+` is restricted below, since concatenation
        // does allocate.)
        Expr::String(_) => true,
        Expr::LocalGet(id) => local_is_inert_primitive(ctx, *id),
        // `++` / `--` on an inert local runs ToNumeric over a value that is
        // already a non-pointer primitive, then a numeric add and a store: no
        // user code, no allocation. (`x++` on a BigInt DOES allocate a fresh
        // BigInt — but `HirType::BigInt` is not in the inert set, and a
        // BigInt-typed local is pointer-typed, so it also has a shadow slot.)
        //
        // [`expr_may_trigger_gc`] deliberately does not route `Update` here and
        // keeps it on the conservative catch-all: #6951's question is about
        // operand lists, where an embedded `Update` is vanishingly rare. The
        // loop-poll caller is the one that needs it (`for (…; …; i++)`).
        Expr::Update { id, .. } => local_is_inert_primitive(ctx, *id),
        Expr::Unary { operand, .. } => expr_is_inert_primitive(ctx, operand),
        Expr::Compare { left, right, .. } => {
            expr_is_inert_primitive(ctx, left) && expr_is_inert_primitive(ctx, right)
        }
        Expr::Binary { op, left, right } => {
            expr_is_inert_primitive(ctx, left)
                && expr_is_inert_primitive(ctx, right)
                // `+` is the one operator whose RESULT can be a fresh heap
                // value: with a string operand it concatenates, and that
                // allocates. Inert operands alone do not rule that out —
                // `Expr::String` is inert — so `Add` additionally demands that
                // neither operand can BE a heap reference, which is exactly
                // `expr_is_known_non_pointer_shadow_value`. Two operands that
                // provably hold no pointer cannot be strings, so the `+` is a
                // numeric add and allocates nothing.
                && (!matches!(op, perry_hir::BinaryOp::Add)
                    || (super::expr_is_known_non_pointer_shadow_value(ctx, left)
                        && super::expr_is_known_non_pointer_shadow_value(ctx, right)))
        }
        _ => false,
    }
}

/// [`expr_is_inert_primitive`] for a bare local id — the shared half of its
/// `LocalGet` and `Update` arms.
///
/// Three independent facts have to line up, and none alone is enough:
///
///  * the refined type is a non-pointer primitive, so `ToPrimitive` on it is
///    the identity and dispatches to nothing;
///  * no shadow slot is reserved for the local — `collect_pointer_typed_locals`'
///    verdict that the local is not pointer-typed. A reserved slot means
///    pointer-possible regardless of what the refined type says; and
///  * the binding is not a module-level global. `local_types` and the
///    shadow-slot map are both computed per function, from that function's body
///    alone, so a module global that a *different* function assigns an object
///    to still looks like a number here. Those per-function facts are sound for
///    a genuine local and not for a global, so a global is never inert.
///
/// What this does NOT defend against is a *lying annotation*: `let n: number`
/// that is handed an object anyway. Nothing here catches that — but nothing
/// else in the compiler does either, and it is not this predicate's assumption
/// to make good on. `collect_pointer_typed_locals` reserves root slots from the
/// same declared type, so such a local has no shadow slot and the precise scan
/// cannot see the object at all; the value is already unrooted long before any
/// coercion of it reaches a poll decision. Honesty of scalar annotations is a
/// standing invariant of the precise-root design, inherited here rather than
/// introduced.
pub(crate) fn local_is_inert_primitive(ctx: &FnCtx<'_>, id: u32) -> bool {
    !ctx.shadow_slot_map.contains_key(&id)
        && !ctx.module_globals.contains_key(&id)
        && matches!(
            ctx.local_types.get(&id),
            Some(
                HirType::Number
                    | HirType::Int32
                    | HirType::Boolean
                    | HirType::Null
                    | HirType::Void
                    | HirType::Never
            )
        )
}

/// Does any expression after index `i` reach a collection point?
///
/// This is the gate for protecting value `i`: a value that nothing allocating
/// follows cannot be collected before it is consumed, so the rooting calls
/// would be pure overhead. `i < n`, `x * 2` on proven-numeric locals,
/// `f(x, y)` on plain locals and `[1, 2, 3]` therefore emit exactly the IR
/// they emitted before #6951.
fn any_later_ref_may_trigger_gc(ctx: &FnCtx<'_>, exprs: &[&Expr], i: usize) -> bool {
    exprs
        .iter()
        .skip(i + 1)
        .any(|e| expr_may_trigger_gc(ctx, e))
}

/// Lower `exprs` left to right, keeping each already-evaluated value precisely
/// rooted across the evaluation of the ones that follow (#6951).
///
/// Returns the lowered values — **re-read from their roots, or re-derived from
/// their immutable storage** ([`OperandProtection`]), so they are valid after
/// an evacuating cycle — and the guard index the caller must pass to
/// [`temp_root_release`] once the consuming call has run. `None` means nothing
/// needed a temp-root slot; it does NOT mean nothing was re-read, because the
/// [`OperandProtection::Reload`] half emits no runtime call at all.
pub(crate) fn lower_exprs_rooted(
    ctx: &mut FnCtx<'_>,
    exprs: &[&Expr],
) -> anyhow::Result<(Vec<String>, Option<String>)> {
    let mut values = Vec::with_capacity(exprs.len());
    let mut slots: Vec<Option<String>> = Vec::with_capacity(exprs.len());
    let mut guard: Option<String> = None;
    let mut reload: Vec<bool> = Vec::with_capacity(exprs.len());
    for (i, expr) in exprs.iter().enumerate() {
        let value = super::lower_expr(ctx, expr)?;
        // `any_later_ref_may_trigger_gc` is the *window*: can anything between
        // this operand and the consuming call collect? [`operand_protection`]
        // turns that window into the one strategy this operand needs.
        let collects = any_later_ref_may_trigger_gc(ctx, exprs, i);
        match operand_protection(ctx, expr, collects) {
            OperandProtection::Root => {
                let idx = temp_root_push_double(ctx, &value);
                // The FIRST slot pushed is the guard: truncating it drops every
                // slot above it too, so one call releases the whole group.
                if guard.is_none() {
                    guard = Some(idx.clone());
                }
                slots.push(Some(idx));
                reload.push(false);
            }
            OperandProtection::Reload => {
                slots.push(None);
                reload.push(true);
            }
            OperandProtection::Reuse => {
                slots.push(None);
                reload.push(false);
            }
        }
        values.push(value);
    }
    for (i, value) in values.iter_mut().enumerate() {
        if let Some(idx) = slots[i].clone() {
            *value = temp_root_get_double(ctx, &idx);
        } else if reload[i] {
            // #7114: no runtime call — just the load that was already emitted,
            // emitted again below the collection point so it observes the
            // address evacuation wrote back into the handle global.
            *value = super::lower_expr(ctx, exprs[i])?;
        }
    }
    Ok((values, guard))
}

/// Lower a `left`/`right` operand pair with the same contract as
/// [`lower_exprs_rooted`].
pub(crate) fn lower_operand_pair_rooted(
    ctx: &mut FnCtx<'_>,
    left: &Expr,
    right: &Expr,
) -> anyhow::Result<(String, String, Option<String>)> {
    let (mut values, guard) = lower_exprs_rooted(ctx, &[left, right])?;
    let right_value = values.pop().expect("pair lowering yields two values");
    let left_value = values.pop().expect("pair lowering yields two values");
    Ok((left_value, right_value, guard))
}

/// Already-lowered operand values kept alive across work whose shape the
/// caller controls — a later operand whose *representation* is chosen per
/// branch (`Expr::MapSet`, #6970) or an allocation that happens after the whole
/// list is lowered (`new C(a, b)`, #6969).
///
/// [`lower_exprs_rooted`] cannot serve those: it decides what to protect from
/// the expressions it is handed and re-reads immediately, whereas these sites
/// need the re-read to happen *after* a step the helper never sees. So the
/// caller supplies the protection decision and picks the re-read point.
///
/// When `protect` is false this emits nothing at all and [`RootedOperands::reread`]
/// hands the original registers straight back, so unprotected sites keep their
/// pre-#6951 IR byte for byte.
pub(crate) struct RootedOperands {
    /// Slot index per operand, or `None` when the operand was not rooted.
    slots: Vec<Option<String>>,
    /// The registers as originally lowered — the answer when nothing is rooted
    /// and the operand cannot be re-loaded.
    values: Vec<String>,
    /// Whether an unrooted operand must be re-loaded from its own storage
    /// rather than reused from its register. See [`RootedOperands::reread`].
    reloadable: Vec<bool>,
    /// First slot pushed; truncating it drops the whole group.
    guard: Option<String>,
}

/// Does this operand read a location the collector *rewrites in place*, so that
/// re-lowering it after a collection yields the corrected address?
///
/// A local with a shadow slot, a module global and a string-literal handle are
/// all registered roots — they are marked, and on an evacuating cycle they are
/// **rewritten**. That keeps the object alive and the *storage* correct, but it
/// says nothing about a register loaded from that storage beforehand: after
/// relocation the register holds the pre-move address. Re-loading is the fix,
/// and it is free — no temp-root traffic, just the load that would have been
/// emitted anyway.
///
/// This is the same staleness #6981 reports one layer in (a raw typed-array
/// pointer passed under the specialized ABI).
///
/// # Why the sibling literal forms are deliberately absent
///
/// `Expr::WtfString` (a lone-surrogate literal) and `Expr::I18nString` lower to
/// exactly the same thing as `Expr::String` — one load of a
/// `__perry_init_strings_*` handle global, registered with
/// `js_gc_register_global_root` by the same loop, `is_wtf8` or not
/// (`codegen/string_pool.rs`). They would be sound here. They are not listed
/// because [`operand_needs_root`] does not suppress them either, so they take a
/// real temp root — and **`Root` is strictly stronger than `Reload`**: it
/// supplies liveness, a rewritten location and the call-time value on its own,
/// where `Reload` borrows the first from the handle global.
///
/// The failure mode to guard against is not the asymmetry, it is *half*-closing
/// it: adding a literal form to [`operand_needs_root`]'s suppression list
/// without adding it here leaves it on `Reuse`, which is #7114 for that form.
/// `wtf8_literal_operand_is_rooted_not_merely_reused` in
/// `tests/temp_root_operand_temporaries.rs` pins the current answer so that edit
/// goes red instead of shipping another silent wrong answer.
pub(crate) fn operand_is_reloadable(expr: &Expr) -> bool {
    // ONLY provably immutable sources. A string literal always re-lowers to a
    // load of the same `__perry_init_strings_*` handle, so re-reading it can
    // never observe a different value.
    //
    // A local or a module global must NOT be here, even though both are
    // registered roots whose storage evacuation rewrites. Re-lowering one reads
    // its value *now*, and "now" is after the later arguments, the field
    // initializers and possibly an inlined constructor body have run — any of
    // which may have reassigned it. `new C(g, bump())` where `bump()` sets
    // `g` must capture `g`'s value at call time; re-lowering produced the
    // post-`bump()` value, a miscompile rather than a rooting bug. Those
    // operands get a real temp root instead: the slot preserves the call-time
    // value AND the collector rewrites it on evacuation.
    matches!(expr, Expr::String(_))
}

/// Build the protection **incrementally**, one operand at a time, so each is
/// rooted before the next one is lowered.
///
/// That ordering is the whole point. Lowering every operand first and rooting
/// the finished list afterwards is not merely late, it is *worse than doing
/// nothing*: by then an earlier operand may already have been swept, and the
/// push publishes a dangling pointer into a slot the collector scans. That is
/// what turned #6969 from a silent wrong answer into a SIGSEGV, and it is why
/// `m.set(k, v)` roots `map` before `key` is lowered rather than after.
///
/// See [`RootedOperands::push`] for the per-operand contract.
pub(crate) fn root_operands_begin(capacity: usize) -> RootedOperands {
    RootedOperands {
        slots: Vec::with_capacity(capacity),
        values: Vec::with_capacity(capacity),
        reloadable: Vec::with_capacity(capacity),
        guard: None,
    }
}

impl RootedOperands {
    /// Record one already-lowered operand.
    ///
    /// `collects` says "something between this operand and the consuming call
    /// can reach a collection point" — the caller supplies it because the
    /// hazard is not visible in an expression list: for `m.set(k, v)` the
    /// receiver's window covers both `key`'s lowering and `value`'s, while the
    /// key's covers only `value`'s.
    ///
    /// From that flag two decisions follow, and an operand needs exactly one:
    ///
    /// - [`operand_needs_root`] → push a temp-root slot, because nothing else
    ///   keeps this value alive;
    /// - otherwise [`operand_is_reloadable`] → emit no runtime call, but
    ///   re-load the value at the re-read point, because its storage is a
    ///   registered root that evacuation *rewrites* while the cached register
    ///   keeps the old address.
    ///
    /// When `collects` is false neither applies: nothing can be swept and
    /// nothing can move, so the register is reused and the IR is unchanged.
    pub(crate) fn push(
        &mut self,
        ctx: &mut FnCtx<'_>,
        operand: &Expr,
        value: &str,
        collects: bool,
    ) {
        let protection = operand_protection(ctx, operand, collects);
        if protection == OperandProtection::Root {
            let idx = temp_root_push_double(ctx, value);
            // The FIRST slot pushed is the guard: truncating it drops every
            // slot above it too, so one call releases the whole group.
            if self.guard.is_none() {
                self.guard = Some(idx.clone());
            }
            self.slots.push(Some(idx));
        } else {
            self.slots.push(None);
        }
        self.reloadable
            .push(protection == OperandProtection::Reload);
        self.values.push(value.to_string());
    }

    /// Re-read every operand after the collection point.
    ///
    /// Three cases, and the third is the subtle one:
    ///
    /// - **rooted** → read the slot. Mandatory, not defensive: the slot is a
    ///   *mutable* root, so an evacuating cycle rewrites it and the register
    ///   pushed beforehand is stale.
    /// - **unrooted but re-loadable** → re-lower it. A local/global/literal is
    ///   already a registered root, so it was never at risk of being *swept* —
    ///   but an evacuating cycle rewrote its storage, so the register loaded
    ///   before the collection points at where the object *used to be*. Emitting
    ///   the load again is correct and costs no runtime call.
    /// - **unrooted and not re-loadable** → keep the register. This is only
    ///   reached for values `expr_is_known_non_pointer_shadow_value` proved are
    ///   not heap references, which relocation cannot invalidate.
    pub(crate) fn reread(
        &self,
        ctx: &mut FnCtx<'_>,
        operands: &[&Expr],
    ) -> anyhow::Result<Vec<String>> {
        let mut out = Vec::with_capacity(self.values.len());
        for (i, original) in self.values.iter().enumerate() {
            let value = match &self.slots[i] {
                Some(idx) => {
                    let idx = idx.clone();
                    temp_root_get_double(ctx, &idx)
                }
                None if self.reloadable[i] => super::lower_expr(ctx, operands[i])?,
                None => original.clone(),
            };
            out.push(value);
        }
        Ok(out)
    }

    /// True when this group actually pushed slots — the signal a caller uses to
    /// keep an eager unbox (and therefore its exact register numbering) on the
    /// unprotected path.
    pub(crate) fn is_rooted(&self) -> bool {
        self.guard.is_some()
    }

    /// Drop the group. Call it *after* the consuming call: the consumer
    /// allocates while reading these values.
    pub(crate) fn release(self, ctx: &mut FnCtx<'_>) {
        temp_root_release(ctx, self.guard);
    }
}

/// Release a guard returned by [`lower_exprs_rooted`]. Call it *after* the
/// consuming call, not before: the consumer allocates while reading these
/// values.
pub(crate) fn temp_root_release(ctx: &mut FnCtx<'_>, guard: Option<String>) {
    if let Some(idx) = guard {
        temp_root_truncate(ctx, &idx);
    }
}

/// The receiver of a property/element STORE, kept valid across the evaluation
/// of the value being stored (#7154).
///
/// `o.k = f()` and `o[k] = f()` evaluate the reference first and the value
/// second — spec order, and codegen follows it. That leaves the receiver in an
/// SSA register while `f()` runs, and `f()` allocates. A back-edge poll inside
/// it drives an evacuating minor which relocates the receiver; the *slot* the
/// register was loaded from is a root and gets rewritten, but the register does
/// not, so the store lands in abandoned from-space memory and the field never
/// appears on the object the program keeps.
///
/// This is the store-side instance of the [module invariant](self): property
/// (2) — a rewritten location — is worthless without property (3), reading that
/// location again below the collection point. It is #7114 with a receiver
/// instead of a string literal.
///
/// A temp root (not a re-load) is the required strategy: re-lowering `object`
/// would observe an assignment made by `f()` itself, which is a miscompile
/// rather than a rooting fix — see [`operand_is_reloadable`].
pub(crate) struct ReceiverGuard {
    slot: Option<String>,
}

/// Root `recv` (the lowered `object`) if evaluating `value` can collect.
/// Emits nothing otherwise, so stores with an inert RHS keep their old IR.
pub(crate) fn guard_store_receiver(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    recv: &str,
    value: &Expr,
) -> ReceiverGuard {
    let collects = expr_may_trigger_gc(ctx, value);
    let slot = match operand_protection(ctx, object, collects) {
        OperandProtection::Root => Some(temp_root_push_double(ctx, recv)),
        // `Reload`/`Reuse` both mean the register survives: a string literal
        // cannot be a store receiver, and a proven non-pointer is not movable.
        OperandProtection::Reload | OperandProtection::Reuse => None,
    };
    ReceiverGuard { slot }
}

/// Re-read the receiver below the value's evaluation. Returns `recv` unchanged
/// when nothing was rooted.
pub(crate) fn reread_store_receiver(
    ctx: &mut FnCtx<'_>,
    guard: &ReceiverGuard,
    recv: &str,
) -> String {
    match &guard.slot {
        Some(idx) => {
            let idx = idx.clone();
            temp_root_get_double(ctx, &idx)
        }
        None => recv.to_string(),
    }
}

/// Drop the guard. Call it *after* the store, not before: the store helper
/// allocates (key interning, field-array growth, shape transition).
pub(crate) fn release_store_receiver(ctx: &mut FnCtx<'_>, guard: ReceiverGuard) {
    if let Some(idx) = guard.slot {
        temp_root_truncate(ctx, &idx);
    }
}

/// A freshly allocated container handle (object, array, …) that generated code
/// keeps writing into while it lowers the initializer expressions.
///
/// The handle is a raw `i64` in an SSA register, and every initializer that
/// allocates is a chance for the half-built container to be swept out from
/// under it — the object-literal form of the #6951 accumulator bug. Re-read
/// the handle through [`rooted_handle_get`] before every use.
pub(crate) struct RootedHandle {
    slot: Option<String>,
    value: String,
}

/// Root `handle` when `protect` says an upcoming initializer can collect.
/// `protect == false` emits nothing and [`rooted_handle_get`] hands the
/// original register straight back, so unprotected sites keep their old IR.
pub(crate) fn rooted_handle_begin(
    ctx: &mut FnCtx<'_>,
    handle_i64: &str,
    protect: bool,
) -> RootedHandle {
    let slot = protect.then(|| temp_root_push_i64(ctx, handle_i64));
    RootedHandle {
        slot,
        value: handle_i64.to_string(),
    }
}

pub(crate) fn rooted_handle_get(ctx: &mut FnCtx<'_>, handle: &RootedHandle) -> String {
    match &handle.slot {
        Some(idx) => {
            let idx = idx.clone();
            temp_root_get_i64(ctx, &idx)
        }
        None => handle.value.clone(),
    }
}

pub(crate) fn rooted_handle_release(ctx: &mut FnCtx<'_>, handle: RootedHandle) {
    if let Some(idx) = handle.slot {
        temp_root_truncate(ctx, &idx);
    }
}

/// Do any of an object literal's / call's initializer expressions collect?
pub(crate) fn any_may_trigger_gc<'a>(
    ctx: &FnCtx<'_>,
    exprs: impl IntoIterator<Item = &'a Expr>,
) -> bool {
    exprs.into_iter().any(|e| expr_may_trigger_gc(ctx, e))
}

/// Would `expr`'s lowered value need a temp root, assuming everything after it
/// reaches a collection point?
///
/// A temp root buys two distinct things, and the suppressions here only give
/// up the first:
///
/// 1. **liveness** — the object is marked instead of swept;
/// 2. **a re-readable location** — a slot the collector rewrites, so the value
///    can be recovered after relocation.
///
/// Suppressed operands already have (1) from somewhere else, and get (2) from
/// [`operand_is_reloadable`] instead, which re-emits the load rather than
/// reusing the pre-collection register. Both halves are required: dropping the
/// second is exactly the staleness #6981 reports one layer in.
///
/// - provably not a heap reference — a slot for it is pure TLS traffic, and
///   relocation cannot invalidate it either;
/// - a string literal — a load from a module global `__perry_init_strings_*`
///   registered with `js_gc_register_global_root`;
/// - a module-global read — `@perry_global_*` are registered GC roots
///   (marked *and* rewritten on evacuation);
/// - a local that **has a reserved shadow slot**, which binds the collector to
///   the local's own alloca — so evacuation rewrites the alloca in place.
///
/// Together these are why `new C(a, b)` on ordinary locals emits no runtime
/// rooting calls even though the instance allocation that follows always
/// collects.
///
/// The shadow-slot check is load-bearing, not decoration. Suppressing every
/// `LocalGet` looks equivalent and is not: a local can be pointer-valued and
/// have *no* shadow slot, in which case it lives in a bare alloca that the root
/// walk never visits (that is the #6968 defect) — so it has neither (1) nor
/// (2). `m.set(fresh(), churn())` regressed straight back to an abort when this
/// was written as a blanket `LocalGet` suppression; the Map receiver was
/// exactly such a local.
pub(crate) fn operand_needs_root(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
    if super::expr_is_known_non_pointer_shadow_value(ctx, expr) {
        return false;
    }
    // Only a string literal is suppressed: it is a registered root AND
    // immutable, so `operand_is_reloadable` can recover it with a plain load.
    //
    // Locals and module globals are deliberately NOT suppressed. Being a
    // registered root buys liveness, but the value has to survive relocation
    // *and* stay the value the call actually observed — and a re-load gives up
    // the second. Rooting is the only thing that gives both, so they pay for a
    // slot.
    !matches!(expr, Expr::String(_))
}

/// What an already-lowered operand needs so that the consuming call observes a
/// valid, current address across a following collection point.
///
/// See the module header for the three properties a root buys. Each variant is
/// the cheapest strategy that supplies all three for its class of operand:
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum OperandProtection {
    /// Push a temp-root slot and re-read it. The only strategy that gives
    /// liveness *and* a rewritten location *and* the call-time value, so it is
    /// what every operand with no other root gets — and also what a local or a
    /// module global gets, because those are mutable and re-deriving them would
    /// observe a later assignment instead of the value the call was given.
    Root,
    /// Emit no runtime call; re-derive the operand below the collection point.
    /// For operands whose storage is a registered root the collector rewrites
    /// **and** which are immutable, so re-lowering provably yields the same
    /// value at the corrected address. Only [`operand_is_reloadable`] answers
    /// yes here, and only for a string literal.
    Reload,
    /// Reuse the register. Correct in exactly two cases: nothing between this
    /// operand and its consumer can collect, or the value provably is not a
    /// heap reference and relocation cannot invalidate it.
    Reuse,
}

/// THE decision. Every operand-protection helper in this module routes through
/// it, so "root, re-derive, or reuse?" is answered in exactly one place.
///
/// It used to be answered in two, and they disagreed. [`RootedOperands`] paired
/// its suppression of string literals with the compensating re-load;
/// [`lower_exprs_rooted`] suppressed them and reused the register. That is
/// #7114: `"acc:" + run(1e7)` lowers through `lower_string_coerce_concat` →
/// `lower_operand_pair_rooted` → `lower_exprs_rooted`, the literal's handle was
/// loaded before the call and masked to a pointer after it, and once `run` drove
/// an evacuating minor the concat read the string's *old* address — printing an
/// empty line and exiting 0.
///
/// Keeping the two predicates but calling them from two places is what let the
/// pair drift, so the fix is the single call site, not a second copy of the
/// re-load.
pub(crate) fn operand_protection(
    ctx: &FnCtx<'_>,
    expr: &Expr,
    collects: bool,
) -> OperandProtection {
    if !collects {
        // Nothing can be swept and nothing can move before the consumer runs,
        // so the register still holds the value the call observes. This is the
        // gate that keeps `total + s.length`, `f(x, y)` and `[1, 2, 3]` at
        // exactly the IR they emitted before #6951.
        return OperandProtection::Reuse;
    }
    if operand_needs_root(ctx, expr) {
        return OperandProtection::Root;
    }
    if operand_is_reloadable(expr) {
        return OperandProtection::Reload;
    }
    // Suppressed by `expr_is_known_non_pointer_shadow_value`: not a heap
    // reference, so there is nothing for the collector to move.
    OperandProtection::Reuse
}

/// Open an expression-scope temp-root barrier for a call/constructor whose
/// operands are `args`.
///
/// Pushes a null marker slot and returns its index. Because
/// [`temp_root_truncate`] is a stack *cut*, [`temp_root_scope_end`] drops the
/// marker and every slot pushed above it — no matter which of the callee's
/// return paths ran. That is what makes rooting tractable in
/// `lower_call/new.rs`, where `lowered_args` is consumed at a dozen sites
/// spread over ~20 return paths (#6969); the alternative is a `temp_root_release`
/// at each, which is exactly the bookkeeping that gets missed.
///
/// A null word decodes to nothing, so the marker itself roots no object.
/// Emits nothing when nothing inside the scope could ever need rooting.
///
/// #7154: `also_needed` is the caller's extra reason to open the scope beyond
/// its operands. `lower_new_impl_inner` roots the freshly-allocated *instance*
/// across the constructor body, and `new C()` with no arguments is precisely
/// the shape that would otherwise push a slot with no marker above it to cut —
/// a temp-root entry leaked per construction.
pub(crate) fn temp_root_scope_begin(
    ctx: &mut FnCtx<'_>,
    args: &[Expr],
    also_needed: bool,
) -> Option<String> {
    (also_needed || args.iter().any(|a| operand_needs_root(ctx, a)))
        .then(|| temp_root_push_i64(ctx, "0"))
}

/// Close a barrier opened by [`temp_root_scope_begin`].
pub(crate) fn temp_root_scope_end(ctx: &mut FnCtx<'_>, scope: Option<String>) {
    if let Some(idx) = scope {
        temp_root_truncate(ctx, &idx);
    }
}
