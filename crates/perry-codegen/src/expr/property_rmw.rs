//! A read/modify/write site uses the plain store site's ShapeId/slot memo.
//! The shared leaf consumes that proof once for both the read and write.
use super::{lower_expr, FnCtx};
use crate::types::{DOUBLE, I1, I32, I64, PTR};
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

/// The class a declared type nominates (the read path's
/// `guarded_declared_class_get_candidate`). It selects inline emission only;
/// the store memo word decides every access.
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

/// The read/modify/write's inline hit, proven by the site's store memo word
/// (the word `js_property_rmw` publishes through the store site's miss): the
/// receiver test, the receiver's ShapeId against the word, then the slot the
/// word names. One mechanism for every receiver, class instance or not
/// (#12483): this used to compare a class-named receiver against its class's
/// BIRTH ShapeId, which an instance that left it (a plain JavaScript class, a
/// constructor that adds keys) missed on every update.
///
/// The matched ShapeId proves the slot an own writable data property:
/// descriptor, deletion and integrity changes all mint a different ShapeId. A
/// ConstFn lane (bit 62) declines. The numeric operation cannot run JS, so the
/// one compare spans the read and the write. An `F64` lane (the word's sign
/// bit) holds a Number by its shape; any other lane needs the plain-finite test
/// on the old bits (tags, NaN and Inf decline). Every decline is the shared
/// `js_property_rmw` continuation, exactly what `js_property_rmw_fast` proves.
///
/// A field of a class whose instances keep its ShapeId
/// (`class_field_predicted_shape`) compares against that ShapeId instead of
/// the word, as its reads do, and shares a class-guard run's proof with them
/// (`class_guard_memo`); its slot and lane are the class layout's.
#[allow(clippy::too_many_arguments)]
fn inline_number_site(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    predicted: Option<(super::class_field_inline_guard::PredictedShape, bool)>,
    receiver: &str,
    rhs: &str,
    safe_number_arithmetic: bool,
    operation: i32,
    flags: &str,
    cache: &str,
    packed: &str,
    key: &str,
) -> Option<String> {
    if !safe_number_arithmetic || !matches!(operation, 0..=3 | 16..=19) {
        return None;
    }
    let token = ctx.new_block("property_rmw.token");
    let fast = ctx.new_block("property_rmw.slot");
    let number = ctx.new_block("property_rmw.number");
    let slow = ctx.new_block("property_rmw.generic");
    let join = ctx.new_block("property_rmw.join");
    let token_label = ctx.block_label(token);
    let fast_label = ctx.block_label(fast);
    let number_label = ctx.block_label(number);
    let slow_label = ctx.block_label(slow);
    let join_label = ctx.block_label(join);
    let bits = ctx.block().bitcast_double_to_i64(receiver);
    let memo = predicted
        .as_ref()
        .and_then(|_| super::class_guard_memo::memo_for(ctx, object));
    let recv = if memo
        .as_ref()
        .and_then(|memo| memo.handle.as_ref())
        .is_none()
    {
        Some(super::receiver_range::emit_fused_receiver_test(
            ctx.block(),
            &bits,
        ))
    } else {
        None
    };
    if let Some(memo) = memo.as_ref().filter(|memo| memo.reuse) {
        let site = ctx.new_block("property_rmw.memo_clear");
        let site_label = ctx.block_label(site);
        let flag = ctx.block().load(crate::types::I8, &memo.flag);
        let proven = ctx.block().icmp_ne(crate::types::I8, &flag, "0");
        ctx.block().cond_br(&proven, &fast_label, &site_label);
        ctx.current_block = site;
    }
    let recv = recv.unwrap_or_else(|| {
        memo.as_ref()
            .map(|memo| memo.receiver_test(ctx.block(), &bits))
            .unwrap_or_else(|| super::receiver_range::emit_fused_receiver_test(ctx.block(), &bits))
    });
    ctx.block()
        .cond_br(&recv.is_object_pointer, &token_label, &slow_label);

    ctx.current_block = token;
    let sid_ptr = super::receiver_range::emit_field_ptr(ctx.block(), &recv.biased, 4);
    let sid = ctx.block().load(I32, &sid_ptr);
    let word = match predicted.as_ref() {
        Some((p, _)) => {
            // The written slot excludes a completed id whose lane there is
            // special (`compatible_final_shape_ids`).
            let expected = crate::typed_shape::class_shape_id_operand_on_block(
                ctx.block(),
                &p.keys_global,
                true,
            );
            let proven = crate::typed_shape::emit_compatible_shape_eq(
                ctx.block(),
                &sid,
                &expected,
                &[p.slot],
            );
            if let Some(memo) = memo.as_ref().filter(|memo| !memo.reuse) {
                let set = ctx.new_block("property_rmw.memo_set");
                let set_label = ctx.block_label(set);
                ctx.block().cond_br(&proven, &set_label, &slow_label);
                ctx.current_block = set;
                ctx.block().store(crate::types::I8, "1", &memo.flag);
                ctx.block().br(&fast_label);
            } else {
                ctx.block().cond_br(&proven, &fast_label, &slow_label);
            }
            None
        }
        None => {
            let word = ctx.block().load_atomic_monotonic(I64, packed, 8);
            let stamp = ctx.block().trunc(I64, &word, I32);
            let shape_eq = ctx.block().icmp_eq(I32, &sid, &stamp);
            let constfn = ctx.block().and(I64, &word, CONSTFN_SLOT_BIT);
            let plain_lane = ctx.block().icmp_eq(I64, &constfn, "0");
            let proven = ctx.block().and(I1, &shape_eq, &plain_lane);
            ctx.block().cond_br(&proven, &fast_label, &slow_label);
            Some(word)
        }
    };

    ctx.current_block = fast;
    let header = crate::target_layout::object_header_size_bytes(ctx.target_triple);
    // The handle again, from the receiver bits: the memo's flag edge reaches
    // this block without the token block.
    let fast_handle = if memo
        .as_ref()
        .and_then(|memo| memo.handle.as_ref())
        .is_some()
    {
        String::new()
    } else {
        super::receiver_range::emit_handle(ctx.block(), &recv.biased)
    };
    let (slot, numeric) = match (&word, predicted.as_ref()) {
        (Some(word), _) => {
            let index = ctx.block().lshr(I64, word, "32");
            let index = ctx.block().and(I64, &index, SLOT_INDEX_MASK);
            let offset = ctx.block().shl(I64, &index, "3");
            let offset = ctx.block().add(I64, &offset, &header.to_string());
            let addr = ctx.block().add(I64, &fast_handle, &offset);
            let slot = ctx.block().inttoptr(I64, &addr);
            let old = ctx.block().load(DOUBLE, &slot);
            let f64_lane = ctx.block().icmp_slt(I64, word, "0");
            let old_bits = ctx.block().bitcast_double_to_i64(&old);
            let plain = super::class_field_inline_guard::emit_plain_finite_number_check(
                ctx.block(),
                &old_bits,
            );
            let numeric = ctx.block().or(I1, &f64_lane, &plain);
            ((slot, old), Some(numeric))
        }
        (None, Some((p, f64_lane))) => {
            let offset = (header + u64::from(p.slot) * 8) as i64;
            let slot = memo
                .as_ref()
                .and_then(|memo| memo.field_ptr(ctx.block(), offset))
                .unwrap_or_else(|| {
                    let addr = ctx.block().add(I64, &fast_handle, &offset.to_string());
                    ctx.block().inttoptr(I64, &addr)
                });
            let old = ctx.block().load(DOUBLE, &slot);
            // An `F64` lane of the compared ShapeId holds a Number.
            let numeric = (!f64_lane).then(|| {
                let old_bits = ctx.block().bitcast_double_to_i64(&old);
                super::class_field_inline_guard::emit_plain_finite_number_check(
                    ctx.block(),
                    &old_bits,
                )
            });
            ((slot, old), numeric)
        }
        (None, None) => unreachable!(),
    };
    let (slot, old) = slot;
    match numeric {
        Some(numeric) => ctx.block().cond_br(&numeric, &number_label, &slow_label),
        None => ctx.block().br(&number_label),
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

/// The store memo word's ConstFn bit (`perry_abi::PACKED_SET_CONSTFN_SLOT`).
const CONSTFN_SLOT_BIT: &str = "4611686018427387904";
const _: () = assert!(crate::runtime_abi::PACKED_SET_CONSTFN_SLOT == 1 << 62);
/// The word's slot index, below its two flag bits (bits 62 and 63).
const SLOT_INDEX_MASK: &str = "1073741823";

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
    // The proof is the same for every receiver; which sites carry it INLINE
    // is a size decision. A receiver whose class the compiler names gets the
    // inline copy (the sites the class guard used to serve inline); every
    // other site keeps the outlined `js_property_rmw_fast` below, the same
    // proof behind a leaf call.
    let inline_class = crate::type_analysis::receiver_class_name(ctx, object)
        .or_else(|| declared_class_candidate(ctx, object))
        .filter(|class| !super::property_set::class_has_computed_runtime_members(ctx, class));
    if let Some(class) = inline_class {
        let predicted = crate::type_analysis::class_field_global_index(ctx, &class, property)
            .and_then(|index| {
                super::class_field_inline_guard::class_field_predicted_shape(
                    ctx, object, &class, index,
                )
            })
            .map(|p| {
                let f64_lane =
                    super::class_field_inline_guard::class_birth_slot_is_f64(ctx, &class, p.slot);
                (p, f64_lane)
            });
        if let Some(result) = inline_number_site(
            ctx,
            object,
            predicted,
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
