//! String CONCATENATION and self-append lowering.
//!
//! Split out of `lower_string_method.rs` (#7615 slice 8). That file was 1,957
//! lines against `scripts/check_file_size.sh`'s 2,000-line cap, and the Layer 1
//! rooting migration has to add closure scopes to five of the functions here —
//! `with_operands_rooted` and `with_rooted_accumulator` both re-indent the body
//! they own, which is line growth on a file with 43 lines of headroom.
//!
//! Pure move, no behaviour change. The boundary is the one the file already
//! had: everything above it dispatches a `str.<method>(...)` call, everything
//! here lowers `a + b` / `s += x` on strings. `str_operand_handle_tag_dispatched`
//! is `pub(crate)` rather than private because three of the dispatch arms above
//! call it.

use anyhow::{anyhow, Result};
use perry_hir::Expr;

use crate::expr::{lower_expr, nanbox_string_inline, unbox_str_handle, FnCtx};
use crate::type_analysis::is_string_expr;
use crate::types::{DOUBLE, I32, I64};

use crate::rooting::{operand_may_collect, with_operands_rooted, with_rooted_group, Repr};

/// Lower the `str = str + rhs` self-append pattern. Uses the in-place
/// `js_string_append` runtime function (refcount=1 → mutate in place,
/// otherwise allocate). The returned pointer is stored back to the local
/// slot — `js_string_append` may realloc when growing past capacity.
///
/// This is the load-bearing optimization for the canonical `let str = "";
/// for (...) str = str + "a"` string-build pattern.
pub(crate) fn lower_string_self_append(
    ctx: &mut FnCtx<'_>,
    local_id: u32,
    rhs: &Expr,
) -> Result<String> {
    let slot = ctx
        .locals
        .get(&local_id)
        .ok_or_else(|| anyhow!("string self-append: local {} not in scope", local_id))?
        .clone();

    // A declared string type is permission to select this lowering, not proof
    // that the slot contains a string. Use the same inline tag dispatch for
    // canonical and ordinary boxed locals: this keeps the true-string append
    // arm direct and makes the annotation-lie arm choose the real JS `+`.
    lower_tag_dispatched_str_self_append(ctx, rhs, &slot)
}

/// Repsel Phase 3a: is this expression PROVEN to lower to a heap-tagged
/// (`STRING_TAG`) NaN-box — never SSO bits, never a non-string? String
/// literals load the interned pool handle (`@.str.N.handle`, always a heap
/// `StringHeader` from `js_string_from_bytes`); `String(x)` routes through
/// `js_string_coerce`, which always allocates a heap header. Deliberately
/// NOT included: `Binary Add` string results — the pairwise concat lowering
/// returns `js_string_concat_box`, which assembles ≤5-byte ASCII results as
/// SSO bits.
fn proven_heap_string_operand(_ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::String(_) | Expr::WtfString(_) | Expr::StringCoerce(_) => true,
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => {
            proven_heap_string_operand(_ctx, then_expr)
                && proven_heap_string_operand(_ctx, else_expr)
        }
        _ => false,
    }
}

/// Repsel Phase 3a: operand → raw `StringHeader*` handle for the string
/// helpers, tag-dispatched:
///
/// - proven heap-tagged operand (see `proven_heap_string_operand`) → inline
///   `bitcast; and POINTER_MASK` — zero calls;
/// - canonical-Str `LocalGet` → 2-arm dispatch: heap `STRING_TAG` bits →
///   bare `and POINTER_MASK` (hot arm, no call); anything else (SSO bits,
///   annotation lie) → the legacy `js_get_string_pointer_unified` (which
///   materializes SSO — cold);
/// - everything else (or flag off) → the legacy unified call, unchanged.
///
/// #7128: the two arms are on separate knobs, because only the second one is
/// about a selected representation. The proven-heap arm keys on the operand's
/// static type and fires with zero canonical-`Str` locals in the program.
pub(crate) fn str_operand_handle_tag_dispatched(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    recv_box: &str,
) -> String {
    use crate::nanbox::POINTER_MASK_I64;
    if crate::expr::static_string_lowering_enabled() && proven_heap_string_operand(ctx, object) {
        let bits = ctx.block().bitcast_double_to_i64(recv_box);
        return ctx.block().and(I64, &bits, POINTER_MASK_I64);
    }
    let canonical = crate::expr::canonical_str_locals_enabled()
        && matches!(
            object, Expr::LocalGet(id) if crate::expr::local_is_canonical_str(ctx, *id)
        );
    if !canonical {
        return unbox_str_handle(ctx.block(), recv_box);
    }
    let bits = ctx.block().bitcast_double_to_i64(recv_box);
    let tag = ctx.block().lshr(I64, &bits, "48");
    let is_heap = ctx
        .block()
        .icmp_eq(I64, &tag, crate::nanbox::STRING_TAG_TOP16_I64);

    let heap_idx = ctx.new_block("strrecv.heap");
    let cold_idx = ctx.new_block("strrecv.cold");
    let merge_idx = ctx.new_block("strrecv.merge");
    let heap_label = ctx.block_label(heap_idx);
    let cold_label = ctx.block_label(cold_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&is_heap, &heap_label, &cold_label);

    ctx.current_block = heap_idx;
    let h_heap = ctx.block().and(I64, &bits, POINTER_MASK_I64);
    let heap_pred = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = cold_idx;
    let h_cold = unbox_str_handle(ctx.block(), recv_box);
    let cold_pred = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    ctx.block()
        .phi(I64, &[(&h_heap, &heap_pred), (&h_cold, &cold_pred)])
}

/// `s += rhs` for a local selected from a declared string type. Runtime tags,
/// not the annotation, choose the actual `+` operator (#7841). Canonical-Str
/// locals originally introduced this inline dispatch; ordinary boxed locals
/// now use it too so disabling representation selection cannot reintroduce a
/// wrong answer.
///
/// - **both heap** (`STRING_TAG` on both sides): `and POINTER_MASK` →
///   `js_string_append(h, h)` → `or STRING_TAG` — the hot accumulator-loop
///   arm; keeps the refcount==1 in-place append (every alias demote site is
///   untouched by this phase, so `let b = a` still demotes first).
/// - **both strings, SSO involved**: `js_string_concat_box(box, box)` —
///   SSO-aware pairwise concat, assembles ≤5-byte ASCII results inline and
///   never mutates in place. No per-op heap materialization of SSO bits
///   (RFC §4 "short-string values stay by-value").
/// - **anything else** (a lying `string` annotation):
///   `js_dynamic_string_or_number_add`, without any prior ToString coercion.
fn lower_tag_dispatched_str_self_append(
    ctx: &mut FnCtx<'_>,
    rhs: &Expr,
    slot: &str,
) -> Result<String> {
    use crate::nanbox::{
        POINTER_MASK_I64, POINTER_TAG_TOP16_I64 as TAG_POINTER,
        SHORT_STRING_TAG_TOP16_I64 as TAG_SSO_STR, STRING_TAG_TOP16_I64 as TAG_HEAP_STR,
    };

    if !is_string_expr(ctx, rhs) {
        // Snapshot lhs before evaluating rhs, as compound assignment requires.
        // Test the destination tag (and the rhs object-pointer tag) before any
        // ToString call. A pointer rhs needs Add's default-hint ToPrimitive, so
        // it joins the dynamic arm; primitive rhs values keep the old direct
        // coercion + in-place append sequence.
        let lhs_box = ctx.block().load(DOUBLE, slot);
        let protect_lhs = operand_may_collect(ctx, rhs);
        return with_rooted_group(ctx, 0, |ctx, group| {
            let lhs_root = group.adopt_emitted(ctx, Repr::Boxed, &lhs_box, protect_lhs);
            let rhs_box = lower_expr(ctx, rhs)?;
            let lhs_box = group.reread_emitted(ctx, lhs_root);
            let bits_d = ctx.block().bitcast_double_to_i64(&lhs_box);
            let tag_d = ctx.block().lshr(I64, &bits_d, "48");
            let d_heap = ctx.block().icmp_eq(I64, &tag_d, TAG_HEAP_STR);

            let dheap_idx = ctx.new_block("strapp.dheap");
            let append_idx = ctx.new_block("strapp.append");
            let dynamic_idx = ctx.new_block("strapp.dynamic");
            let merge_idx = ctx.new_block("strapp.merge");
            let dheap_label = ctx.block_label(dheap_idx);
            let append_label = ctx.block_label(append_idx);
            let dynamic_label = ctx.block_label(dynamic_idx);
            let merge_label = ctx.block_label(merge_idx);
            ctx.block().cond_br(&d_heap, &dheap_label, &dynamic_label);

            ctx.current_block = dheap_idx;
            let bits_r = ctx.block().bitcast_double_to_i64(&rhs_box);
            let tag_r = ctx.block().lshr(I64, &bits_r, "48");
            let r_pointer = ctx.block().icmp_eq(I64, &tag_r, TAG_POINTER);
            ctx.block()
                .cond_br(&r_pointer, &dynamic_label, &append_label);

            ctx.current_block = append_idx;
            let r_handle = ctx
                .block()
                .call(I64, "js_jsvalue_to_string", &[(DOUBLE, &rhs_box)]);
            // The coercion can collect. If rhs evaluation needed an old-value
            // root, re-read it; otherwise rhs was inert and the local slot is
            // still the same value, so reload its GC-updated bits directly.
            let lhs_after_coercion = if protect_lhs {
                group.reread_emitted(ctx, lhs_root)
            } else {
                ctx.block().load(DOUBLE, slot)
            };
            let bits_d_after = ctx.block().bitcast_double_to_i64(&lhs_after_coercion);
            let h_d = ctx.block().and(I64, &bits_d_after, POINTER_MASK_I64);
            let h_new = ctx
                .block()
                .call(I64, "js_string_append", &[(I64, &h_d), (I64, &r_handle)]);
            let box_append = nanbox_string_inline(ctx.block(), &h_new);
            let append_pred = ctx.block().label.clone();
            ctx.block().br(&merge_label);

            ctx.current_block = dynamic_idx;
            let box_dynamic = ctx.block().call(
                DOUBLE,
                "js_dynamic_string_or_number_add",
                &[(DOUBLE, &lhs_box), (DOUBLE, &rhs_box)],
            );
            let dynamic_pred = ctx.block().label.clone();
            ctx.block().br(&merge_label);

            ctx.current_block = merge_idx;
            let new_box = ctx.block().phi(
                DOUBLE,
                &[(&box_append, &append_pred), (&box_dynamic, &dynamic_pred)],
            );
            ctx.block().store(DOUBLE, &new_box, slot);
            Ok(new_box)
        });
    }

    // Proven-string rhs. Compound assignment still snapshots lhs before rhs;
    // a collecting rhs therefore gets a temporary root for that old value.
    //
    // Arm layout — the load-bearing property is that a HEAP destination
    // ALWAYS reaches `js_string_append` (whose refcount==1 in-place path is
    // what makes accumulator loops amortized O(n)). Routing a heap-dest /
    // SSO-rhs iteration through `js_string_concat_box` instead would copy
    // the whole accumulator every time a ≤5-byte part arrives — O(n²).
    //
    //   dest heap, rhs heap  → append(h, h)                 (hot, no calls)
    //   dest heap, rhs SSO   → unified(rhs), append(h, h)   (still in-place)
    //   dest heap, rhs lie   → dynamic `+`                  (cold)
    //   dest other           → js_string_concat_box          (SSO-aware and
    //                          total: lies delegate to dynamic `+`)
    let lhs_box = ctx.block().load(DOUBLE, slot);
    let protect_lhs = operand_may_collect(ctx, rhs);
    with_rooted_group(ctx, 0, |ctx, group| {
        let lhs_root = group.adopt_emitted(ctx, Repr::Boxed, &lhs_box, protect_lhs);
        let rhs_box = lower_expr(ctx, rhs)?;
        let lhs_box = group.reread_emitted(ctx, lhs_root);
        let bits_d = ctx.block().bitcast_double_to_i64(&lhs_box);
        let bits_r = ctx.block().bitcast_double_to_i64(&rhs_box);
        let tag_d = ctx.block().lshr(I64, &bits_d, "48");
        let tag_r = ctx.block().lshr(I64, &bits_r, "48");
        let d_heap = ctx.block().icmp_eq(I64, &tag_d, TAG_HEAP_STR);

        let dheap_idx = ctx.new_block("strapp.dheap");
        let heap_idx = ctx.new_block("strapp.heap");
        let rnotheap_idx = ctx.new_block("strapp.rnotheap");
        let rsso_idx = ctx.new_block("strapp.rsso");
        let dynamic_idx = ctx.new_block("strapp.dynamic");
        let dother_idx = ctx.new_block("strapp.dother");
        let merge_idx = ctx.new_block("strapp.merge");
        let dheap_label = ctx.block_label(dheap_idx);
        let heap_label = ctx.block_label(heap_idx);
        let rnotheap_label = ctx.block_label(rnotheap_idx);
        let rsso_label = ctx.block_label(rsso_idx);
        let dynamic_label = ctx.block_label(dynamic_idx);
        let dother_label = ctx.block_label(dother_idx);
        let merge_label = ctx.block_label(merge_idx);
        ctx.block().cond_br(&d_heap, &dheap_label, &dother_label);

        // The load-bearing hot arm remains exactly one tag check followed by
        // raw handle masks and the in-place append call.
        ctx.current_block = dheap_idx;
        let r_heap = ctx.block().icmp_eq(I64, &tag_r, TAG_HEAP_STR);
        ctx.block().cond_br(&r_heap, &heap_label, &rnotheap_label);

        ctx.current_block = heap_idx;
        let h_d = ctx.block().and(I64, &bits_d, POINTER_MASK_I64);
        let h_r = ctx.block().and(I64, &bits_r, POINTER_MASK_I64);
        let h_new = ctx
            .block()
            .call(I64, "js_string_append", &[(I64, &h_d), (I64, &h_r)]);
        let box_heap = nanbox_string_inline(ctx.block(), &h_new);
        let heap_pred = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        ctx.current_block = rnotheap_idx;
        let r_sso = ctx.block().icmp_eq(I64, &tag_r, TAG_SSO_STR);
        ctx.block().cond_br(&r_sso, &rsso_label, &dynamic_label);

        ctx.current_block = rsso_idx;
        let r_handle = unbox_str_handle(ctx.block(), &rhs_box);
        let lhs_after_materialize = if protect_lhs {
            group.reread_emitted(ctx, lhs_root)
        } else {
            ctx.block().load(DOUBLE, slot)
        };
        let bits_d_after = ctx.block().bitcast_double_to_i64(&lhs_after_materialize);
        let h_d_after = ctx.block().and(I64, &bits_d_after, POINTER_MASK_I64);
        let h_sso = ctx.block().call(
            I64,
            "js_string_append",
            &[(I64, &h_d_after), (I64, &r_handle)],
        );
        let box_sso = nanbox_string_inline(ctx.block(), &h_sso);
        let rsso_pred = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        ctx.current_block = dynamic_idx;
        let box_dynamic = ctx.block().call(
            DOUBLE,
            "js_dynamic_string_or_number_add",
            &[(DOUBLE, &lhs_box), (DOUBLE, &rhs_box)],
        );
        let dynamic_pred = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        ctx.current_block = dother_idx;
        let box_other = ctx.block().call(
            DOUBLE,
            "js_string_concat_box",
            &[(DOUBLE, &lhs_box), (DOUBLE, &rhs_box)],
        );
        let dother_pred = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        ctx.current_block = merge_idx;
        let new_box = ctx.block().phi(
            DOUBLE,
            &[
                (&box_heap, &heap_pred),
                (&box_sso, &rsso_pred),
                (&box_dynamic, &dynamic_pred),
                (&box_other, &dother_pred),
            ],
        );
        ctx.block().store(DOUBLE, &new_box, slot);
        Ok(new_box)
    })
}

/// Lower `string + non_string` (or vice versa) concat with runtime
/// coercion of the non-string side. The non-string operand passes through
/// `js_jsvalue_to_string` which inspects its NaN tag and produces the
/// canonical JS string form (numbers via the formatter at
/// `crates/perry-runtime/src/value.rs:825`, booleans → `"true"`/`"false"`,
/// objects → `"[object Object]"`, etc.).
///
/// The string-typed side still uses the fast inline `bitcast double → i64;
/// and POINTER_MASK_I64` unbox; only the non-string side pays the function
/// call. Both operand handles then feed `js_string_concat`.
pub(crate) fn lower_string_coerce_concat(
    ctx: &mut FnCtx<'_>,
    left: &Expr,
    right: &Expr,
    l_is_string: bool,
    r_is_string: bool,
) -> Result<String> {
    // #6951: `l_box` is a heap string in an SSA register while `right` is
    // lowered. If `right` allocates (`"tag" + f()`), a collection sweeps the
    // left operand and the concat reads freed memory — a segfault, not a
    // dropped character. `with_operands_rooted` emits nothing at all when
    // `right` provably cannot collect, which is the common `"user_" + i` case,
    // and it owns the release on all three exits — including the two early
    // returns below, which is where #7462's "released on one arm" lived.
    with_operands_rooted(ctx, &[left, right], |ctx, values| {
        coerce_concat_body(
            ctx,
            left,
            right,
            &values[0],
            &values[1],
            l_is_string,
            r_is_string,
        )
    })
}

/// The body of [`lower_string_coerce_concat`], below its operand roots.
///
/// A separate function rather than a closure body so the three arms keep their
/// indentation — and so the ONE place that still needs a nested scope (the
/// both-non-string fallback, whose left coercion has to survive the right
/// coercion) reads as the exception it is.
#[allow(clippy::too_many_arguments)]
fn coerce_concat_body(
    ctx: &mut FnCtx<'_>,
    left: &Expr,
    right: &Expr,
    l_box: &str,
    r_box: &str,
    l_is_string: bool,
    r_is_string: bool,
) -> Result<String> {
    // Issue #58: fused string+value concat — when one side is a string
    // and the other is not, use the fused runtime call that collapses
    // js_jsvalue_to_string + js_string_concat into a single allocation
    // for number operands (the common `"item_" + i` pattern).
    if l_is_string && !r_is_string {
        // #7837: `l_is_string` may be nothing more than `let l: string`, and a
        // declared type is not enforced at runtime. This arm chooses the
        // OPERATOR, not just a representation — `s + 7` on a slot holding `42`
        // must answer `49`, not `"427"` — and the strict lowering below cannot
        // recover: it unboxes to a `StringHeader*` first, so the tag the
        // decision needs is gone before the helper sees it. Hand the box over
        // instead and let the helper pick. One predictable compare inside a
        // call that already allocates; no codegen diamond, so the honest
        // shape's fused single-allocation concat is untouched.
        if crate::type_analysis::string_proof_is_declared_only(ctx, left) {
            let blk = ctx.block();
            return Ok(blk.call(
                DOUBLE,
                "js_string_add_value",
                &[(DOUBLE, l_box), (DOUBLE, r_box)],
            ));
        }
        // Issue #214: SSO-safe unbox; repsel Phase 3a: inline `bitcast+and`
        // for proven-heap operands (string literals — the `"user_" + i`
        // shape) and tag-dispatch for canonical-Str locals.
        let l_handle = str_operand_handle_tag_dispatched(ctx, left, l_box);
        let blk = ctx.block();
        let result_handle = blk.call(
            I64,
            "js_string_concat_value",
            &[(I64, &l_handle), (DOUBLE, r_box)],
        );
        return Ok(nanbox_string_inline(blk, &result_handle));
    }

    if !l_is_string && r_is_string {
        // #7837, mirrored: see the left-string arm above.
        if crate::type_analysis::string_proof_is_declared_only(ctx, right) {
            let blk = ctx.block();
            return Ok(blk.call(
                DOUBLE,
                "js_value_add_string",
                &[(DOUBLE, l_box), (DOUBLE, r_box)],
            ));
        }
        // Issue #214: SSO-safe unbox; repsel Phase 3a: see above.
        let r_handle = str_operand_handle_tag_dispatched(ctx, right, r_box);
        let blk = ctx.block();
        let result_handle = blk.call(
            I64,
            "js_value_concat_string",
            &[(DOUBLE, l_box), (I64, &r_handle)],
        );
        return Ok(nanbox_string_inline(blk, &result_handle));
    }

    // Both non-string (shouldn't normally reach here) — fall back to
    // the generic path.
    let l_handle = ctx
        .block()
        .call(I64, "js_jsvalue_to_string", &[(DOUBLE, l_box)]);
    // The coercion of the right operand allocates, and `l_handle` is a bare
    // string address in an SSA register — root it across that call (#6951).
    // It is an EMITTED value (a coercion result, not a lowered `Expr`), so the
    // group is the only form that can take it.
    with_rooted_group(ctx, 0, |ctx, group| {
        let l_root = group.adopt_emitted(ctx, Repr::Ptr, &l_handle, true);
        let r_handle = ctx
            .block()
            .call(I64, "js_jsvalue_to_string", &[(DOUBLE, r_box)]);
        let l_handle = group.reread_emitted(ctx, l_root);
        let blk = ctx.block();
        let result_handle = blk.call(
            I64,
            "js_string_concat",
            &[(I64, &l_handle), (I64, &r_handle)],
        );
        Ok(nanbox_string_inline(blk, &result_handle))
    })
}

/// Lower a static `s1 + s2` string concatenation. Both operands must
/// already be statically string-typed (caller's responsibility — see
/// `is_string_expr`).
///
/// Pattern:
/// ```llvm
/// ; %l_box and %r_box are NaN-boxed strings (double values with STRING_TAG)
/// %l_bits = bitcast double %l_box to i64
/// %l_handle = and i64 %l_bits, 281474976710655   ; POINTER_MASK_I64
/// %r_bits = bitcast double %r_box to i64
/// %r_handle = and i64 %r_bits, 281474976710655
/// %result_handle = call i64 @js_string_concat(i64 %l_handle, i64 %r_handle)
/// %result_box = call double @js_nanbox_string(i64 %result_handle)
/// ```
///
/// The bitcast+and is the inline-fast unboxing pattern. We avoid calling
/// the slower `js_nanbox_get_pointer` (which does the same thing in Rust)
/// to keep concat hot-path overhead minimal.
pub(crate) fn lower_string_concat(
    ctx: &mut FnCtx<'_>,
    left: &Expr,
    right: &Expr,
) -> Result<String> {
    // #6951: same hazard as `lower_string_coerce_concat` — the left operand is
    // a heap string in an SSA register across the right operand's evaluation.
    with_operands_rooted(ctx, &[left, right], |ctx, values| {
        let blk = ctx.block();
        // SSO-aware fast path: pass operands as NaN-boxed f64s directly to
        // `js_string_concat_sso`, which keeps SSO operands inline (no
        // materialise-to-heap defeat) and returns the result NaN-boxed —
        // SSO when the total fits 5 bytes, heap-pointer otherwise. Saves up
        // to 3 heap allocations per concat on hot paths like ABC451D's
        // recursive `before + after` (1.4M concats with 1-9 byte operands).
        Ok(blk.call(
            DOUBLE,
            "js_string_concat_box",
            &[(DOUBLE, &values[0]), (DOUBLE, &values[1])],
        ))
    })
}

/// Cap the per-call part count for the n-way fold. Must match the
/// runtime's `MAX_PARTS` in `js_string_concat_chain`. 32 covers every
/// realistic CSV / log-line / template chain in user code.
const CONCAT_CHAIN_MAX_PARTS: usize = 32;

/// Try to flatten a left-spine of `Binary { Add }` nodes where every Add
/// has at least one statically-string operand. Returns the parts in
/// left-to-right (source-order) order. Returns `None` if the chain is
/// shorter than the existing pairwise fast path's preference, has too
/// many parts, or contains an Add node where neither side is statically
/// string (which would risk numeric semantics under JS spec).
///
/// Caller passes the OUTERMOST Add's children. If the outermost Add's
/// left child is itself a string-shaped Add, we recurse into it; right
/// children are always leaves in our flat representation.
pub(crate) fn flatten_string_add_chain<'a>(
    ctx: &FnCtx<'_>,
    left: &'a Expr,
    right: &'a Expr,
) -> Option<Vec<&'a Expr>> {
    use perry_hir::BinaryOp;

    let mut parts: Vec<&Expr> = Vec::with_capacity(8);
    parts.push(right);

    // Walk down the left spine. At each step, the current `cur` was the
    // left child of an Add we already accepted — so we know `cur + ...`
    // is string-shaped at the level above. We need each Add we descend
    // INTO to itself be string-shaped (≥1 statically-string operand), so
    // the entire chain has unambiguous string semantics.
    let mut cur: &Expr = left;
    loop {
        match cur {
            Expr::Binary {
                op: BinaryOp::Add,
                left: l,
                right: r,
            } => {
                let l_str = crate::type_analysis::string_value_is_runtime_guaranteed(ctx, l);
                let r_str = crate::type_analysis::string_value_is_runtime_guaranteed(ctx, r);
                if !l_str && !r_str {
                    // Stop the descent — this Add isn't unambiguously
                    // string-shaped. Treat the entire `cur` subtree as
                    // one opaque part.
                    parts.push(cur);
                    break;
                }
                parts.push(r);
                cur = l;
                if parts.len() >= CONCAT_CHAIN_MAX_PARTS {
                    return None;
                }
            }
            _ => {
                parts.push(cur);
                break;
            }
        }
    }

    parts.reverse();
    Some(parts)
}

/// Lower a flat parts list to a single `js_string_concat_chain` call.
/// Each part is lowered to its NaN-boxed value, then stored into a
/// stack-allocated `[CONCAT_CHAIN_MAX_PARTS x double]` buffer; we pass
/// the base pointer + N to the runtime helper, which produces a single
/// allocation containing the entire concatenated result.
///
/// The buffer is fixed-size (always sized to MAX_PARTS) and hoisted to
/// the function entry block via `alloca_entry_array`. A non-entry-block
/// alloca lowers to a runtime `sub %rsp, N` with no matching restore;
/// inside a loop body that's a stack leak (issue #167 — same shape that
/// blew up `buf.readInt32BE` in tight loops). Function-entry allocas
/// run once at prologue and the slot dominates every reachable use.
/// One per-function buffer is shared across all chain call sites — fine
/// because each chain call writes its parts and immediately calls into
/// the runtime helper before any other call site can clobber the slots.
pub(crate) fn lower_string_concat_chain(ctx: &mut FnCtx<'_>, parts: &[&Expr]) -> Result<String> {
    debug_assert!(parts.len() >= 2);
    debug_assert!(parts.len() <= CONCAT_CHAIN_MAX_PARTS);

    // Lower each part first (in source order); side effects must fire
    // left-to-right per JS spec. #6951: that ordering is exactly what makes
    // every earlier part a heap value in an SSA register across every later
    // part's evaluation — this is the template-literal / log-line shape, and
    // one allocating interpolation was enough to sweep the parts already
    // lowered. Parts that nothing allocating follows emit no rooting calls.
    with_operands_rooted(ctx, parts, |ctx, lowered| {
        let n = lowered.len();
        // Hoist the buffer to the function entry block. Issue #167.
        let buf_reg = ctx.func.alloca_entry_array(DOUBLE, CONCAT_CHAIN_MAX_PARTS);
        let blk = ctx.block();
        for (i, val) in lowered.iter().enumerate() {
            let slot = blk.gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
            blk.store(DOUBLE, val, &slot);
        }
        // Pass the array's base pointer as i64 (codegen ABI uses i64 for
        // raw pointer args matching the existing `js_string_concat` shape).
        let base_i64 = blk.next_reg();
        blk.emit_raw(format!("{} = ptrtoint ptr {} to i64", base_i64, buf_reg));

        let result_handle = blk.call(
            I64,
            "js_string_concat_chain",
            &[(I64, &base_i64), (I32, &format!("{}", n))],
        );
        Ok(nanbox_string_inline(blk, &result_handle))
    })
}
