//! A read/modify/write site uses the plain store site's ShapeId/slot memo.
//! The shared leaf consumes that proof once for both the read and write.
use super::{lower_expr, FnCtx};
use crate::types::{DOUBLE, I32, I64, PTR};
use anyhow::Result;
use perry_hir::{BinaryOp, Expr};

pub(super) fn lower_update(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    op: BinaryOp,
    prefix: bool,
    strict: bool,
) -> Result<String> {
    let operation = i32::from(op != BinaryOp::Sub) + 2 * i32::from(prefix);
    lower_site(ctx, object, property, &Expr::Number(0.0), operation, strict)
}

fn same_reference(ctx: &FnCtx<'_>, a: &Expr, b: &Expr) -> bool {
    match (a, b) {
        (Expr::This, Expr::This) => true,
        (Expr::LocalGet(a), Expr::LocalGet(b)) => a == b && !ctx.reassigned_locals.contains(a),
        _ => false,
    }
}

/// An inert numeric RHS may move ahead of GetValue: it neither throws nor
/// runs JS. Every other RHS keeps the normal read/operation/store lowering,
/// which rechecks the shape after any user call or collection.
pub(super) fn try_lower_compound(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    value: &Expr,
    strict: bool,
) -> Result<Option<String>> {
    let Expr::Binary { op, left, right } = value else {
        return Ok(None);
    };
    let Expr::PropertyGet {
        object: read_object,
        property: read_property,
        ..
    } = left.as_ref()
    else {
        return Ok(None);
    };
    if property != read_property
        || !same_reference(ctx, object, read_object)
        || !matches!(right.as_ref(), Expr::Number(_) | Expr::Integer(_))
    {
        return Ok(None);
    }
    // Scalar-replaced/POD objects have no heap receiver. Their existing
    // lowering already reads and writes the same stack slot.
    if let Expr::LocalGet(id) = object {
        if ctx.scalar_replaced.contains_key(id) || ctx.pod_records.contains_key(id) {
            return Ok(None);
        }
    }
    if matches!(object, Expr::This) && !ctx.scalar_ctor_target.is_empty() {
        return Ok(None);
    }
    let operation = match op {
        BinaryOp::Add => 16,
        BinaryOp::Sub => 17,
        BinaryOp::Mul => 18,
        BinaryOp::Div => 19,
        BinaryOp::Mod => 20,
        BinaryOp::BitOr => 21,
        BinaryOp::BitAnd => 22,
        BinaryOp::BitXor => 23,
        BinaryOp::Shl => 24,
        BinaryOp::Shr => 25,
        BinaryOp::UShr => 26,
        BinaryOp::Pow => 27,
    };
    lower_site(ctx, object, property, right, operation, strict).map(Some)
}

pub(super) fn try_lower_logical(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<Option<String>> {
    let Expr::Logical { op, left, right } = expr else {
        return Ok(None);
    };
    let Expr::PropertyGet {
        object, property, ..
    } = left.as_ref()
    else {
        return Ok(None);
    };
    let Expr::PropertySet {
        object: write_object,
        property: write_property,
        value,
    } = right.as_ref()
    else {
        return Ok(None);
    };
    if property != write_property
        || !same_reference(ctx, object, write_object)
        || !matches!(
            value.as_ref(),
            Expr::Number(_) | Expr::Integer(_) | Expr::Bool(_) | Expr::Null | Expr::Undefined
        )
    {
        return Ok(None);
    }
    if let Expr::LocalGet(id) = object.as_ref() {
        if ctx.scalar_replaced.contains_key(id) || ctx.pod_records.contains_key(id) {
            return Ok(None);
        }
    }
    if matches!(object.as_ref(), Expr::This) && !ctx.scalar_ctor_target.is_empty() {
        return Ok(None);
    }
    let operation = match op {
        perry_hir::LogicalOp::And => 32,
        perry_hir::LogicalOp::Or => 33,
        perry_hir::LogicalOp::Coalesce => 34,
    };
    lower_site(ctx, object, property, value, operation, ctx.is_strict_fn).map(Some)
}

/// The class a declared type nominates for a guarded field site (the read
/// path's `guarded_declared_class_get_candidate`). It selects the lowering
/// only; the inline ShapeId guard decides every access.
fn declared_class_candidate(ctx: &FnCtx<'_>, object: &Expr) -> Option<String> {
    let name = match object {
        Expr::LocalGet(id) => match ctx.local_type_hint(id)? {
            perry_hir::types::Type::Named(name) => name,
            _ => return None,
        },
        Expr::This => ctx.guarded_this_class.as_ref()?,
        _ => return None,
    };
    ctx.classes.contains_key(name).then(|| name.clone())
}

/// The plain class-field read guard proves the slot is an own writable data
/// property: descriptor, deletion and integrity changes all mint a different
/// ShapeId. The numeric operation cannot run JS, so the one guard spans the
/// read and the write. An `F64` lane holds a Number by its shape; a boxed lane
/// needs the plain-finite test on the old bits (tags, NaN and Inf decline).
/// Every decline is the shared `js_property_rmw` continuation.
#[allow(clippy::too_many_arguments)]
fn inline_number_site(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    receiver: &str,
    rhs: &str,
    safe_number_arithmetic: bool,
    operation: i32,
    flags: &str,
    cache: &str,
    packed: &str,
    key: &str,
) -> Option<String> {
    use super::class_field_inline_guard as guard;
    if !safe_number_arithmetic || !matches!(operation, 0..=3 | 16..=19) {
        return None;
    }
    // Types nominate the class exactly as the plain read does; the shape
    // guard below is the proof.
    let class = crate::type_analysis::receiver_class_name(ctx, object)
        .or_else(|| declared_class_candidate(ctx, object))?;
    if super::property_set::class_has_computed_runtime_members(ctx, &class)
        || guard::class_instances_have_constructor_reservations(ctx, &class)
        || guard::class_instances_grow_past_layout(ctx, &class)
    {
        return None;
    }
    let index = crate::type_analysis::class_field_global_index(ctx, &class, property)?;
    let raw_f64 = guard::class_field_site_raw_f64(ctx, &class, property, index);
    let class_id = ctx.class_ids.get(&class)?.to_string();
    let keys = ctx.class_keys_globals.get(&class)?.clone();
    let arms = guard::class_field_subclass_arms(ctx, &class, property, index, raw_f64);
    if !raw_f64 && !guard::class_field_arms_cover_every_subclass(ctx, &class, &arms) {
        return None;
    }
    let fast = ctx.new_block("property_rmw.slot");
    let number = ctx.new_block("property_rmw.number");
    let slow = ctx.new_block("property_rmw.generic");
    let join = ctx.new_block("property_rmw.join");
    let fast_label = ctx.block_label(fast);
    let number_label = ctx.block_label(number);
    let slow_label = ctx.block_label(slow);
    let join_label = ctx.block_label(join);
    let bits = ctx.block().bitcast_double_to_i64(receiver);
    let (_, handle) = guard::emit_class_field_read_precheck_for(
        ctx,
        Some(object),
        &bits,
        &class_id,
        raw_f64,
        &fast_label,
        &arms,
        &keys,
    );
    ctx.block().br(&slow_label);
    ctx.current_block = fast;
    let pointer = ctx.block().inttoptr(I64, &handle);
    let offset = (crate::target_layout::object_header_size_bytes(ctx.target_triple)
        + index as u64 * 8)
        .to_string();
    let slot = ctx
        .block()
        .gep(crate::types::I8, &pointer, &[(I64, &offset)]);
    let old = ctx.block().load(DOUBLE, &slot);
    if raw_f64 {
        ctx.block().br(&number_label);
    } else {
        let old_bits = ctx.block().bitcast_double_to_i64(&old);
        let plain = guard::emit_plain_finite_number_check(ctx.block(), &old_bits);
        ctx.block().cond_br(&plain, &number_label, &slow_label);
    }
    ctx.current_block = number;
    let new = match operation {
        0 | 2 => ctx.block().fsub(&old, "1.0"),
        1 | 3 => ctx.block().fadd(&old, "1.0"),
        16 => ctx.block().fadd(&old, rhs),
        17 => ctx.block().fsub(&old, rhs),
        18 => ctx.block().fmul(&old, rhs),
        19 => ctx.block().fdiv(&old, rhs),
        _ => unreachable!(),
    };
    // GC_STORE_AUDIT(POINTER_FREE): a Number operation on a Number with a
    // finite literal yields a non-NaN double: no pointer edge, no tag, and
    // the lane keeps its representation.
    ctx.block().store(DOUBLE, &new, &slot);
    let result = if operation < 2 { old } else { new };
    let fast_pred = ctx.block().label.clone();
    ctx.block().br(&join_label);
    ctx.current_block = slow;
    let generic = ctx.block().call(
        DOUBLE,
        "js_property_rmw",
        &[
            (DOUBLE, receiver),
            (I64, key),
            (DOUBLE, rhs),
            (I32, flags),
            (PTR, cache),
            (PTR, packed),
        ],
    );
    let slow_pred = ctx.block().label.clone();
    ctx.block().br(&join_label);
    ctx.current_block = join;
    Some(
        ctx.block()
            .phi(DOUBLE, &[(&result, &fast_pred), (&generic, &slow_pred)]),
    )
}

fn lower_site(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    rhs: &Expr,
    operation: i32,
    strict: bool,
) -> Result<String> {
    // Update by finite unit preserves canonical NaN. Add/sub with a finite
    // literal and mul/div with a finite nonzero literal do too. Other numeric
    // operations use the shared leaf, which canonicalizes its result once.
    let literal = match rhs {
        Expr::Number(n) => Some(*n),
        Expr::Integer(n) => Some(*n as f64),
        _ => None,
    };
    let safe_number_arithmetic = operation < 4
        || literal.is_some_and(|n| {
            n.is_finite()
                && (matches!(operation, 16 | 17) || (matches!(operation, 18 | 19) && n != 0.0))
        });
    let receiver = lower_expr(ctx, object)?;
    let rhs = lower_expr(ctx, rhs)?;
    let site = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let cache = super::inline_cache_global_name(ctx, site);
    ctx.ic_globals.push(cache.clone());
    let cache = format!("@{cache}");
    let packed = super::put_value_store_ic::allocate_packed_store_memo(ctx);
    let key = ctx.strings.intern(property);
    let global = format!("@{}", ctx.strings.entry(key).handle_global);
    let key_box = ctx.block().load(DOUBLE, &global);
    let key_bits = ctx.block().bitcast_double_to_i64(&key_box);
    let key = ctx
        .block()
        .and(I64, &key_bits, crate::nanbox::POINTER_MASK_I64);
    let op = operation.to_string();
    let flags = (operation | (i32::from(strict) << 8)).to_string();
    if let Some(result) = inline_number_site(
        ctx,
        object,
        property,
        &receiver,
        &rhs,
        safe_number_arithmetic,
        operation,
        &flags,
        &cache,
        &packed,
        &key,
    ) {
        return Ok(result);
    }
    Ok(super::ic_fast_split::emit_hole_declining_split(
        ctx,
        "property_rmw",
        "js_property_rmw_fast",
        &[
            (DOUBLE, &receiver),
            (DOUBLE, &rhs),
            (I32, &op),
            (PTR, &packed),
        ],
        "js_property_rmw",
        &[
            (DOUBLE, &receiver),
            (I64, &key),
            (DOUBLE, &rhs),
            (I32, &flags),
            (PTR, &cache),
            (PTR, &packed),
        ],
    ))
}

/// Reuse the ordinary dynamic store site through its shared outlined entry.
/// Expanding its ways/transition tower per update would exceed the inline
/// replacement budget. The helper and the plain store consume the same memo.
pub(super) fn lower_dynamic_store(
    ctx: &mut FnCtx<'_>,
    receiver: &str,
    key: &str,
    value: &str,
    strict: &str,
) -> Result<String> {
    let site = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let cache = super::inline_cache_global_name(ctx, site);
    ctx.ic_globals.push(cache.clone());
    let cache = format!("@{cache}");
    Ok(ctx.block().call(
        DOUBLE,
        "js_put_value_set_dyn_ic",
        &[
            (PTR, &cache),
            (DOUBLE, receiver),
            (DOUBLE, key),
            (DOUBLE, value),
            (I32, strict),
        ],
    ))
}
