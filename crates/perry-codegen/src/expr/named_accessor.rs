//! Consume the existing contained receiver proof for a named class accessor.
//! The one live ShapeId compare replaces the generic operation's call site;
//! misses use the shared runtime operation, with no site cache or validator.
use super::{lower_expr, FnCtx};
use crate::nanbox::POINTER_MASK_I64;
use crate::types::{DOUBLE, I32, I64, I8};
use anyhow::Result;
use perry_hir::Expr;

pub(super) fn candidate(
    ctx: &FnCtx<'_>,
    object: &Expr,
    property: &str,
    setter: bool,
) -> Option<(String, String)> {
    let fact = ctx.ptr_shape_receiver_fact(object)?;
    // A routed method's this proof is weaker than a contained local's proof.
    // Only the latter has vetted every invoked accessor and its arguments.
    if !matches!(object, Expr::LocalGet(id) if ctx.native_facts.shape_proven_ptr_local(*id).is_some())
    {
        return None;
    }
    let class = ctx.classes.get(&fact.class_name)?;
    let accessors = if setter {
        &class.setters
    } else {
        &class.getters
    };
    if !accessors.iter().any(|(name, _)| name == property) {
        return None;
    }
    let function = ctx
        .methods
        .get(&(
            fact.class_name.clone(),
            format!("__{}_{property}", if setter { "set" } else { "get" }),
        ))?
        .clone();
    let keys = ctx.class_keys_globals.get(&fact.class_name)?.clone();
    Some((function, keys))
}

pub(super) fn get(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    function: &str,
    keys: &str,
) -> Result<String> {
    let receiver = lower_expr(ctx, object)?;
    emit(ctx, &receiver, property, function, keys, None, false)
}

pub(super) fn emit(
    ctx: &mut FnCtx<'_>,
    receiver: &str,
    property: &str,
    function: &str,
    keys: &str,
    value: Option<&str>,
    strict: bool,
) -> Result<String> {
    let fast = ctx.new_block("named_accessor.hit");
    let slow = ctx.new_block("named_accessor.miss");
    let merge = ctx.new_block("named_accessor.merge");
    let fast_label = ctx.block_label(fast);
    let slow_label = ctx.block_label(slow);
    let merge_label = ctx.block_label(merge);
    let bits = ctx.block().bitcast_double_to_i64(receiver);
    let handle = ctx.block().and(I64, &bits, POINTER_MASK_I64);
    let pointer = ctx.block().inttoptr(I64, &handle);
    let stamp_ptr = ctx.block().gep(I8, &pointer, &[(I64, "4")]);
    let stamp = ctx.block().load(I32, &stamp_ptr);
    let expected = crate::typed_shape::class_shape_id_operand_on_block(ctx.block(), keys, true);
    let matches = ctx.block().icmp_eq(I32, &stamp, &expected);
    ctx.block().cond_br(&matches, &fast_label, &slow_label);
    ctx.current_block = fast;
    let mut args = vec![(DOUBLE, receiver)];
    if let Some(v) = value {
        args.push((DOUBLE, v));
    }
    let result = ctx.block().call(DOUBLE, function, &args);
    ctx.block().br(&merge_label);
    ctx.current_block = slow;
    let index = ctx.strings.intern(property);
    let global = format!("@{}", ctx.strings.entry(index).handle_global);
    let key = ctx.block().load(DOUBLE, &global);
    let cold = if let Some(v) = value {
        let strict = if strict { "1" } else { "0" };
        ctx.block().call(
            DOUBLE,
            "js_put_value_set",
            &[
                (DOUBLE, receiver),
                (DOUBLE, &key),
                (DOUBLE, v),
                (DOUBLE, receiver),
                (I32, strict),
            ],
        )
    } else {
        let key_bits = ctx.block().bitcast_double_to_i64(&key);
        let key_handle = ctx.block().and(I64, &key_bits, POINTER_MASK_I64);
        ctx.block().call(
            DOUBLE,
            "js_object_get_field_by_name_f64",
            &[(I64, &handle), (I64, &key_handle)],
        )
    };
    ctx.block().br(&merge_label);
    ctx.current_block = merge;
    Ok(ctx
        .block()
        .phi(DOUBLE, &[(&result, &fast_label), (&cold, &slow_label)]))
}
