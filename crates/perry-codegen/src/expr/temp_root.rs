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
fn expr_is_inert_primitive(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
    match expr {
        Expr::Undefined | Expr::Null | Expr::Bool(_) | Expr::Number(_) | Expr::Integer(_) => true,
        // A heap value, but ToPrimitive on a string is the identity: no user
        // code, no allocation. (`+` is excluded below, since concatenation
        // does allocate.)
        Expr::String(_) => true,
        Expr::LocalGet(id) => {
            !ctx.shadow_slot_map.contains_key(id)
                && matches!(
                    ctx.local_types.get(id),
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
        Expr::Unary { operand, .. } => expr_is_inert_primitive(ctx, operand),
        Expr::Compare { left, right, .. } => {
            expr_is_inert_primitive(ctx, left) && expr_is_inert_primitive(ctx, right)
        }
        // `+` allocates whenever it is a concatenation, so it is never inert
        // even over two string literals.
        Expr::Binary { op, left, right } => {
            !matches!(op, perry_hir::BinaryOp::Add)
                && expr_is_inert_primitive(ctx, left)
                && expr_is_inert_primitive(ctx, right)
        }
        _ => false,
    }
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
/// Returns the lowered values — **re-read from their roots**, so they are
/// valid after an evacuating cycle — and the guard index the caller must pass
/// to [`temp_root_release`] once the consuming call has run. `None` means
/// nothing needed protecting and no runtime calls were emitted.
pub(crate) fn lower_exprs_rooted(
    ctx: &mut FnCtx<'_>,
    exprs: &[&Expr],
) -> anyhow::Result<(Vec<String>, Option<String>)> {
    let mut values = Vec::with_capacity(exprs.len());
    let mut slots: Vec<Option<String>> = Vec::with_capacity(exprs.len());
    let mut guard: Option<String> = None;
    for (i, expr) in exprs.iter().enumerate() {
        let value = super::lower_expr(ctx, expr)?;
        // A value that provably cannot be a heap reference roots nothing, so a
        // slot for it is pure TLS traffic. This is the gate that keeps
        // `total + s.length` and other numeric operand pairs at their old IR.
        //
        // A string literal is skipped for the opposite reason: it is a load
        // from a module global that `__perry_init_strings_*` registered with
        // `js_gc_register_global_root`, so it already has a precise root and
        // the sweep can never take it. (A register loaded from that global is
        // still stale after an *evacuating* cycle — but that is true of every
        // `Expr::String` use in the compiler, not something this site
        // introduces, and it is not the hazard #6951 is about.) Template
        // literals are mostly literal parts, so this matters.
        let needs_root = !super::expr_is_known_non_pointer_shadow_value(ctx, expr)
            && !matches!(expr, Expr::String(_));
        if needs_root && any_later_ref_may_trigger_gc(ctx, exprs, i) {
            let idx = temp_root_push_double(ctx, &value);
            // The FIRST slot pushed is the guard: truncating it drops every
            // slot above it too, so one call releases the whole group.
            if guard.is_none() {
                guard = Some(idx.clone());
            }
            slots.push(Some(idx));
        } else {
            slots.push(None);
        }
        values.push(value);
    }
    for (value, slot) in values.iter_mut().zip(slots.iter()) {
        if let Some(idx) = slot {
            *value = temp_root_get_double(ctx, idx);
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
    /// The registers as originally lowered — the answer when nothing is rooted.
    values: Vec<String>,
    /// First slot pushed; truncating it drops the whole group.
    guard: Option<String>,
}

/// Root each of `values` (NaN-boxed `double` registers) whose corresponding
/// `protect` flag says something between here and the consuming call can
/// collect *and* the operand is not already rooted elsewhere.
///
/// The caller supplies the flags precisely because the hazard is not visible in
/// an expression list: for `m.set(k, v)` it is `v`'s lowering, for
/// `new C(a, b)` it is the instance allocation. Pair each flag with
/// [`operand_needs_root`] so a plain local receiver — already held by the
/// shadow stack — keeps its old IR.
pub(crate) fn root_operands(
    ctx: &mut FnCtx<'_>,
    values: &[&str],
    protect: &[bool],
) -> RootedOperands {
    let mut slots = Vec::with_capacity(values.len());
    let mut guard: Option<String> = None;
    for (i, value) in values.iter().enumerate() {
        if protect.get(i).copied().unwrap_or(false) {
            let idx = temp_root_push_double(ctx, value);
            if guard.is_none() {
                guard = Some(idx.clone());
            }
            slots.push(Some(idx));
        } else {
            slots.push(None);
        }
    }
    RootedOperands {
        slots,
        values: values.iter().map(|v| (*v).to_string()).collect(),
        guard,
    }
}

impl RootedOperands {
    /// Re-read every rooted operand. Mandatory after the collection point, not
    /// defensive: the slot is a *mutable* root, so an evacuating cycle rewrites
    /// it and the register pushed beforehand is stale.
    ///
    /// Emits nothing when nothing was rooted.
    pub(crate) fn reread(&self, ctx: &mut FnCtx<'_>) -> Vec<String> {
        self.slots
            .iter()
            .zip(self.values.iter())
            .map(|(slot, original)| match slot {
                Some(idx) => {
                    let idx = idx.clone();
                    temp_root_get_double(ctx, &idx)
                }
                None => original.clone(),
            })
            .collect()
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
/// Three suppressions, each meaning "already rooted, or nothing to root":
///
/// - provably not a heap reference — a slot for it is pure TLS traffic;
/// - a string literal — a load from a module global `__perry_init_strings_*`
///   registered with `js_gc_register_global_root`;
/// - a plain local or module-global read — the shadow stack and the module-var
///   scanners already hold those for as long as generated code can see them.
///
/// The last one is why `new C(a, b)` on plain locals stays at its old IR even
/// though the instance allocation that follows always collects.
pub(crate) fn operand_needs_root(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
    !super::expr_is_known_non_pointer_shadow_value(ctx, expr)
        && !matches!(
            expr,
            Expr::String(_) | Expr::LocalGet(_) | Expr::GlobalGet(_)
        )
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
/// Emits nothing when no operand could ever need rooting.
pub(crate) fn temp_root_scope_begin(ctx: &mut FnCtx<'_>, args: &[Expr]) -> Option<String> {
    args.iter()
        .any(|a| operand_needs_root(ctx, a))
        .then(|| temp_root_push_i64(ctx, "0"))
}

/// Close a barrier opened by [`temp_root_scope_begin`].
pub(crate) fn temp_root_scope_end(ctx: &mut FnCtx<'_>, scope: Option<String>) {
    if let Some(idx) = scope {
        temp_root_truncate(ctx, &idx);
    }
}
