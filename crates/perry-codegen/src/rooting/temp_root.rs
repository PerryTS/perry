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

use crate::expr::FnCtx;

/// Per-function pool of frame-rooted temp allocas (#7469).
///
/// The FFI contract above cost three runtime calls per protected temporary —
/// push, mandatory re-read, truncate — and on `churn.ts` those three were 206
/// of the 542 remaining `_tlv_get_addr`-attributed samples after #7474. But
/// the function's *named* locals already demonstrate the cheap form of the
/// same root: an entry alloca bound to a shadow-frame slot, written and
/// re-read with plain stores and loads, upgraded by the RS4GC / stack-map
/// lowering into a relocated `addrspace(1)` slot. A temp needs nothing a
/// local doesn't; this pool gives temps the identical mechanism.
///
/// The pool is **compile-time bookkeeping only** — at runtime there is no
/// stack to balance, no depth to restore on `longjmp` (the slots die with the
/// shadow frame like every local slot), and nothing for
/// `ShadowSavepoint`'s temp-depth restore to do (the FFI stack it snapshots
/// simply stays empty).
///
/// Slot handles keep the same `String` currency the FFI version used, so
/// every caller — including the nested `RootedOperands` /
/// `StoreOperandGuard` machinery — compiles unchanged. In alloca mode the
/// string is the entry-alloca register; in fallback mode it is the FFI
/// index register, exactly as before.
///
/// Reuse is a stack watermark, mirroring `js_gc_temp_root_truncate`'s
/// "drop `base` and everything above it" contract: releasing a handle frees
/// it AND every handle acquired after it. That is the discipline the FFI
/// API already imposed on callers (a truncate at index N invalidated all
/// higher indexes), so no caller can observe the difference.
#[derive(Default)]
pub(crate) struct TempRootPool {
    /// `(entry alloca register, reserved frame slot index)` in acquisition
    /// order. Entries at positions `>= active` are free for reuse.
    slots: Vec<(String, u32)>,
    /// Stack watermark: the number of live handles.
    active: usize,
    /// `Some(true)` — this function lowers temps onto frame-rooted allocas.
    /// `Some(false)` — shadow-stack emission is off for this build (
    /// `reserve_shadow_slot` returned `None`); every temp uses the runtime
    /// FFI stack, byte-for-byte the pre-#7469 emission. Decided at the first
    /// acquisition and uniform for the whole function, so `get`/`set`/
    /// `truncate` can interpret handles without per-handle tags.
    alloca_mode: Option<bool>,
}

impl TempRootPool {
    /// Frame slot index for a live alloca handle. Panics on an unknown
    /// handle — that would mean a caller invented a slot string or used one
    /// across functions, both of which were equally broken under the FFI
    /// contract (a stale index addressed someone else's slot silently; this
    /// at least fails loudly at compile time).
    fn frame_idx(&self, handle: &str) -> u32 {
        self.slots
            .iter()
            .find(|(alloca, _)| alloca == handle)
            .map(|(_, idx)| *idx)
            .unwrap_or_else(|| panic!("temp-root handle {handle} not in this function's pool"))
    }

    /// Watermark position of a live handle, for release.
    fn position(&self, handle: &str) -> Option<usize> {
        self.slots[..self.active]
            .iter()
            .position(|(alloca, _)| alloca == handle)
    }
}

/// Acquire a pooled frame-rooted slot, or `None` when this build lowers
/// temps through the runtime FFI stack.
fn temp_pool_acquire(ctx: &mut FnCtx<'_>) -> Option<String> {
    if ctx.temp_roots.alloca_mode == Some(false) {
        return None;
    }
    let pos = ctx.temp_roots.active;
    if let Some((alloca, _)) = ctx.temp_roots.slots.get(pos) {
        let alloca = alloca.clone();
        ctx.temp_roots.active += 1;
        return Some(alloca);
    }
    // Grow the pool: entry alloca + on-demand frame slot. `reserve_shadow_slot`
    // rewrites the emitted `js_shadow_frame_enter` slot count in place, so the
    // #7184 hazard (slot index outside the pushed frame) cannot arise.
    let Some(slot_idx) = ctx.func.reserve_shadow_slot() else {
        ctx.temp_roots.alloca_mode = Some(false);
        return None;
    };
    ctx.temp_roots.alloca_mode = Some(true);
    let alloca = ctx.func.alloca_entry(I64);
    // Null-init at entry: the slot is scanned from bind onward, and the
    // RS4GC retype pass re-emits exactly this `store i64 0` as a null
    // `addrspace(1)` store.
    ctx.func.entry_allocas_push_store(I64, "0", &alloca);
    ctx.temp_roots.slots.push((alloca.clone(), slot_idx));
    ctx.temp_roots.active += 1;
    Some(alloca)
}

/// Root-store for an alloca-mode handle: plain store, then the same
/// bind + root-shading emission every named-local store uses. The bind must
/// be emitted here — after the store, before whatever collects — so the
/// rooted location dominates the collection point (#7192's invariant).
fn temp_slot_store(ctx: &mut FnCtx<'_>, handle: &str, value_i64: &str) {
    ctx.block().store(I64, value_i64, handle);
    let idx = ctx.temp_roots.frame_idx(handle);
    crate::expr::shadow_slot::emit_shadow_slot_bind_ptr(ctx, idx, handle);
}

/// Push `value_i64` (a bare heap pointer or NaN-boxed bits) and return the
/// slot handle.
pub(in crate::rooting) fn temp_root_push_i64(ctx: &mut FnCtx<'_>, value_i64: &str) -> String {
    if let Some(handle) = temp_pool_acquire(ctx) {
        temp_slot_store(ctx, &handle, value_i64);
        return handle;
    }
    ctx.block()
        .call(I32, "js_gc_temp_root_push", &[(I64, value_i64)])
}

/// Push a NaN-boxed `double` temporary and return the slot-index register.
pub(in crate::rooting) fn temp_root_push_double(ctx: &mut FnCtx<'_>, value: &str) -> String {
    let bits = ctx.block().bitcast_double_to_i64(value);
    temp_root_push_i64(ctx, &bits)
}

/// Re-read slot `idx` as a raw `i64`.
///
/// In alloca mode the re-read is a plain load — the collector rewrote the
/// alloca (shadow scan or statepoint relocation), so the load IS the
/// post-collection value, same as a named local's re-read.
pub(in crate::rooting) fn temp_root_get_i64(ctx: &mut FnCtx<'_>, idx: &str) -> String {
    if ctx.temp_roots.alloca_mode == Some(true) {
        return ctx.block().load(I64, idx);
    }
    ctx.block().call(I64, "js_gc_temp_root_get", &[(I32, idx)])
}

/// Re-read slot `idx` as a NaN-boxed `double`.
pub(in crate::rooting) fn temp_root_get_double(ctx: &mut FnCtx<'_>, idx: &str) -> String {
    let bits = temp_root_get_i64(ctx, idx);
    ctx.block().bitcast_i64_to_double(&bits)
}

/// Overwrite slot `idx` with a new raw `i64`.
///
/// For producers that hand back a *different* address each round — the
/// `concat` accumulator (#6971), where every `js_string_concat` yields a new
/// string and the old one stops being the value that must stay alive.
pub(in crate::rooting) fn temp_root_set_i64(ctx: &mut FnCtx<'_>, idx: &str, value_i64: &str) {
    if ctx.temp_roots.alloca_mode == Some(true) {
        temp_slot_store(ctx, idx, value_i64);
        return;
    }
    ctx.block()
        .call_void("js_gc_temp_root_set", &[(I32, idx), (I64, value_i64)]);
}

/// Overwrite slot `idx` with a new NaN-boxed `double`.
///
/// The `Object.assign` accumulator (#7200) is the same shape as the `concat`
/// one: `js_object_assign_one` returns the target's *post-collection* address,
/// so each link must republish rather than keep the address it passed in.
pub(in crate::rooting) fn temp_root_set_double(ctx: &mut FnCtx<'_>, idx: &str, value: &str) {
    let bits = ctx.block().bitcast_double_to_i64(value);
    temp_root_set_i64(ctx, idx, &bits);
}

/// Drop slot `idx` and everything pushed above it.
pub(in crate::rooting) fn temp_root_truncate(ctx: &mut FnCtx<'_>, idx: &str) {
    if ctx.temp_roots.alloca_mode == Some(true) {
        // Mirror the FFI contract exactly: drop `idx` and everything acquired
        // above it. Each released slot is zeroed (dropping its retention) and
        // its frame mirror cleared; the pool entry becomes reusable.
        //
        // A repeated release of an already-released handle (the documented
        // `implicit_this_restore` → outer-group interleaving) finds no
        // watermark position and is the same harmless no-op the FFI's
        // `base < len` guard made it.
        let Some(pos) = ctx.temp_roots.position(idx) else {
            return;
        };
        let released: Vec<(String, u32)> =
            ctx.temp_roots.slots[pos..ctx.temp_roots.active].to_vec();
        ctx.temp_roots.active = pos;
        for (alloca, slot_idx) in released {
            ctx.block().store(I64, "0", &alloca);
            crate::expr::shadow_slot::emit_shadow_slot_clear(ctx, slot_idx);
        }
        return;
    }
    ctx.block()
        .call_void("js_gc_temp_root_truncate", &[(I32, idx)]);
}

/// Push `value` onto the array held in temp-root slot `idx`, writing the
/// possibly-reallocated array pointer back into the slot.
///
/// The fused `js_array_push_f64_temp_rooted` runtime helper exists only to
/// collapse the FFI stack's get+push+set triple into one call; in alloca mode
/// the triple is a load, the push itself, and a store — so the plain
/// `js_array_push_f64` (which roots `value` internally on its grow path) is
/// the cheaper form and the fused helper stays FFI-fallback-only.
pub(in crate::rooting) fn temp_rooted_array_push(ctx: &mut FnCtx<'_>, idx: &str, value: &str) {
    if ctx.temp_roots.alloca_mode == Some(true) {
        let arr = ctx.block().load(I64, idx);
        let new_arr = ctx
            .block()
            .call(I64, "js_array_push_f64", &[(I64, &arr), (DOUBLE, value)]);
        temp_slot_store(ctx, idx, &new_arr);
        return;
    }
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
pub(in crate::rooting) fn rooted_array_begin(ctx: &mut FnCtx<'_>, cap: &str) -> String {
    let arr = ctx.block().call(I64, "js_array_alloc", &[(I32, cap)]);
    temp_root_push_i64(ctx, &arr)
}

/// Read the accumulator back out of its temp-root slot. Does NOT truncate:
/// callers truncate after the consuming call, so the array is still rooted
/// while the consumer runs (formatting an argument list allocates).
pub(in crate::rooting) fn rooted_array_read(ctx: &mut FnCtx<'_>, idx: &str) -> String {
    temp_root_get_i64(ctx, idx)
}

/// Can lowering `expr` reach a collection point?
///
/// Deliberately one-sided: `false` must mean "provably allocates nothing", and
/// everything unrecognized answers `true`. A wrong `false` is a
/// use-after-free; a wrong `true` costs two runtime calls on a cold path.
pub(in crate::rooting) fn expr_may_trigger_gc(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
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
        // A bounds- and lifetime-proven byte read is a native load, not a
        // helper call.  This matters inside a store RHS: classifying
        // `buf[i]` as collecting made `buf[i] = (buf[i] + 1) & 255` discard
        // the enclosing store's cached-view proof even though the RHS lowers
        // to a load and integer arithmetic only.
        Expr::Uint8ArrayGet { array, index } => crate::expr::can_lower_buffer_access_without_calls(
            ctx,
            array,
            index,
            crate::expr::BufferAccessSpec::uint8array_get(),
        ),
        Expr::BufferIndexGet { buffer, index } => {
            crate::expr::can_lower_buffer_access_without_calls(
                ctx,
                buffer,
                index,
                crate::expr::BufferAccessSpec::buffer_index_get(),
            )
        }
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
                    || (crate::expr::expr_is_known_non_pointer_shadow_value(ctx, left)
                        && crate::expr::expr_is_known_non_pointer_shadow_value(ctx, right)))
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
/// A declaration is deliberately absent from this judgment. The type arm uses
/// only runtime-derived evidence; `integer_locals` and
/// `number_by_construction_locals` are the separate whole-write structural
/// proofs for integer recurrences and general Number-valued accumulators. A
/// lying scalar annotation therefore retains its root and cannot make coercion
/// look inert.
pub(in crate::rooting) fn local_is_inert_primitive(ctx: &FnCtx<'_>, id: u32) -> bool {
    !ctx.shadow_slot_map.contains_key(&id)
        && !ctx.module_globals.contains_key(&id)
        && (ctx.integer_locals.contains(&id)
            || ctx.number_by_construction_locals.contains(&id)
            || matches!(
                ctx.stable_local_type_proof(&id),
                Some(
                    HirType::Number
                        | HirType::Int32
                        | HirType::Boolean
                        | HirType::Null
                        | HirType::Void
                        | HirType::Never
                )
            ))
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
pub(in crate::rooting) struct RootedOperands {
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
pub(in crate::rooting) fn operand_is_reloadable(expr: &Expr) -> bool {
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
pub(in crate::rooting) fn root_operands_begin(capacity: usize) -> RootedOperands {
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
    pub(in crate::rooting) fn push(
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
    pub(in crate::rooting) fn reread(
        &self,
        ctx: &mut FnCtx<'_>,
        operands: &[&Expr],
    ) -> anyhow::Result<Vec<String>> {
        let mut out = Vec::with_capacity(self.values.len());
        for i in 0..self.values.len() {
            out.push(self.reread_one(ctx, operands, i)?);
        }
        Ok(out)
    }

    /// Re-read ONE operand, at a point the caller picks.
    ///
    /// [`RootedOperands::reread`] re-reads the whole group at a single point,
    /// which is right when one collection point separates the group from its
    /// consumer. It is wrong when the operands are consumed by *different*
    /// instructions with a collection point between them — the generic
    /// dynamic-call lowering is exactly that shape (#7154): the callee and the
    /// `this` receiver are consumed by `js_closure_unbox_callee_checked_rebind`,
    /// that rebind CLONES a `this`-capturing closure and therefore allocates,
    /// and only then does `js_closure_callN` consume the arguments. Re-reading
    /// the arguments above the rebind would put them right back in the window
    /// the roots exist to close.
    ///
    /// Same three cases as [`RootedOperands::reread`]; see its documentation.
    pub(in crate::rooting) fn reread_one(
        &self,
        ctx: &mut FnCtx<'_>,
        operands: &[&Expr],
        i: usize,
    ) -> anyhow::Result<String> {
        Ok(match &self.slots[i] {
            Some(idx) => {
                let idx = idx.clone();
                temp_root_get_double(ctx, &idx)
            }
            None if self.reloadable[i] => crate::expr::lower_expr(ctx, operands[i])?,
            None => self.values[i].clone(),
        })
    }

    /// Drop the group. Call it *after* the consuming call: the consumer
    /// allocates while reading these values.
    pub(in crate::rooting) fn release(self, ctx: &mut FnCtx<'_>) {
        temp_root_release(ctx, self.guard);
    }

    /// The group's guard slot, for a caller that must release it together with
    /// slots it pushed ITSELF.
    ///
    /// [`RootedOperands::release`] is the ordinary exit and consumes the group.
    /// The rest-argument lowering cannot use it: it pushes accumulator slots
    /// ([`rooted_array_begin`]) *above* this group, and because
    /// [`temp_root_truncate`] is a stack cut, one truncate at the LOWEST index
    /// drops both. So that caller needs the index rather than the act — and it
    /// must not release early, since the accumulator has to stay rooted across
    /// the consuming call too.
    pub(in crate::rooting) fn guard(&self) -> Option<String> {
        self.guard.clone()
    }
}

/// Release a guard returned by [`lower_exprs_rooted`]. Call it *after* the
/// consuming call, not before: the consumer allocates while reading these
/// values.
pub(in crate::rooting) fn temp_root_release(ctx: &mut FnCtx<'_>, guard: Option<String>) {
    if let Some(idx) = guard {
        temp_root_truncate(ctx, &idx);
    }
}

/// Do any of an object literal's / call's initializer expressions collect?
pub(in crate::rooting) fn any_may_trigger_gc<'a>(
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
pub(in crate::rooting) fn operand_needs_root(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
    if crate::expr::expr_is_known_non_pointer_shadow_value(ctx, expr) {
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
pub(in crate::rooting) enum OperandProtection {
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
pub(in crate::rooting) fn operand_protection(
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
