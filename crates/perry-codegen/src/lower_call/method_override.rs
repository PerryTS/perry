//! Issue #620 own-method-override runtime check.
//!
//! Extracted from `lower_call.rs` (#1099, part of #1097) — pure move,
//! no behavior change. `emit_own_override_value_call` calls the own-property
//! override (`this.method = X`, or `class X { method = fn; }`) that a
//! class-method site's `perry_probe_own_method` found, ahead of any prototype
//! body.

use crate::expr::{
    emit_typed_feedback_register_site, i32_bool_to_nanbox, i32_to_nanbox, FnCtx,
    TypedFeedbackContract, TypedFeedbackKind,
};
use crate::nanbox::double_literal;
use crate::native_value::LoweredValue;
use crate::types::{DOUBLE, I1, I32, I64, I8};

use super::direct_method_guard::{
    class_chain_may_declare_method_field, emit_inline_direct_method_shape_guard, family_clone_word,
    family_method_word,
};

pub(super) const POINTER_TAG_HI16: &str = "32765"; // 0x7FFD
pub(super) const GC_TYPE_OBJECT: &str = "2";
// The first four bytes before an ObjectHeader are, in little-endian order,
// `gtype: u8`, `flags: u8`, and `reserved: u16`.  One masked i32 load can
// therefore prove the three fields the direct-method contract consumes:
//
//   gtype == GC_TYPE_OBJECT
//   flags & GC_FLAG_FORWARDED == 0
//   reserved & OBJ_FLAG_HAS_DESCRIPTORS == 0
//
// Mask: 0x0800_0000 (descriptor bit) | 0x0000_8000 (forwarded bit) |
// 0x0000_00ff (the complete gtype byte). The exact class/ShapeId comparison
// rejects the numeric-proof sibling.
pub(super) const GC_OBJECT_METHOD_GUARD_MASK_I32: &str = "134250751"; // 0x0800_80ff
                                                                      // The learned-word arms test only the kind and the forwarding bit. Every
                                                                      // descriptor install on a `GC_TYPE_OBJECT` changes its ShapeId (RULE 1,
                                                                      // `perry-runtime` `descriptor_state::note_descriptor_target_edits`), so the
                                                                      // exact word compare already subsumes the descriptor bit; the runtime only
                                                                      // learns a word whose key list lacks the method name, and an accessor or
                                                                      // attribute for that name would have to add the key.
pub(super) const LAYOUT_TAG_NEG_I32: &str = "-2147483648"; // subtract 0x8000_0000
pub(super) const ORDINARY_LAYOUT_RANGE_LEN: &str = "268435456"; // ordinary unshadowed layout band

/// A deliberately small constructive Boolean proof for method returns.
///
/// Source annotations are erased and therefore cannot license a native result.
/// These expression forms, however, produce a Boolean for every JavaScript
/// input. Keeping this proof local to the guarded direct-call site also means a
/// dynamic own/prototype override remains completely unconstrained.
fn expr_constructs_boolean(expr: &perry_hir::Expr) -> bool {
    use perry_hir::{Expr, UnaryOp};
    match expr {
        Expr::Bool(_)
        | Expr::Compare { .. }
        | Expr::BooleanCoerce(_)
        | Expr::IsFinite(_)
        | Expr::IsNaN(_)
        | Expr::NumberIsNaN(_)
        | Expr::NumberIsFinite(_)
        | Expr::NumberIsInteger(_)
        | Expr::IsUndefinedOrBareNan(_)
        | Expr::SetHas { .. }
        | Expr::SetDelete { .. }
        | Expr::MapHas { .. }
        | Expr::MapDelete { .. }
        | Expr::ArrayIncludes { .. } => true,
        Expr::Unary {
            op: UnaryOp::Not, ..
        } => true,
        Expr::Logical { left, right, .. } => {
            expr_constructs_boolean(left) && expr_constructs_boolean(right)
        }
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => expr_constructs_boolean(then_expr) && expr_constructs_boolean(else_expr),
        _ => false,
    }
}

/// `(all encountered returns are Boolean, every normal path exits)` for the
/// conservative straight-line/if subset used by hot predicate methods.
/// Unsupported control flow rejects the proof instead of trying to infer it.
fn block_constructively_returns_boolean(stmts: &[perry_hir::Stmt]) -> (bool, bool) {
    use perry_hir::Stmt;
    for stmt in stmts {
        match stmt {
            Stmt::Return(Some(expr)) => return (expr_constructs_boolean(expr), true),
            Stmt::Return(None) => return (false, true),
            Stmt::Throw(_) => return (true, true),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                let (then_ok, then_exits) = block_constructively_returns_boolean(then_branch);
                let (else_ok, else_exits) = else_branch
                    .as_deref()
                    .map(block_constructively_returns_boolean)
                    .unwrap_or((true, false));
                if !then_ok || !else_ok {
                    return (false, false);
                }
                if then_exits && else_exits {
                    return (true, true);
                }
            }
            // Neither form can hide a statement-level return.
            Stmt::Let { .. } | Stmt::Expr(_) => {}
            // Loops, try/finally, switches and labels need a real CFG proof.
            // Refuse them here; the runtime truthiness path remains exact.
            _ => return (false, false),
        }
    }
    (true, false)
}

pub(super) fn direct_method_constructively_returns_boolean(
    ctx: &FnCtx<'_>,
    direct_fn: &str,
) -> bool {
    ctx.classes.iter().any(|(class_name, class)| {
        class.methods.iter().any(|method| {
            !method.is_async
                && !method.is_generator
                && ctx
                    .methods
                    .get(&(class_name.to_string(), method.name.clone()))
                    .is_some_and(|name| name == direct_fn)
                && block_constructively_returns_boolean(&method.body) == (true, true)
        })
    })
}

pub(super) fn canonical_boolean_truthy(ctx: &mut FnCtx<'_>, value: &str) -> String {
    let bits = ctx.block().bitcast_double_to_i64(value);
    ctx.block().icmp_eq(I64, &bits, crate::nanbox::TAG_TRUE_I64)
}

#[derive(Clone, Copy)]
enum ConstructiveMethodTruthiness {
    CanonicalBoolean,
    RawNumber,
}

/// The representation-independent truthiness contract of a statically
/// resolved method body.
///
/// The Number case is intentionally narrower than the Boolean proof. It is
/// licensed only when the complete source body is the canonical ECS bitset
/// return. That expression either returns a Number or throws for every input;
/// selecting its native non-negative-index clone is a separate lowering
/// decision. Erased return annotations never participate in either proof.
fn constructive_method_truthiness(
    ctx: &FnCtx<'_>,
    direct_fn: &str,
) -> Option<ConstructiveMethodTruthiness> {
    if direct_method_constructively_returns_boolean(ctx, direct_fn) {
        return Some(ConstructiveMethodTruthiness::CanonicalBoolean);
    }
    ctx.classes.iter().find_map(|(class_name, class)| {
        class.methods.iter().find_map(|method| {
            let is_target = !method.is_async
                && !method.is_generator
                && ctx
                    .methods
                    .get(&(class_name.to_string(), method.name.clone()))
                    .is_some_and(|name| name == direct_fn);
            let [perry_hir::Stmt::Return(Some(expr))] = method.body.as_slice() else {
                return None;
            };
            (is_target && crate::expr::is_u32_bitset_test(expr))
                .then_some(ConstructiveMethodTruthiness::RawNumber)
        })
    })
}

fn constructive_truthy(
    ctx: &mut FnCtx<'_>,
    kind: ConstructiveMethodTruthiness,
    value: &str,
) -> String {
    match kind {
        ConstructiveMethodTruthiness::CanonicalBoolean => canonical_boolean_truthy(ctx, value),
        // `fcmp one` exactly matches Number truthiness: both signed zeroes and
        // NaN are false; every other finite or infinite Number is true.
        ConstructiveMethodTruthiness::RawNumber => ctx.block().fcmp("one", value, "0.0"),
    }
}

/// Publish a native truthiness result for a call site that has already proved
/// exact method identity (for example Phase 3b's containment route). Unlike
/// the guarded diamond, no arbitrary fallback arm exists here.
pub(super) fn publish_constructive_method_truthy(
    ctx: &mut FnCtx<'_>,
    direct_fn: &str,
    boxed: &str,
) {
    if let Some(kind) = ctx
        .truthy_call_result_requested
        .then(|| constructive_method_truthiness(ctx, direct_fn))
        .flatten()
    {
        let truthy = constructive_truthy(ctx, kind, boxed);
        ctx.pending_truthy_call_result = Some((boxed.to_string(), truthy));
    }
}

fn total_value_truthy(ctx: &mut FnCtx<'_>, value: &str) -> String {
    let raw = ctx.block().call(I32, "js_is_truthy", &[(DOUBLE, value)]);
    ctx.block().icmp_ne(I32, &raw, "0")
}

/// Emit the single-arm equivalent of `js_method_direct_shape_guard` directly
/// into the generated module. The guard remains dynamic at every call site:
/// arbitrary callback code may replace a prototype method or mutate the
/// receiver between loop iterations.
///
/// The first block proves that the value is a tagged heap pointer or the raw
/// object-address form used by internal method ABIs before any dereference.
/// The second block reproduces the runtime helper's production contract: the
/// receiver is a non-forwarded ordinary object without own descriptors,
/// and its exact `(class_id, ShapeId)` pair still matches the
/// compiler-published pair. Any failed proof takes the unchanged dynamic
/// method fallback.
/// Kill switch for the probe-before-runtime-guard emission
/// (`PERRY_METHOD_INLINE_PROBE=0` restores the guard-first form for A/B).
fn method_inline_probe_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_METHOD_INLINE_PROBE").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// Resolve the live receiver identity and admission predicate for packed
/// class selection, or the runtime probe's sanitized scalar field pair.
/// Admission declines on a non-pointer, an address
/// outside the heap band, a non-`GC_TYPE_OBJECT` / forwarded /
/// descriptor-bearing / packed-numeric-proof header, a zero class id
/// or a zero ShapeId. Packed consumers must branch on the predicate before
/// selecting a body; the legacy word wrapper still returns zero on decline.
///
/// The header word and the packed `class_id | ShapeId << 32` load are the
/// ones [`emit_inline_direct_method_shape_guard`] already uses for the
/// single-arm form; this is the same sequence without the comparison baked
/// in. On the wolf-ecs cycle the runtime probe was 2.2–2.6% of self time on
/// call, address classification and flag resolution for a few loads.
fn emit_direct_method_probe(ctx: &mut FnCtx<'_>, recv_box: &str, packed: bool) -> (String, String) {
    let deref_idx = ctx.new_block("method_probe.deref");
    let read_idx = ctx.new_block("method_probe.read");
    let merge_idx = ctx.new_block("method_probe.merge");
    let deref_label = ctx.block_label(deref_idx);
    let read_label = ctx.block_label(read_idx);
    let merge_label = ctx.block_label(merge_idx);
    let floor = crate::target_layout::heap_addr_lower_bound_inclusive(ctx.target_triple);
    let ceiling = crate::target_layout::heap_addr_upper_bound_exclusive(ctx.target_triple);
    let check_end = {
        let blk = ctx.block();
        let recv_bits = blk.bitcast_double_to_i64(recv_box);
        let recv_handle = blk.and(I64, &recv_bits, crate::nanbox::POINTER_MASK_I64);
        let tag = blk.lshr(I64, &recv_bits, "48");
        let is_tagged_ptr = blk.icmp_eq(I64, &tag, POINTER_TAG_HI16);
        // `normalize_raw_object_addr` also accepts the top-word-zero raw
        // address form internal method ABIs carry.
        let is_raw_ptr = blk.icmp_eq(I64, &tag, "0");
        let is_ptr = blk.or(I1, &is_tagged_ptr, &is_raw_ptr);
        let in_heap_range = if packed {
            let relative = blk.sub(I64, &recv_handle, &floor.to_string());
            blk.icmp_ult(I64, &relative, &(ceiling - floor).to_string())
        } else {
            let above = blk.icmp_uge(I64, &recv_handle, &floor.to_string());
            let below = blk.icmp_ult(I64, &recv_handle, &ceiling.to_string());
            blk.and(I1, &above, &below)
        };
        let ptr_safe = blk.and(I1, &is_ptr, &in_heap_range);
        let can_deref = ptr_safe;
        blk.cond_br(&can_deref, &deref_label, &merge_label);
        blk.label.clone()
    };
    ctx.current_block = deref_idx;
    let deref_end = {
        let blk = ctx.block();
        let recv_bits = blk.bitcast_double_to_i64(recv_box);
        let recv_handle = blk.and(I64, &recv_bits, crate::nanbox::POINTER_MASK_I64);
        let obj_ptr = blk.inttoptr(I64, &recv_handle);
        let gc_header_ptr = blk.gep(I8, &obj_ptr, &[(I64, "-8")]);
        let gc_header = blk.load(I32, &gc_header_ptr);
        let guarded_gc_bits = blk.and(I32, &gc_header, GC_OBJECT_METHOD_GUARD_MASK_I32);
        let gc_header_ok = blk.icmp_eq(I32, &guarded_gc_bits, GC_TYPE_OBJECT);
        blk.cond_br(&gc_header_ok, &read_label, &merge_label);
        blk.label.clone()
    };
    ctx.current_block = read_idx;
    let (first, second, read_end) = {
        let blk = ctx.block();
        let recv_bits = blk.bitcast_double_to_i64(recv_box);
        let recv_handle = blk.and(I64, &recv_bits, crate::nanbox::POINTER_MASK_I64);
        let obj_ptr = blk.inttoptr(I64, &recv_handle);
        let (first, second) = if packed {
            // The method site's authoritative class_id | ShapeId << 32 encoding.
            let word = blk.load(I64, &obj_ptr);
            let cid = blk.trunc(I64, &word, I32);
            let high = blk.lshr(I64, &word, "32");
            let shape = blk.trunc(I64, &high, I32);
            let cid_nonzero = blk.icmp_ne(I32, &cid, "0");
            let shape_nonzero = blk.icmp_ne(I32, &shape, "0");
            // Linked/null ShapeIds encode a live [[Prototype]] edge that
            // class-id/ShapeId dispatch does not model. Only plain identity
            // shapes may enter the direct class-method tower.
            let shape_kind = blk.and(I32, &shape, "3145728"); // 3 << 20
            let plain_shape = blk.icmp_eq(I32, &shape_kind, "0");
            let both = blk.and(I1, &cid_nonzero, &shape_nonzero);
            let both = blk.and(I1, &both, &plain_shape);
            (word, both)
        } else {
            // Preserve the existing scalar emission for callers needing both fields.
            let cid = blk.load(I32, &obj_ptr);
            let shape_ptr = blk.gep(I8, &obj_ptr, &[(I64, "4")]);
            let shape = blk.load(I32, &shape_ptr);
            let cid_nonzero = blk.icmp_ne(I32, &cid, "0");
            let shape_nonzero = blk.icmp_ne(I32, &shape, "0");
            // Match the packed probe above: only plain ShapeIds are safe for
            // direct class dispatch; linked and null shapes must stay dynamic.
            let shape_kind = blk.and(I32, &shape, "3145728"); // 3 << 20
            let plain_shape = blk.icmp_eq(I32, &shape_kind, "0");
            let both = blk.and(I1, &cid_nonzero, &shape_nonzero);
            let both = blk.and(I1, &both, &plain_shape);
            (
                blk.select(I1, &both, I32, &cid, "0"),
                blk.select(I1, &both, I32, &shape, "0"),
            )
        };
        blk.br(&merge_label);
        (first, second, blk.label.clone())
    };
    ctx.current_block = merge_idx;
    let blk = ctx.block();
    let first = blk.phi(
        if packed { I64 } else { I32 },
        &[("0", &check_end), ("0", &deref_end), (&first, &read_end)],
    );
    let second = blk.phi(
        if packed { I1 } else { I32 },
        &[("0", &check_end), ("0", &deref_end), (&second, &read_end)],
    );
    (first, second)
}

pub(crate) fn emit_inline_direct_method_word_probe(ctx: &mut FnCtx<'_>, recv_box: &str) -> String {
    let (word, admitted) = emit_direct_method_probe(ctx, recv_box, true);
    ctx.block().select(I1, &admitted, I64, &word, "0")
}

/// Keep the packed identity and its admission predicate separate until the
/// caller branches into class selection. Selecting a zero word first forces
/// the backend to materialize conditional moves for each field predicate,
/// only to immediately switch on that word's class id. No receiver checks
/// are omitted: a false predicate selects the ordinary dynamic lookup.
pub(crate) fn emit_direct_method_selection_probe(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
) -> (String, String) {
    emit_direct_method_probe(ctx, recv_box, true)
}

pub(crate) fn emit_inline_direct_method_shape_probe(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
) -> (String, String) {
    emit_direct_method_probe(ctx, recv_box, false)
}

/// Emit the exact ordinary-object `(class_id, ShapeId)` guard used by a
/// `$pshape_args` route.  Unlike the method-receiver guard above this does not
/// consult prototype-method invalidation state: it proves field offsets only.
/// Descriptor-bearing, forwarded, proxy, subclass, wrong-class, and mutated-
/// shape values all take `fallback_label` before any clone field access.
fn emit_inline_exact_argument_shape_guard(
    ctx: &mut FnCtx<'_>,
    value: &str,
    non_alias_values: &[String],
    expected_class_id: u32,
    expected_shape_id: &str,
    fast_label: &str,
    fallback_label: &str,
) {
    let deref_idx = ctx.new_block("pshape_arg.guard_deref");
    let deref_label = ctx.block_label(deref_idx);
    let heap_floor =
        crate::target_layout::heap_addr_lower_bound_inclusive(ctx.target_triple).to_string();
    let heap_ceiling =
        crate::target_layout::heap_addr_upper_bound_exclusive(ctx.target_triple).to_string();

    {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(value);
        let handle = blk.and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
        let tag = blk.lshr(I64, &bits, "48");
        let tagged = blk.icmp_eq(I64, &tag, POINTER_TAG_HI16);
        let above_floor = blk.icmp_uge(I64, &handle, &heap_floor);
        let below_ceiling = blk.icmp_ult(I64, &handle, &heap_ceiling);
        let in_heap = blk.and(I1, &above_floor, &below_ceiling);
        let mut safe_to_deref = blk.and(I1, &tagged, &in_heap);
        for other in non_alias_values {
            let other_bits = blk.bitcast_double_to_i64(other);
            let distinct = blk.icmp_ne(I64, &bits, &other_bits);
            safe_to_deref = blk.and(I1, &safe_to_deref, &distinct);
        }
        blk.cond_br(&safe_to_deref, &deref_label, fallback_label);
    }

    ctx.current_block = deref_idx;
    {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(value);
        let handle = blk.and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
        let obj_ptr = blk.inttoptr(I64, &handle);
        let gc_header_ptr = blk.gep(I8, &obj_ptr, &[(I64, "-8")]);
        let gc_header = blk.load(I32, &gc_header_ptr);
        let guarded_gc_bits = blk.and(I32, &gc_header, GC_OBJECT_METHOD_GUARD_MASK_I32);
        let gc_header_ok = blk.icmp_eq(I32, &guarded_gc_bits, GC_TYPE_OBJECT);

        let class_shape = blk.load(I64, &obj_ptr);
        let expected_shape_i64 = blk.zext(I32, expected_shape_id, I64);
        let expected_shape_high = blk.shl(I64, &expected_shape_i64, "32");
        let expected_class_shape =
            blk.or(I64, &expected_shape_high, &expected_class_id.to_string());
        let class_shape_ok = crate::typed_shape::emit_compatible_class_shape_eq(
            blk,
            &class_shape,
            &expected_class_id.to_string(),
            expected_shape_id,
            &expected_class_shape,
            &[],
        );
        let shape_id_rel = blk.add(I32, expected_shape_id, LAYOUT_TAG_NEG_I32);
        let shape_valid = blk.icmp_ult(I32, &shape_id_rel, ORDINARY_LAYOUT_RANGE_LEN);
        let pass = blk.and(I1, &gc_header_ok, &class_shape_ok);
        let pass = blk.and(I1, &pass, &shape_valid);
        blk.cond_br(&pass, fast_label, fallback_label);
    }
}

/// Route a receiver-proven method call through its exact-shape argument clone.
/// `generic_fn` is the already-selected receiver-safe body for guard failure.
pub(super) fn emit_pshape_argument_dispatch(
    ctx: &mut FnCtx<'_>,
    receiver_class_name: &str,
    property: &str,
    direct_fn: &str,
    generic_fn: &str,
    direct_arg_slices: &[(crate::types::LlvmType, &str)],
    source_args: &[perry_hir::Expr],
) -> Option<String> {
    let key = (receiver_class_name.to_string(), property.to_string());
    let plan = ctx.pshape_arg_methods.get(&key)?.clone();
    let clone_fn = crate::collectors::pshape_args_method_name(direct_fn);

    let mut routed = Vec::with_capacity(plan.args.len());
    for arg in &plan.args {
        let direct_index = arg.param_index + 1;
        let source_arg = source_args.get(arg.param_index)?;
        let (caller_fact, requires_runtime_guard) =
            ctx.ptr_shape_argument_route_fact(source_arg)?;
        if caller_fact.class_name != arg.fact.class_name {
            return None;
        }
        let value = direct_arg_slices.get(direct_index)?.1.to_string();
        let non_alias_values: Vec<String> = direct_arg_slices
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != direct_index)
            .map(|(_, (_, other))| (*other).to_string())
            .collect();
        routed.push((arg.clone(), value, non_alias_values, requires_runtime_guard));
    }
    if routed.is_empty() {
        return None;
    }

    // A native fresh-local fact proves exact allocation class, unchanged
    // shape, and non-aliasing up to this call. Re-reading tag/range/GC header,
    // class, ShapeId, and every formal-alias comparison is redundant and was
    // slower than the one field IC the clone removes in perform-ecs. Keep the
    // guarded path below for forwarded clone parameters, whose fact originates
    // at a dynamic caller boundary.
    if routed.iter().all(|route| !route.3) {
        let result = ctx.block().call(DOUBLE, &clone_fn, direct_arg_slices);
        let mut notes = vec![
            format!("argument_clone={clone_fn}"),
            format!("generic_method={generic_fn}"),
            format!("receiver_class={receiver_class_name}"),
            format!("method={property}"),
            "argument_abi=tagged_js_value_shadow_rooted".to_string(),
            "argument_guard=elided_by_fresh_provenance_and_containment".to_string(),
            "wrong_shape_route=generic_method_before_clone_selection".to_string(),
        ];
        for (arg, _, _, _) in &routed {
            notes.push(format!("argument_index={}", arg.param_index));
            notes.push(format!("argument_class={}", arg.fact.class_name));
            notes.push("argument_alias_proof=caller_containment".to_string());
            notes.push("argument_provenance=fresh_exact_class".to_string());
        }
        ctx.record_lowered_value(
            "MethodCall",
            None,
            "proven_shape_argument_method_call",
            &LoweredValue::js_value(result.clone()),
            None,
            None,
            None,
            false,
            false,
            notes,
        );
        return Some(result);
    }

    let mut guarded = Vec::with_capacity(routed.len());
    for (arg, value, non_alias_values, requires_runtime_guard) in routed {
        let class_id = *ctx.class_ids.get(&arg.fact.class_name)?;
        let keys_global = ctx.class_keys_globals.get(&arg.fact.class_name)?.clone();
        let shape_id =
            crate::typed_shape::class_shape_id_operand(ctx, &arg.fact.class_name, &keys_global);
        guarded.push((
            arg,
            value,
            non_alias_values,
            class_id,
            shape_id,
            requires_runtime_guard,
        ));
    }

    let fast_idx = ctx.new_block("pshape_arg.fast");
    let fallback_idx = ctx.new_block("pshape_arg.fallback");
    let merge_idx = ctx.new_block("pshape_arg.merge");
    let intermediate_idxs: Vec<usize> = (1..guarded.len())
        .map(|_| ctx.new_block("pshape_arg.guard_next"))
        .collect();
    let fast_label = ctx.block_label(fast_idx);
    let fallback_label = ctx.block_label(fallback_idx);
    let merge_label = ctx.block_label(merge_idx);

    for (index, (_, value, non_alias_values, class_id, shape_id, _)) in guarded.iter().enumerate() {
        let pass_label = intermediate_idxs
            .get(index)
            .map(|block| ctx.block_label(*block))
            .unwrap_or_else(|| fast_label.clone());
        emit_inline_exact_argument_shape_guard(
            ctx,
            value,
            non_alias_values,
            *class_id,
            shape_id,
            &pass_label,
            &fallback_label,
        );
        if let Some(next) = intermediate_idxs.get(index) {
            ctx.current_block = *next;
        }
    }

    ctx.current_block = fast_idx;
    let fast_value = ctx.block().call(DOUBLE, &clone_fn, direct_arg_slices);
    let fast_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = fallback_idx;
    let fallback_value = ctx.block().call(DOUBLE, generic_fn, direct_arg_slices);
    let fallback_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    let merged = ctx.block().phi(
        DOUBLE,
        &[
            (fast_value.as_str(), fast_end.as_str()),
            (fallback_value.as_str(), fallback_end.as_str()),
        ],
    );
    let mut notes = vec![
        format!("argument_clone={clone_fn}"),
        format!("generic_method={generic_fn}"),
        format!("receiver_class={receiver_class_name}"),
        format!("method={property}"),
        "argument_abi=tagged_js_value_shadow_rooted".to_string(),
        "guard_failure_fallback=generic_method".to_string(),
    ];
    for (arg, _, _, _, _, _) in &guarded {
        notes.push(format!("argument_index={}", arg.param_index));
        notes.push(format!("argument_class={}", arg.fact.class_name));
        notes.push("argument_guard=exact_class_and_shape".to_string());
        notes.push("argument_alias_guard=receiver_and_formals_distinct".to_string());
        notes.push("argument_provenance=caller_containment_plus_runtime_guard".to_string());
    }
    ctx.record_lowered_value(
        "MethodCall",
        None,
        "proven_shape_argument_method_call",
        &LoweredValue::js_value(merged.clone()),
        None,
        None,
        None,
        false,
        false,
        notes,
    );
    Some(merged)
}

#[cfg(test)]
mod packed_guard_tests {
    use super::*;

    /// Anti-drift gate for the runtime fields packed into the i32 load at
    /// `obj - 8`. Keep this alongside the emitter so a flag move changes a
    /// failing test instead of silently weakening a generated guard.
    #[test]
    fn gc_header_mask_preserves_the_direct_method_contract() {
        let obj_type_mask = 0x0000_00ffu32;
        let forwarded = u32::from(0x80u8) << 8;
        let has_descriptors = 0x0800u32 << 16;
        let mask = obj_type_mask | forwarded | has_descriptors;
        let expected = u32::from(2u8);

        assert_eq!(GC_OBJECT_METHOD_GUARD_MASK_I32, mask.to_string());
        assert_eq!(expected & mask, expected);
        assert_ne!((expected | forwarded) & mask, expected);
        assert_ne!((expected | has_descriptors) & mask, expected);
        assert_ne!((expected ^ 1) & mask, expected);
    }

    /// `ObjectHeader::{class_id,parent_class_id}` are adjacent u32 fields.
    /// Perry's supported native targets are little-endian, so one i64 load
    /// sees class in the low word and ShapeId in the high word.
    #[test]
    fn class_shape_word_uses_the_supported_little_endian_layout() {
        let class_id = 0x1234_5678u32;
        let shape_id = 0x89ab_cdefu32;
        let mut bytes = [0u8; 8];
        bytes[..4].copy_from_slice(&class_id.to_le_bytes());
        bytes[4..].copy_from_slice(&shape_id.to_le_bytes());
        assert_eq!(
            u64::from_le_bytes(bytes),
            (u64::from(shape_id) << 32) | u64::from(class_id)
        );

        for triple in [
            "aarch64-apple-darwin",
            "x86_64-unknown-linux-gnu",
            "aarch64-linux-android",
            "x86_64-pc-windows-msvc",
        ] {
            assert!(!triple.starts_with("s390") && !triple.starts_with("powerpc64-"));
        }
    }
}

fn typed_i1_method_signature_note(reps: &[crate::codegen::TypedParamRep]) -> String {
    let first = reps.first().map(|rep| rep.label()).unwrap_or("void");
    if reps.len() <= 1 {
        format!("typed_signature=i1({first})->i1")
    } else {
        format!("typed_signature=i1({first}, ...)->i1")
    }
}

fn typed_method_signature_note(ret: &str, reps: &[crate::codegen::TypedParamRep]) -> String {
    let first = reps.first().map(|rep| rep.label()).unwrap_or("void");
    if reps.len() <= 1 {
        format!("typed_signature={ret}({first})->{ret}")
    } else {
        format!("typed_signature={ret}({first}, ...)->{ret}")
    }
}

/// Call an own-property method override found by `perry_probe_own_method`:
/// spill the flat user arguments into a fresh buffer and call the value with
/// the receiver as `this` through `js_native_call_value`.
///
/// The override may be an arrow / `.bind(...)`-bound function whose `this` is
/// captured/bound, but it can also be a regular function assigned via
/// `this.method = fn` or `class X { method = fn; }` (hono's RegExpRouter uses
/// this exact shape: `match = match;` assigns the imported standalone `match`
/// function as an instance own-property; its body reads
/// `this.buildAllMatchers()`). Passing the receiver as `this` lets non-arrow
/// function bodies see the right `this` (issue #632 / #519 pattern).
pub(super) fn emit_own_override_value_call(
    ctx: &mut FnCtx<'_>,
    own_method: &str,
    this_box: &str,
    override_user_args: &[String],
) -> String {
    let user_arg_count = override_user_args.len();
    let (args_ptr, args_len) = if user_arg_count == 0 {
        ("null".to_string(), "0".to_string())
    } else {
        let buf_reg = ctx.func.alloca_entry_array(DOUBLE, user_arg_count);
        for (i, a_val) in override_user_args.iter().enumerate() {
            let slot = ctx
                .block()
                .gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
            ctx.block().store(DOUBLE, a_val, &slot);
        }
        let ptr_reg = ctx.block().next_reg();
        ctx.block().emit_raw(format!(
            "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
            ptr_reg, user_arg_count, buf_reg
        ));
        (ptr_reg, user_arg_count.to_string())
    };
    let recv_for_this = if this_box.is_empty() {
        double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
    } else {
        this_box.to_string()
    };
    let this_bits = ctx.block().bitcast_double_to_i64(&recv_for_this);
    ctx.block().call(
        DOUBLE,
        "js_native_call_value",
        &[
            (DOUBLE, own_method),
            (I64, &this_bits),
            (crate::types::PTR, &args_ptr),
            (I64, &args_len),
        ],
    )
}

/// One additional `(class id, keys token) -> concrete method` arm for the
/// shape-guarded direct call, describing a class in the DECLARED receiver
/// class's subclass closure.
///
/// The declared-class guard speculates that the receiver's dynamic class is
/// exactly its static class. For a receiver typed as the base of a hierarchy —
/// `nodes: Node2D[]`, every element a `Rect` / `Circle` / `Square` / `Marker` /
/// `Group` — that speculation is wrong for EVERY element, so the guard misses
/// 100% of the time and each call pays a wasted guard plus the full
/// `js_native_call_method` dispatch tower. Each arm here is the same proof the
/// declared-class guard performs (exact class id + exact keys token), applied
/// to one more class whose implementation of the method codegen already
/// resolved statically.
pub(super) struct SubclassDispatchArm {
    /// `class_id` of the concrete subclass this arm matches.
    pub class_id: u32,
    /// Name of the module global holding that subclass's canonical keys array.
    pub keys_global: String,
    /// The method body `property` resolves to when walked from that subclass.
    pub target_fn: String,
}

/// Emit a typed-feedback runtime guard before a known class-method direct call.
///
/// The guard validates that the receiver still has the expected class shape,
/// has no own-property method replacement, and still resolves the method name
/// to the direct function pointer in the runtime vtable. Failures branch to the
/// existing dynamic method dispatcher and record a fallback once.
pub(super) struct ConstantMethodSelection<'a> {
    pub selected: &'a str,
    /// Reuse the lookup word only across order-free argument evaluation.
    pub receiver_word: Option<&'a str>,
    pub lookup: Option<(&'a crate::expr::method_site::SiteLookup, &'a str)>,
    pub call_byte_offset: u32,
}

pub(super) fn emit_guarded_direct_method_call(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
    receiver_class_name: &str,
    property: &str,
    direct_fn: &str,
    direct_call_fn: Option<&str>,
    direct_arg_slices: &[(crate::types::LlvmType, &str)],
    source_args: &[perry_hir::Expr],
    fallback_user_args: &[String],
    nonnegative_index_direct_fn: Option<&str>,
    typed_direct_fn: Option<(&str, Vec<crate::codegen::TypedParamRep>)>,
    typed_f64_receiver_direct_fn: Option<(&str, usize, &crate::codegen::TypedReceiverMethodInfo)>,
    typed_i32_direct_fn: Option<(&str, Vec<crate::codegen::TypedParamRep>)>,
    typed_i1_direct_fn: Option<(&str, Vec<crate::codegen::TypedParamRep>)>,
    typed_string_direct_fn: Option<(&str, Vec<crate::codegen::TypedParamRep>)>,
    shape_only_guard: bool,
    subclass_arms: &[SubclassDispatchArm],
    certificate: Option<&ConstantMethodSelection<'_>>,
) -> Option<String> {
    // `class C { m = fn; m() {} }` and an inherited field with the same name
    // both require ordinary own-property lookup. Falling back here reaches
    // the own-property probe / dynamic dispatch in the caller.
    if class_chain_may_declare_method_field(ctx, receiver_class_name, property) {
        return None;
    }

    let truthy_result_kind = ctx
        .truthy_call_result_requested
        .then(|| constructive_method_truthiness(ctx, direct_fn))
        .flatten();
    let expected_class_id = *ctx.class_ids.get(receiver_class_name)?;
    let keys_global_name = ctx.class_keys_globals.get(receiver_class_name)?.clone();
    // Only the shape-only guard is widened. The typed-feedback guard records an
    // observation keyed to ONE (class, method, func ptr) contract per site; a
    // multi-class site would feed it a stream of "different class" observations
    // and it would (correctly) mark the site polymorphic. That form keeps its
    // single-arm shape.
    let subclass_arms: &[SubclassDispatchArm] = if shape_only_guard && certificate.is_some() {
        subclass_arms
    } else {
        &[]
    };

    // A certificate selects method identity; the exact canonical word
    // separately licenses the existing declaring-class layout clone.
    // Static methods use the class operation and have no such clone.
    let pshape_fn: Option<String> = (direct_call_fn.is_none()
        && !direct_fn.starts_with("perry_static_")
        && ctx
            .pshape_methods
            .contains_key(&(receiver_class_name.to_string(), property.to_string())))
    .then(|| crate::collectors::pshape_method_name(direct_fn));
    let pshape_index_fn = nonnegative_index_direct_fn.and_then(|index_fn| {
        let pshape = pshape_fn.as_ref()?;
        let index_suffix = index_fn.strip_prefix(direct_fn)?;
        Some(format!("{pshape}{index_suffix}"))
    });

    // The body a failed typed guard falls back to. Arm-invariant (both inputs
    // are), so it is resolved once here rather than five times below.
    let generic_body_fn: String = pshape_fn
        .clone()
        .or_else(|| direct_call_fn.map(str::to_string))
        .unwrap_or_else(|| crate::codegen::generic_method_body_name(direct_fn));

    let expected_class_id_str = expected_class_id.to_string();
    let expected_shape_id =
        crate::typed_shape::class_shape_id_operand(ctx, receiver_class_name, &keys_global_name);

    let key_idx = ctx.strings.intern(property);
    let entry = ctx.strings.entry(key_idx);
    let bytes_global = format!("@{}", entry.bytes_global);
    let name_len_str = entry.byte_len.to_string();
    let dispatch_global = ctx.strings.static_dispatch_global(key_idx);
    let site_id = if shape_only_guard {
        None
    } else {
        Some(emit_typed_feedback_register_site(
            ctx,
            TypedFeedbackKind::MethodCall,
            property,
            TypedFeedbackContract::method_direct_call(),
        ))
    };

    // Per-arm ShapeIds, loaded through entry-block scalar slots.
    let subclass_shape_ids: Vec<String> = subclass_arms
        .iter()
        .map(|arm| {
            if let Some(id) = crate::typed_shape::static_class_shape_id(&arm.keys_global) {
                return id.to_string();
            }
            let shape_global =
                crate::typed_shape::shape_id_global_name_from_keys_global(&arm.keys_global);
            let slot = ctx.func.entry_init_load_global(&shape_global, I32);
            ctx.block().load(I32, &slot)
        })
        .collect();

    let guard_idx = ctx.new_block("method_direct.guard");
    let fast_idx = ctx.new_block("method_direct.fast");
    let sub_case_idxs: Vec<usize> = (0..subclass_arms.len())
        .map(|i| ctx.new_block(&format!("method_direct.sub{i}")))
        .collect();
    let fallback_idx = ctx.new_block("method_direct.fallback");
    let merge_idx = ctx.new_block("method_direct.merge");
    let guard_label = ctx.block_label(guard_idx);
    let fast_label = ctx.block_label(fast_idx);
    let fallback_label = ctx.block_label(fallback_idx);
    let merge_label = ctx.block_label(merge_idx);
    let sub_case_labels: Vec<String> = sub_case_idxs.iter().map(|&i| ctx.block_label(i)).collect();
    ctx.block().br(&guard_label);

    ctx.current_block = guard_idx;
    // The certificate chooses the body; the canonical word admits its layout.
    let multi_arm = !subclass_arms.is_empty();
    let inline_single_arm = shape_only_guard && !multi_arm;
    // Feedback builds retain their observation guard.
    let probe_before_runtime_guard = !shape_only_guard
        && !multi_arm
        && !crate::expr::typed_feedback_emission_enabled()
        && method_inline_probe_enabled();
    // Legacy feedback sites keep their learned arm. Constant-selected sites
    // never allocate this separate memo.
    let learned_joins_fast_arm = typed_f64_receiver_direct_fn.is_none()
        && pshape_fn.is_none()
        && (typed_direct_fn.is_some()
            || typed_i32_direct_fn.is_some()
            || typed_i1_direct_fn.is_some()
            || typed_string_direct_fn.is_some());
    let mut layout_misses = Vec::new();
    let holder_ok = if multi_arm || certificate.is_some() {
        "true".to_string()
    } else {
        crate::proto_constants::holder_constant(ctx, receiver_class_name, property)
    };
    if let Some(cert) = certificate {
        // Method identity was selected before the arguments. The exact
        // canonical word admits the existing layout clone after them.
        let word = cert
            .receiver_word
            .map(str::to_string)
            .unwrap_or_else(|| emit_inline_direct_method_word_probe(ctx, recv_box));
        let mut targets = vec![(
            expected_class_id_str.clone(),
            expected_shape_id.clone(),
            fast_label.clone(),
            direct_fn.to_string(),
        )];
        targets.extend(subclass_arms.iter().enumerate().map(|(i, a)| {
            (
                a.class_id.to_string(),
                subclass_shape_ids[i].clone(),
                sub_case_labels[i].clone(),
                a.target_fn.clone(),
            )
        }));
        let tests: Vec<_> = (0..targets.len())
            .map(|_| ctx.new_block("method_direct.layout"))
            .collect();
        let mut switch = format!("switch i32 {}, label %{fallback_label} [", cert.selected);
        for (i, &test) in tests.iter().enumerate() {
            switch.push_str(&format!(" i32 {}, label %{}", i + 1, ctx.block_label(test)));
        }
        switch.push_str(" ]");
        ctx.block().emit_raw(switch);
        ctx.block().mark_terminated();
        let miss_joins_fast = learned_joins_fast_arm
            || (pshape_fn.is_none()
                && typed_f64_receiver_direct_fn.is_none()
                && !ctx
                    .pshape_arg_methods
                    .contains_key(&(receiver_class_name.to_string(), property.to_string())));
        for (i, (cid, shape, fast, public)) in targets.iter().enumerate() {
            ctx.current_block = tests[i];
            let ordinary = ctx.new_block("method_direct.layout_miss");
            let ordinary_l = ctx.block_label(ordinary);
            // Case 2c: a compiled `new C` births at C's family layout. Where
            // that layout is the birth pair's instance, its exact word enters
            // the same arm (the `$pshape` clone), not the ordinary body. The
            // typed-f64 receiver arm keeps its pair-only field proof.
            let family = if i == 0 && (miss_joins_fast || typed_f64_receiver_direct_fn.is_some()) {
                None
            } else {
                let class_id = if i == 0 {
                    expected_class_id
                } else {
                    subclass_arms[i - 1].class_id
                };
                let class_name = if i == 0 {
                    Some(receiver_class_name.to_string())
                } else {
                    ctx.class_ids
                        .iter()
                        .filter(|(_, &c)| c == class_id)
                        .map(|(n, _)| n.clone())
                        .min()
                };
                class_name.and_then(|n| family_clone_word(ctx, &n, class_id, property, public))
            };
            let blk = ctx.block();
            // The family word first: compiled births carry it.
            let family_ok = family.map(|family| blk.icmp_eq(I64, &word, &family));
            let high = blk.zext(I32, shape, I64);
            let high = blk.shl(I64, &high, "32");
            let expected = blk.or(I64, &high, cid);
            let pass = crate::typed_shape::emit_compatible_class_shape_eq(
                blk,
                &word,
                cid,
                shape,
                &expected,
                &[],
            );
            let pass = match family_ok {
                Some(family_ok) => blk.or(I1, &family_ok, &pass),
                None => pass,
            };
            blk.cond_br(&pass, fast, &ordinary_l);
            ctx.current_block = ordinary;
            if i == 0 && miss_joins_fast {
                // With no receiver-layout capability, both edges share the
                // already selected ordinary/argument-only body.
                ctx.block().br(&fast_label);
                continue;
            }
            // Layout failure cannot re-read the method after arguments.
            // Keep the selected ordinary body shared on this cold edge.
            let value = ctx.block().call_noinline(DOUBLE, public, direct_arg_slices);
            let truthy = truthy_result_kind.map(|_| total_value_truthy(ctx, &value));
            let end = ctx.block().label.clone();
            ctx.block().br(&merge_label);
            layout_misses.push((value, truthy, end));
        }
        ctx.current_block = guard_idx;
    }
    // Object model case 2c: a compiled `new C` births at C's family layout,
    // so its instances carry the family layout id, never the birth pair
    // above. Their arm enters the fast arm when the family holds the birth
    // pair's instance (`family_holds_birth_layout`), else it calls the
    // ordinary body, preserving any argument-only direct-call ABI.
    let family_arm = if (inline_single_arm && certificate.is_none()) || probe_before_runtime_guard {
        family_method_word(
            ctx,
            receiver_class_name,
            expected_class_id,
            property,
            direct_fn,
        )
    } else {
        None
    };
    // A family that holds the birth pair's instance enters the fast arm
    // itself (`family_holds_birth_layout`); the typed-f64 receiver arm keeps
    // its pair-only field proof.
    let family_enters_fast = family_arm.is_some()
        && typed_f64_receiver_direct_fn.is_none()
        && super::direct_method_guard::family_holds_birth_layout(
            ctx,
            receiver_class_name,
            expected_class_id,
        );
    let family_label = family_arm
        .as_ref()
        .filter(|_| !family_enters_fast)
        .map(|_| {
            let idx = ctx.new_block("method_direct.family_call");
            (idx, ctx.block_label(idx))
        });
    let family_guard = family_arm.as_deref().zip(if family_enters_fast {
        Some(fast_label.as_str())
    } else {
        family_label.as_ref().map(|(_, l)| l.as_str())
    });
    if inline_single_arm && certificate.is_none() {
        emit_inline_direct_method_shape_guard(
            ctx,
            recv_box,
            &expected_class_id_str,
            &expected_shape_id,
            &holder_ok,
            &fast_label,
            &fallback_label,
            true,
            family_guard,
        );
    }
    if probe_before_runtime_guard {
        let runtime_guard_idx = ctx.new_block("method_direct.runtime_guard");
        let runtime_guard_label = ctx.block_label(runtime_guard_idx);
        emit_inline_direct_method_shape_guard(
            ctx,
            recv_box,
            &expected_class_id_str,
            &expected_shape_id,
            &holder_ok,
            &fast_label,
            &runtime_guard_label,
            false,
            family_guard,
        );
        ctx.current_block = runtime_guard_idx;
    }
    if let Some((family_idx, _)) = family_label {
        let resume = ctx.current_block;
        ctx.current_block = family_idx;
        // This arm proves method identity but not the birth-field layout.
        // An arguments-length clone needs no field-layout proof, and its
        // scalar count cannot be passed to the public arguments-object ABI.
        let target = direct_call_fn.unwrap_or(direct_fn);
        let value = ctx.block().call(DOUBLE, target, direct_arg_slices);
        let truthy = truthy_result_kind.map(|_| total_value_truthy(ctx, &value));
        let end = ctx.block().label.clone();
        ctx.block().br(&merge_label);
        layout_misses.push((value, truthy, end));
        ctx.current_block = resume;
    }
    let guard_ok = if certificate.is_some() || multi_arm || inline_single_arm {
        // The chain above already terminated the guard block and every test
        // block, or the inline single-arm guard terminated both its pointer
        // gate and header block; `fast_idx` / `fallback_idx` are entered from
        // either form unchanged.
        String::new()
    } else if shape_only_guard {
        ctx.block().call(
            I32,
            "js_method_direct_shape_guard",
            &[
                (DOUBLE, recv_box),
                (I32, &expected_class_id_str),
                (I32, &expected_shape_id),
            ],
        )
    } else {
        ctx.block().call(
            I32,
            "js_typed_feedback_method_direct_call_guard",
            &[
                (
                    I64,
                    site_id.as_deref().expect("typed-feedback method site id"),
                ),
                (DOUBLE, recv_box),
                (I32, &expected_class_id_str),
                (I32, &expected_shape_id),
                (crate::types::PTR, &bytes_global),
                (I64, &name_len_str),
                (crate::types::PTR, &format!("@{}", direct_fn)),
            ],
        )
    };
    if certificate.is_none() && !multi_arm && !inline_single_arm {
        let guard_pass = ctx.block().icmp_ne(I32, &guard_ok, "0");
        let guard_pass = ctx.block().and(I1, &guard_pass, &holder_ok);
        ctx.block()
            .cond_br(&guard_pass, &fast_label, &fallback_label);
    }

    ctx.current_block = fast_idx;
    let fast_value = {
        if let Some((typed_fn, typed_formal_count, receiver_info)) = typed_f64_receiver_direct_fn {
            let formal_args: Vec<&str> = direct_arg_slices
                .iter()
                .skip(1)
                .take(typed_formal_count)
                .map(|(_, value)| *value)
                .collect();
            let mut guard: Option<String> = None;
            for value in &formal_args {
                let raw = ctx
                    .block()
                    .call(I32, "js_typed_f64_arg_guard", &[(DOUBLE, *value)]);
                let ok = ctx.block().icmp_ne(I32, &raw, "0");
                guard = Some(match guard {
                    Some(prev) => ctx.block().and(I1, &prev, &ok),
                    None => ok,
                });
            }
            // Same dispensation as the method-direct probe above, for the
            // receiver FIELD proof: the runtime field guard re-derives, per
            // field and per call, facts the exact `(class_id, ShapeId)` pair
            // already pins (slot count, key-at-slot) plus a descriptor lookup
            // (`shape_descriptor_by_id`, a thread-local map) — it was ~80% of
            // the probe-first `c.inc()` loop. The inline read precheck the
            // field-GET sites already use proves the exact (class id, ShapeId)
            // pair — which also carries kind, not-forwarded and descriptor
            // state (`emit_class_field_read_precheck`) — plus the per-OBJECT
            // raw-f64 intact bit in two loads, and
            // because the intact bit is object-wide, ONE precheck vouches for
            // every receiver field at once. Its miss edge runs the unchanged
            // per-field runtime guard chain, so nothing is lost.
            let inline_fields_proof = !receiver_info.fields.is_empty()
                && !crate::expr::typed_feedback_emission_enabled()
                && method_inline_probe_enabled();
            let (fields_proven_idx, fields_merge_idx) = if inline_fields_proof {
                let proven_idx = ctx.new_block("typed_f64_recv_method.fields_proven");
                let proven_label = ctx.block_label(proven_idx);
                let obj_bits = ctx.block().bitcast_double_to_i64(recv_box);
                // Leaves `current_block` at the freshly created guardcall
                // block, where the per-field runtime chain below is emitted.
                // The READ guard: this proof licenses the clone's raw-f64
                // field reads (class id + ShapeId + intact bit).
                let _guardcall =
                    crate::expr::class_field_inline_guard::emit_class_field_read_precheck(
                        ctx,
                        &obj_bits,
                        &expected_class_id_str,
                        true,
                        &proven_label,
                        &[],
                        &keys_global_name,
                    );
                // Created after the precheck's own blocks so the merge (and the
                // typed/generic branch it feeds) follows the per-field guard
                // calls in emission order.
                let merge_idx = ctx.new_block("typed_f64_recv_method.fields_merge");
                (Some(proven_idx), Some(merge_idx))
            } else {
                (None, None)
            };
            let mut fields_chain: Option<String> = None;
            for field in &receiver_info.fields {
                let site_id = emit_typed_feedback_register_site(
                    ctx,
                    TypedFeedbackKind::PropertyGet,
                    &field.name,
                    TypedFeedbackContract::class_field_get(),
                );
                let key_idx = ctx.strings.intern(&field.name);
                let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
                let key_box = ctx.block().load(DOUBLE, &key_handle_global);
                let key_bits = ctx.block().bitcast_double_to_i64(&key_box);
                let key_raw = ctx
                    .block()
                    .and(I64, &key_bits, crate::nanbox::POINTER_MASK_I64);
                let field_index_str = field.index.to_string();
                let raw_guard = ctx.block().call(
                    I32,
                    "js_typed_feedback_class_field_get_guard",
                    &[
                        (I64, &site_id),
                        (DOUBLE, recv_box),
                        (I32, &expected_class_id_str),
                        (I32, &expected_shape_id),
                        (I64, &key_raw),
                        (I32, &field_index_str),
                        (I32, "1"),
                    ],
                );
                let ok = ctx.block().icmp_ne(I32, &raw_guard, "0");
                if inline_fields_proof {
                    fields_chain = Some(match fields_chain {
                        Some(prev) => ctx.block().and(I1, &prev, &ok),
                        None => ok,
                    });
                } else {
                    guard = Some(match guard {
                        Some(prev) => ctx.block().and(I1, &prev, &ok),
                        None => ok,
                    });
                }
            }
            if let (Some(proven_idx), Some(merge_idx)) = (fields_proven_idx, fields_merge_idx) {
                let merge_label = ctx.block_label(merge_idx);
                let chain_pred = ctx.block_label(ctx.current_block);
                let chain_ok = fields_chain.clone().unwrap_or_else(|| "true".to_string());
                ctx.block().br(&merge_label);
                ctx.current_block = proven_idx;
                let proven_pred = ctx.block_label(proven_idx);
                ctx.block().br(&merge_label);
                ctx.current_block = merge_idx;
                let fields_ok = ctx
                    .block()
                    .phi(I1, &[("true", &proven_pred), (&chain_ok, &chain_pred)]);
                guard = Some(match guard {
                    Some(prev) => ctx.block().and(I1, &prev, &fields_ok),
                    None => fields_ok,
                });
            }

            let typed_idx = ctx.new_block("typed_f64_recv_method.fast");
            let generic_idx = ctx.new_block("typed_f64_recv_method.generic");
            let typed_merge_idx = ctx.new_block("typed_f64_recv_method.merge");
            let typed_label = ctx.block_label(typed_idx);
            let generic_label = ctx.block_label(generic_idx);
            let typed_merge_label = ctx.block_label(typed_merge_idx);
            if let Some(guard) = guard {
                ctx.block().cond_br(&guard, &typed_label, &generic_label);
            } else {
                ctx.block().br(&typed_label);
            }

            ctx.current_block = typed_idx;
            let recv_bits = ctx.block().bitcast_double_to_i64(recv_box);
            let recv_handle = ctx
                .block()
                .and(I64, &recv_bits, crate::nanbox::POINTER_MASK_I64);
            let mut typed_args_storage: Vec<String> = Vec::with_capacity(formal_args.len());
            for value in &formal_args {
                typed_args_storage.push(ctx.block().call(
                    DOUBLE,
                    "js_typed_f64_arg_to_raw",
                    &[(DOUBLE, *value)],
                ));
            }
            let mut typed_args: Vec<(crate::types::LlvmType, &str)> =
                Vec::with_capacity(typed_args_storage.len() + 1);
            typed_args.push((I64, recv_handle.as_str()));
            for value in &typed_args_storage {
                typed_args.push((DOUBLE, value.as_str()));
            }
            let typed_value = ctx.block().call(DOUBLE, typed_fn, &typed_args);
            let after_typed = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&typed_merge_label);
            }

            ctx.current_block = generic_idx;
            let generic_value = ctx
                .block()
                .call(DOUBLE, &generic_body_fn, direct_arg_slices);
            let after_generic = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&typed_merge_label);
            }

            ctx.current_block = typed_merge_idx;
            let result = ctx.block().phi(
                DOUBLE,
                &[
                    (typed_value.as_str(), after_typed.as_str()),
                    (generic_value.as_str(), after_generic.as_str()),
                ],
            );
            ctx.record_lowered_value(
                "MethodCall",
                None,
                "typed_f64_receiver_method_direct_call",
                &LoweredValue::f64(result.clone()),
                None,
                None,
                None,
                false,
                false,
                vec![
                    format!("typed_clone={typed_fn}"),
                    format!("generic_method={generic_body_fn}"),
                    format!("receiver_class={receiver_class_name}"),
                    format!("method={property}"),
                    "receiver_arg=i64".to_string(),
                    "raw_f64_field_guard=required".to_string(),
                ],
            );
            result
        } else if let Some((typed_fn, typed_param_reps)) = typed_direct_fn {
            let formal_args: Vec<&str> = direct_arg_slices
                .iter()
                .skip(1)
                .take(typed_param_reps.len())
                .map(|(_, value)| *value)
                .collect();
            let mut guard: Option<String> = None;
            for (value, rep) in formal_args.iter().zip(typed_param_reps.iter()) {
                let ok = crate::codegen::emit_typed_arg_guard(ctx.block(), *rep, value);
                guard = Some(match guard {
                    Some(prev) => ctx.block().and(I1, &prev, &ok),
                    None => ok,
                });
            }

            let typed_idx = ctx.new_block("typed_f64_method.fast");
            let generic_idx = ctx.new_block("typed_f64_method.generic");
            let typed_merge_idx = ctx.new_block("typed_f64_method.merge");
            let typed_label = ctx.block_label(typed_idx);
            let generic_label = ctx.block_label(generic_idx);
            let typed_merge_label = ctx.block_label(typed_merge_idx);
            if let Some(guard) = guard {
                ctx.block().cond_br(&guard, &typed_label, &generic_label);
            } else {
                ctx.block().br(&typed_label);
            }

            ctx.current_block = typed_idx;
            let mut typed_args_storage: Vec<String> = Vec::with_capacity(formal_args.len());
            for (value, rep) in formal_args.iter().zip(typed_param_reps.iter()) {
                typed_args_storage.push(crate::codegen::emit_typed_arg_to_raw(
                    ctx.block(),
                    *rep,
                    value,
                ));
            }
            let typed_args: Vec<(crate::types::LlvmType, &str)> = typed_args_storage
                .iter()
                .zip(typed_param_reps.iter())
                .map(|(value, rep)| (rep.llvm_ty(), value.as_str()))
                .collect();
            let typed_value = ctx.block().call(DOUBLE, typed_fn, &typed_args);
            let after_typed = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&typed_merge_label);
            }

            ctx.current_block = generic_idx;
            let generic_value = ctx
                .block()
                .call(DOUBLE, &generic_body_fn, direct_arg_slices);
            let after_generic = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&typed_merge_label);
            }

            ctx.current_block = typed_merge_idx;
            let result = ctx.block().phi(
                DOUBLE,
                &[
                    (typed_value.as_str(), after_typed.as_str()),
                    (generic_value.as_str(), after_generic.as_str()),
                ],
            );
            ctx.record_lowered_value(
                "MethodCall",
                None,
                "typed_f64_method_direct_call",
                &LoweredValue::f64(result.clone()),
                None,
                None,
                None,
                false,
                false,
                vec![
                    format!("typed_clone={typed_fn}"),
                    format!("generic_method={generic_body_fn}"),
                    format!("receiver_class={receiver_class_name}"),
                    format!("method={property}"),
                    typed_method_signature_note("f64", &typed_param_reps),
                ],
            );
            result
        } else if let Some((typed_fn, typed_param_reps)) = typed_i32_direct_fn {
            let formal_args: Vec<&str> = direct_arg_slices
                .iter()
                .skip(1)
                .take(typed_param_reps.len())
                .map(|(_, value)| *value)
                .collect();
            let mut guard: Option<String> = None;
            for (value, rep) in formal_args.iter().zip(typed_param_reps.iter()) {
                let ok = crate::codegen::emit_typed_arg_guard(ctx.block(), *rep, value);
                guard = Some(match guard {
                    Some(prev) => ctx.block().and(I1, &prev, &ok),
                    None => ok,
                });
            }

            let typed_idx = ctx.new_block("typed_i32_method.fast");
            let generic_idx = ctx.new_block("typed_i32_method.generic");
            let typed_merge_idx = ctx.new_block("typed_i32_method.merge");
            let typed_label = ctx.block_label(typed_idx);
            let generic_label = ctx.block_label(generic_idx);
            let typed_merge_label = ctx.block_label(typed_merge_idx);
            if let Some(guard) = guard {
                ctx.block().cond_br(&guard, &typed_label, &generic_label);
            } else {
                ctx.block().br(&typed_label);
            }

            ctx.current_block = typed_idx;
            let mut typed_args_storage: Vec<String> = Vec::with_capacity(formal_args.len());
            for (value, rep) in formal_args.iter().zip(typed_param_reps.iter()) {
                typed_args_storage.push(crate::codegen::emit_typed_arg_to_raw(
                    ctx.block(),
                    *rep,
                    value,
                ));
            }
            let typed_args: Vec<(crate::types::LlvmType, &str)> = typed_args_storage
                .iter()
                .zip(typed_param_reps.iter())
                .map(|(value, rep)| (rep.llvm_ty(), value.as_str()))
                .collect();
            let raw_i32 = ctx.block().call(I32, typed_fn, &typed_args);
            let typed_value = i32_to_nanbox(ctx.block(), &raw_i32);
            let after_typed = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&typed_merge_label);
            }

            ctx.current_block = generic_idx;
            let generic_value = ctx
                .block()
                .call(DOUBLE, &generic_body_fn, direct_arg_slices);
            let after_generic = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&typed_merge_label);
            }

            ctx.current_block = typed_merge_idx;
            let result = ctx.block().phi(
                DOUBLE,
                &[
                    (typed_value.as_str(), after_typed.as_str()),
                    (generic_value.as_str(), after_generic.as_str()),
                ],
            );
            ctx.record_lowered_value(
                "MethodCall",
                None,
                "typed_i32_method_direct_call",
                &LoweredValue::js_value(result.clone()),
                None,
                None,
                None,
                false,
                false,
                vec![
                    format!("typed_clone={typed_fn}"),
                    format!("generic_method={generic_body_fn}"),
                    format!("receiver_class={receiver_class_name}"),
                    format!("method={property}"),
                    typed_method_signature_note("i32", &typed_param_reps),
                    "boxed_result_at=direct_call_boundary".to_string(),
                ],
            );
            result
        } else if let Some((typed_fn, typed_param_reps)) = typed_i1_direct_fn {
            let formal_args: Vec<&str> = direct_arg_slices
                .iter()
                .skip(1)
                .take(typed_param_reps.len())
                .map(|(_, value)| *value)
                .collect();
            let mut guard: Option<String> = None;
            for (value, rep) in formal_args.iter().zip(typed_param_reps.iter()) {
                let ok = crate::codegen::emit_typed_arg_guard(ctx.block(), *rep, value);
                guard = Some(match guard {
                    Some(prev) => ctx.block().and(I1, &prev, &ok),
                    None => ok,
                });
            }

            let typed_idx = ctx.new_block("typed_i1_method.fast");
            let generic_idx = ctx.new_block("typed_i1_method.generic");
            let typed_merge_idx = ctx.new_block("typed_i1_method.merge");
            let typed_label = ctx.block_label(typed_idx);
            let generic_label = ctx.block_label(generic_idx);
            let typed_merge_label = ctx.block_label(typed_merge_idx);
            if let Some(guard) = guard {
                ctx.block().cond_br(&guard, &typed_label, &generic_label);
            } else {
                ctx.block().br(&typed_label);
            }

            ctx.current_block = typed_idx;
            let mut typed_args_storage: Vec<String> = Vec::with_capacity(formal_args.len());
            for (value, rep) in formal_args.iter().zip(typed_param_reps.iter()) {
                typed_args_storage.push(crate::codegen::emit_typed_arg_to_raw(
                    ctx.block(),
                    *rep,
                    value,
                ));
            }
            let typed_args: Vec<(crate::types::LlvmType, &str)> = typed_args_storage
                .iter()
                .zip(typed_param_reps.iter())
                .map(|(value, rep)| (rep.llvm_ty(), value.as_str()))
                .collect();
            let typed_i1 = ctx.block().call(I1, typed_fn, &typed_args);
            let typed_i32 = ctx.block().zext(I1, &typed_i1, I32);
            let typed_value = i32_bool_to_nanbox(ctx.block(), &typed_i32);
            let after_typed = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&typed_merge_label);
            }

            ctx.current_block = generic_idx;
            let generic_value = ctx
                .block()
                .call(DOUBLE, &generic_body_fn, direct_arg_slices);
            let after_generic = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&typed_merge_label);
            }

            ctx.current_block = typed_merge_idx;
            let result = ctx.block().phi(
                DOUBLE,
                &[
                    (typed_value.as_str(), after_typed.as_str()),
                    (generic_value.as_str(), after_generic.as_str()),
                ],
            );
            ctx.record_lowered_value(
                "MethodCall",
                None,
                "typed_i1_method_direct_call",
                &LoweredValue::js_value(result.clone()),
                None,
                None,
                None,
                false,
                false,
                vec![
                    format!("typed_clone={typed_fn}"),
                    format!("generic_method={generic_body_fn}"),
                    format!("receiver_class={receiver_class_name}"),
                    format!("method={property}"),
                    typed_i1_method_signature_note(&typed_param_reps),
                    "boxed_result_at=direct_call_boundary".to_string(),
                ],
            );
            result
        } else if let Some((typed_fn, typed_param_reps)) = typed_string_direct_fn {
            let formal_args: Vec<&str> = direct_arg_slices
                .iter()
                .skip(1)
                .take(typed_param_reps.len())
                .map(|(_, value)| *value)
                .collect();
            let mut guard: Option<String> = None;
            for (value, rep) in formal_args.iter().zip(typed_param_reps.iter()) {
                let ok = crate::codegen::emit_typed_arg_guard(ctx.block(), *rep, value);
                guard = Some(match guard {
                    Some(prev) => ctx.block().and(I1, &prev, &ok),
                    None => ok,
                });
            }

            let typed_idx = ctx.new_block("typed_string_method.fast");
            let generic_idx = ctx.new_block("typed_string_method.generic");
            let typed_merge_idx = ctx.new_block("typed_string_method.merge");
            let typed_label = ctx.block_label(typed_idx);
            let generic_label = ctx.block_label(generic_idx);
            let typed_merge_label = ctx.block_label(typed_merge_idx);
            if let Some(guard) = guard {
                ctx.block().cond_br(&guard, &typed_label, &generic_label);
            } else {
                ctx.block().br(&typed_label);
            }

            ctx.current_block = typed_idx;
            let mut typed_args_storage: Vec<String> = Vec::with_capacity(formal_args.len());
            for (value, rep) in formal_args.iter().zip(typed_param_reps.iter()) {
                typed_args_storage.push(crate::codegen::emit_typed_arg_to_raw(
                    ctx.block(),
                    *rep,
                    value,
                ));
            }
            let typed_args: Vec<(crate::types::LlvmType, &str)> = typed_args_storage
                .iter()
                .zip(typed_param_reps.iter())
                .map(|(value, rep)| (rep.llvm_ty(), value.as_str()))
                .collect();
            let raw_string = ctx.block().call(I64, typed_fn, &typed_args);
            let typed_value = ctx
                .block()
                .call(DOUBLE, "js_nanbox_string", &[(I64, &raw_string)]);
            let after_typed = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&typed_merge_label);
            }

            ctx.current_block = generic_idx;
            let generic_value = ctx
                .block()
                .call(DOUBLE, &generic_body_fn, direct_arg_slices);
            let after_generic = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&typed_merge_label);
            }

            ctx.current_block = typed_merge_idx;
            let result = ctx.block().phi(
                DOUBLE,
                &[
                    (typed_value.as_str(), after_typed.as_str()),
                    (generic_value.as_str(), after_generic.as_str()),
                ],
            );
            ctx.record_lowered_value(
                "MethodCall",
                None,
                "typed_string_method_direct_call",
                &LoweredValue::js_value(result.clone()),
                None,
                None,
                None,
                false,
                false,
                vec![
                    format!("typed_clone={typed_fn}"),
                    format!("generic_method={generic_body_fn}"),
                    format!("receiver_class={receiver_class_name}"),
                    format!("method={property}"),
                    typed_method_signature_note("string", &typed_param_reps),
                    "boxed_result_at=direct_call_boundary".to_string(),
                ],
            );
            result
        } else {
            // #8774: the receiver guard dominating this block already proves
            // method identity.  Guard the selected object arguments here and
            // enter the tagged `$pshape_args` body only when every exact shape
            // matches.  An argument miss stays on the receiver-safe ordinary
            // body; a receiver miss is still handled by the outer dynamic
            // fallback.
            let pshape_arg_fallback = pshape_fn.as_deref().unwrap_or(direct_fn);
            let argument_specialized = direct_call_fn
                .is_none()
                .then(|| {
                    emit_pshape_argument_dispatch(
                        ctx,
                        receiver_class_name,
                        property,
                        direct_fn,
                        pshape_arg_fallback,
                        direct_arg_slices,
                        source_args,
                    )
                })
                .flatten();
            if let Some(argument_specialized) = argument_specialized {
                argument_specialized
            } else {
                // Representation-selection Phase 5a: this arm is reached ONLY
                // after `js_method_direct_shape_guard` /
                // `js_typed_feedback_method_direct_call_guard` matched the exact
                // class id AND the keys token — i.e. the receiver's shape is
                // already proven, and the proof is then thrown away by calling the
                // guard-ridden public body. Route to the proven-`this` clone
                // instead; identical ABI, so only the callee name changes.
                //
                // A `pshape_methods` hit additionally proves `receiver_class_name`
                // DECLARES `property` (locally by analysis or across modules by a
                // producer-authored capability), so the clone's `this` is exactly
                // the class it was compiled for — an inherited `Base::m` reached
                // through a subclass receiver never routes here.
                //
                // NOTE: the per-field `js_typed_feedback_class_field_get_guard`
                // loop above is deliberately LEFT IN PLACE. It guards the
                // `$typed_f64_recv` clone's bare `load double` field access, and
                // the whole-object shape guard does NOT subsume it: an external
                // `obj.f = "s"` preserves both the class id and the key set while
                // downgrading the slot's raw-f64 layout. The `$pshape` clone
                // needs no such guard because it never claims `JsNumber` — its
                // bare loads carry generic `JsValue` semantics (see
                // `collectors/proven_this.rs`).
                //
                // `pshape_fn` (computed once at the top of this function, where the
                // `perry_static_` exclusion and the declaring-class argument are
                // written out) is the same clone the typed arms above now route
                // their generic fallbacks to.
                let target = direct_call_fn
                    .or(pshape_index_fn.as_deref())
                    .or(nonnegative_index_direct_fn)
                    .or(pshape_fn.as_deref())
                    .unwrap_or(direct_fn);
                let result = ctx.block().call(DOUBLE, target, direct_arg_slices);
                if nonnegative_index_direct_fn.is_none() {
                    if let Some(pshape) = pshape_fn.as_deref() {
                        let receiver_provenance =
                            if ctx.imported_class_sources.contains_key(receiver_class_name) {
                                "imported_class_metadata"
                            } else {
                                "module_local_analysis"
                            };
                        ctx.record_lowered_value(
                            "MethodCall",
                            None,
                            "proven_this_method_direct_call",
                            &LoweredValue::js_value(result.clone()),
                            None,
                            None,
                            None,
                            false,
                            false,
                            vec![
                                format!("typed_clone={pshape}"),
                                format!("generic_method={direct_fn}"),
                                format!("receiver_class={receiver_class_name}"),
                                format!("method={property}"),
                                format!("receiver_provenance={receiver_provenance}"),
                                "this_representation=tagged_js_value_exact_shape".to_string(),
                                "method_identity_guard=required".to_string(),
                                "generic_dispatch_fallback=js_native_call_method_by_id".to_string(),
                            ],
                        );
                    }
                }
                result
            }
        }
    };
    let fast_truthy =
        truthy_result_kind.map(|kind| constructive_truthy(ctx, kind, fast_value.as_str()));
    let after_fast = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    // One direct call per subclass arm. Reached only from that arm's test,
    // which proved the receiver's class id AND keys token exactly — the same
    // proof the declared-class arm rests on, so the statically resolved body
    // is the one the dispatch tower would have found.
    let mut sub_values: Vec<(String, String)> = Vec::with_capacity(subclass_arms.len());
    let mut sub_truthy_values: Vec<(String, String)> = Vec::with_capacity(subclass_arms.len());
    for (i, arm) in subclass_arms.iter().enumerate() {
        ctx.current_block = sub_case_idxs[i];
        // Consume an own declaration's proof only while body evaluation
        // cannot invalidate it before a later receiver read or store.
        // An inherited body was compiled for its parent layout and keeps its
        // ordinary receiver guards. No extra emitted guard or dispatch arm.
        let target = ctx.class_ids.iter().find_map(|(name, &cid)| {
            let key = (name.clone(), property.to_string());
            (cid == arm.class_id
                && ctx.methods.get(&key) == Some(&arm.target_fn)
                && ctx.pshape_methods.contains_key(&key)
                && super::method_shape_proof::can_consume(ctx, name, property, source_args))
            .then(|| crate::collectors::pshape_method_name(&arm.target_fn))
        });
        let target = target.as_deref().unwrap_or(&arm.target_fn);
        let value = ctx.block().call(DOUBLE, target, direct_arg_slices);
        let truthy = truthy_result_kind.map(|_| {
            if let Some(kind) = constructive_method_truthiness(ctx, &arm.target_fn) {
                constructive_truthy(ctx, kind, &value)
            } else {
                total_value_truthy(ctx, &value)
            }
        });
        let after = ctx.block().label.clone();
        if !ctx.block().is_terminated() {
            ctx.block().br(&merge_label);
        }
        if let Some(truthy) = truthy {
            sub_truthy_values.push((truthy, after.clone()));
        }
        sub_values.push((value, after));
    }

    ctx.current_block = fallback_idx;
    let (args_ptr, args_len) = if fallback_user_args.is_empty() {
        ("null".to_string(), "0".to_string())
    } else {
        let n = fallback_user_args.len();
        let buf_reg = ctx.func.alloca_entry_array(DOUBLE, n);
        for (i, a_val) in fallback_user_args.iter().enumerate() {
            let slot = ctx
                .block()
                .gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
            ctx.block().store(DOUBLE, a_val, &slot);
        }
        let ptr_reg = ctx.block().next_reg();
        ctx.block().emit_raw(format!(
            "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
            ptr_reg, n, buf_reg
        ));
        (ptr_reg, n.to_string())
    };
    if let Some(site_id) = site_id {
        crate::expr::emit_typed_feedback_record_call(
            ctx.block(),
            "js_typed_feedback_record_fallback_call",
            &[(I64, &site_id)],
        );
    }
    let method_id = crate::strings::emit_static_dispatch_id(ctx.block(), &dispatch_global);
    let _decision = crate::fallback_diag::decision(
        "receiver/method-entry guard matched no compiled target arm",
        "receiver layout and method-entry identity",
    );
    let fallback_value = if let Some(cert) = certificate {
        match cert.lookup {
            Some((lookup, value)) => super::console_promise::emit_split_call_half(
                ctx,
                lookup,
                recv_box,
                value,
                fallback_user_args,
                property,
                cert.call_byte_offset,
                &method_id,
            ),
            None => ctx.block().call(
                DOUBLE,
                "js_native_call_method_by_id",
                &[
                    (DOUBLE, recv_box),
                    (I64, &method_id),
                    (crate::types::PTR, &args_ptr),
                    (I64, &args_len),
                ],
            ),
        }
    } else {
        ctx.block().call(
            DOUBLE,
            "js_native_call_method_by_id",
            &[
                (DOUBLE, recv_box),
                (I64, &method_id),
                (crate::types::PTR, &args_ptr),
                (I64, &args_len),
            ],
        )
    };
    let fallback_truthy = truthy_result_kind.map(|_| total_value_truthy(ctx, &fallback_value));
    let after_fallback = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = merge_idx;
    let mut phi_inputs: Vec<(&str, &str)> = Vec::with_capacity(sub_values.len() + 2);
    phi_inputs.push((fast_value.as_str(), after_fast.as_str()));
    for (value, label) in &sub_values {
        phi_inputs.push((value.as_str(), label.as_str()));
    }
    for (value, _, end) in &layout_misses {
        phi_inputs.push((value.as_str(), end.as_str()));
    }
    phi_inputs.push((fallback_value.as_str(), after_fallback.as_str()));
    let boxed = ctx.block().phi(DOUBLE, &phi_inputs);
    if truthy_result_kind.is_some() {
        let mut truthy_inputs: Vec<(&str, &str)> = Vec::with_capacity(sub_truthy_values.len() + 2);
        truthy_inputs.push((
            fast_truthy
                .as_deref()
                .expect("truthy mode constructs a fast truthiness value"),
            after_fast.as_str(),
        ));
        for (value, label) in &sub_truthy_values {
            truthy_inputs.push((value.as_str(), label.as_str()));
        }
        truthy_inputs.push((
            fallback_truthy
                .as_deref()
                .expect("truthy mode constructs a fallback truthiness value"),
            after_fallback.as_str(),
        ));
        for (_, truthy, end) in &layout_misses {
            truthy_inputs.push((
                truthy.as_deref().expect("layout miss truthiness"),
                end.as_str(),
            ));
        }
        let truthy = ctx.block().phi(I1, &truthy_inputs);
        ctx.pending_truthy_call_result = Some((boxed.clone(), truthy));
    }
    Some(boxed)
}
