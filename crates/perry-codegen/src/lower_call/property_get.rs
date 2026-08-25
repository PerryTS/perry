//! String / array / class / Map / Set / Promise / fetch / static-method
//! / instance-method dispatch — the big PropertyGet branch of
//! `lower_call`. This is by far the longest helper in this directory.
//!
//! The dispatch tower's cohesive sub-arms live in sibling modules under
//! `property_get/` (pure code move; no behavior change). This trunk keeps the
//! orchestrating `try_lower_property_get_method_call` plus the string/array
//! routing that is interleaved with `is_string_expr`/`is_array_expr` gating.

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{lower_expr, FnCtx};
use crate::lower_array_method::lower_array_method;
use crate::lower_string_method::{
    is_known_string_method_name, lower_string_method, lower_string_method_from_proven_box,
};
use crate::rooting::{any_operand_may_collect, open_rooted_group, Repr};
use crate::type_analysis::{
    is_array_expr, is_string_expr, is_typed_array_expr, receiver_class_name,
};
use crate::types::{DOUBLE, I1, I64};

mod dynamic_dispatch;
mod fetch_chain;
mod helpers;
mod imported_object;
mod map_set;
mod number_string;
mod promise_chain;
mod static_dispatch;

// Re-export the moved predicate / resolution helpers so the sibling modules
// (which begin with `use super::*;`) and the trunk can reach them by their
// original unqualified names.
pub(crate) use helpers::{
    class_chain_has_field_named, is_array_only_method_name, is_date_receiver,
    is_inherited_object_prototype_method, resolve_static_dispatch_cls, string_only_method_arity_ok,
    typed_array_lacks_array_method,
};

/// Preserve the old String-method fast path for an unproven receiver without
/// using a method name as its type proof (#7673).
///
/// The receiver is evaluated once, then a pure NaN-box tag check selects the
/// direct String lowering or the universal runtime method dispatcher. Known
/// classes skip this guard and retain the class-dispatch tower below.
fn try_lower_tag_guarded_string_method(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
    call_byte_offset: u32,
) -> Result<Option<String>> {
    if !is_known_string_method_name(property)
        || !string_only_method_arity_ok(property, args.len())
        || is_string_expr(ctx, object)
        || receiver_class_name(ctx, object).is_some()
        || matches!(object, Expr::GlobalGet(_) | Expr::NativeModuleRef(_))
    {
        return Ok(None);
    }

    let recv_box = lower_expr(ctx, object)?;
    let bits = ctx.block().bitcast_double_to_i64(&recv_box);
    let tag = ctx.block().lshr(I64, &bits, "48");
    let is_heap_string = ctx
        .block()
        .icmp_eq(I64, &tag, crate::nanbox::STRING_TAG_TOP16_I64);
    let is_short_string = ctx
        .block()
        .icmp_eq(I64, &tag, crate::nanbox::SHORT_STRING_TAG_TOP16_I64);
    let is_string = ctx.block().or(I1, &is_heap_string, &is_short_string);

    let string_idx = ctx.new_block("anystr.string");
    let generic_idx = ctx.new_block("anystr.generic");
    let merge_idx = ctx.new_block("anystr.merge");
    let string_label = ctx.block_label(string_idx);
    let generic_label = ctx.block_label(generic_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block()
        .cond_br(&is_string, &string_label, &generic_label);

    ctx.current_block = string_idx;
    let string_value =
        lower_string_method_from_proven_box(ctx, object, property, args, recv_box.clone())?;
    let string_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = generic_idx;
    let mut group = open_rooted_group(args.len() + 1);
    let recv_collects = any_operand_may_collect(ctx, args.iter());
    let rooted_recv = group.adopt_emitted(ctx, Repr::Boxed, &recv_box, recv_collects);
    for (i, arg) in args.iter().enumerate() {
        let collects = any_operand_may_collect(ctx, args[i + 1..].iter());
        group.lower(ctx, arg, collects)?;
    }
    let generic_recv = group.reread_emitted(ctx, rooted_recv);
    let generic_args = group.reread_all(ctx)?;
    let generic_value = super::console_promise::emit_native_method_str_dispatch(
        ctx,
        property,
        call_byte_offset,
        &generic_recv,
        &generic_args,
    );
    group.release(ctx);
    let generic_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    Ok(Some(ctx.block().phi(
        DOUBLE,
        &[
            (string_value.as_str(), string_end.as_str()),
            (generic_value.as_str(), generic_end.as_str()),
        ],
    )))
}

/// Try to lower a `Call { callee: PropertyGet { .. } }` via the
/// string/array/class/Map/Set/Promise/fetch/static/instance dispatch tower.
pub fn try_lower_property_get_method_call(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    // String/array method dispatch (Phase B.12) and class method
    // dispatch (Phase C.2). For PropertyGet receivers, dispatch based
    // on the receiver's static type.
    let Expr::PropertyGet {
        object, property, ..
    } = callee
    else {
        return Ok(None);
    };
    // #5247: capture this call's source byte offset now, before any argument
    // (which may be a nested call that overwrites the pending offset) is
    // lowered. The dynamic `js_native_call_method` fallback below emits the
    // `js_set_call_location` from this captured value, immediately before the
    // throwing dispatch. `0` (and the default build) → no emission.
    let call_byte_offset = ctx.strings.pending_call_offset();
    if let Some(value) =
        super::web_storage::try_lower_web_storage_method_call(ctx, object, property, args)?
    {
        return Ok(Some(value));
    }

    // Producer-proven immutable ESM object literals must run before the
    // builtin-name routes below: an adapter is allowed to own a method named
    // `set`, `toString`, `trim`, etc.
    if let Some(value) = imported_object::try_lower_imported_object_method_call(
        ctx,
        object,
        property,
        args,
        call_byte_offset,
    )? {
        return Ok(Some(value));
    }

    // Number `.toFixed`/`.toPrecision`/`.toExponential`, Buffer/Number
    // `.toString(encoding|radix)`, and the universal `.toString()` arms.
    if let Some(value) =
        number_string::try_lower_number_string_methods(ctx, object, property, args)?
    {
        return Ok(Some(value));
    }

    if is_string_expr(ctx, object)
        && !is_array_only_method_name(property)
        && is_known_string_method_name(property)
    {
        return Ok(Some(lower_string_method(ctx, object, property, args)?));
    }
    // #7673: a String-builtin method NAME is not proof that its receiver is a
    // string. An Any-typed call result may be a library object with an own
    // `trim`/`split`/`charAt` method (Zod schemas are the reported case).
    // Keep the static fast path positive-proof-only; the runtime dispatcher
    // below handles both genuine Any-typed strings and user methods.
    if is_array_expr(ctx, object)
        && !is_inherited_object_prototype_method(property)
        && !(is_typed_array_expr(ctx, object) && typed_array_lacks_array_method(property))
    {
        return Ok(Some(lower_array_method(ctx, object, property, args)?));
    }

    // -------- Promise.then / .catch / .finally --------
    if let Some(value) = promise_chain::try_lower_promise_chain_method(ctx, object, property, args)?
    {
        return Ok(Some(value));
    }

    // -------- Map/Set methods on PropertyGet receivers --------
    if let Some(value) = map_set::try_lower_map_set_methods(ctx, object, property, args)? {
        return Ok(Some(value));
    }

    // -------- Map.forEach / Set.forEach / URLSearchParams.forEach --------
    if let Some(value) = map_set::try_lower_collection_foreach(ctx, object, property, args)? {
        return Ok(Some(value));
    }

    // ── AbortController / AbortSignal / EventTarget + chained Web Fetch ──
    if let Some(value) = fetch_chain::try_lower_fetch_chain(ctx, object, property, args)? {
        return Ok(Some(value));
    }

    // Issue #687 — ClassRef receiver static-method dispatch.
    if let Some(value) =
        static_dispatch::try_lower_static_dispatch(ctx, callee, object, property, args)?
    {
        return Ok(Some(value));
    }

    if let Some(value) =
        try_lower_tag_guarded_string_method(ctx, object, property, args, call_byte_offset)?
    {
        return Ok(Some(value));
    }

    // Class instance method call (interface/dynamic dispatch tower +
    // static-fallback / virtual-override tower).
    if let Some(value) = dynamic_dispatch::try_lower_instance_method_call(
        ctx,
        object,
        property,
        args,
        call_byte_offset,
    )? {
        return Ok(Some(value));
    }

    Ok(None)
}
