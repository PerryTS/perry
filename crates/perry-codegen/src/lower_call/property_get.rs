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

mod builtin_kind_guard;
pub(crate) mod completed_object;
mod dynamic_dispatch;
pub(crate) use dynamic_dispatch::method_target_is_cased;
mod fetch_chain;
mod helpers;
mod imported_object;
mod map_set;
mod number_string;
pub(crate) mod own_override_guard;
pub(crate) mod patched_proto;
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
    let proof = crate::lower_string_method::string_proof_for(ctx, property, args);
    if proof != crate::lower_string_method::StringProof::Guard {
        return lower_tag_guarded_string_entry(
            ctx,
            object,
            property,
            args,
            call_byte_offset,
            recv_box,
            proof,
        )
        .map(Some);
    }

    let string_idx = ctx.new_block("anystr.string");
    let generic_idx = ctx.new_block("anystr.generic");
    let merge_idx = ctx.new_block("anystr.merge");
    let tag_idx = ctx.new_block("anystr.tag");
    let string_label = ctx.block_label(string_idx);
    let generic_label = ctx.block_label(generic_idx);
    let merge_label = ctx.block_label(merge_idx);
    let tag_label = ctx.block_label(tag_idx);
    // #11915: no entry for this method and arity, so the site proves it: a
    // modified `%String.prototype%` sends every receiver to the generic arm
    // below, whose runtime entry performs the real `Get` and call — one byte
    // compare and an unlikely branch, ahead of the tag test and the arguments.
    {
        use crate::builtin_proto::{emit_patched_test, BuiltinProto};
        let patched = emit_patched_test(ctx, BuiltinProto::String);
        ctx.block().cond_br(&patched, &generic_label, &tag_label);
    }

    ctx.current_block = tag_idx;
    let is_string = emit_is_string_tag(ctx, &recv_box);
    ctx.block()
        .cond_br(&is_string, &string_label, &generic_label);

    ctx.current_block = string_idx;
    let string_value = lower_string_method_from_proven_box(
        ctx,
        object,
        property,
        args,
        recv_box.clone(),
        crate::lower_string_method::StringProof::Proven,
    )?;
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
    let generic_value = emit_tag_guarded_generic(
        ctx,
        property,
        args,
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

/// `recv` carries a heap or short string tag.
fn emit_is_string_tag(ctx: &mut FnCtx<'_>, recv_box: &str) -> String {
    let blk = ctx.block();
    let bits = blk.bitcast_double_to_i64(recv_box);
    let tag = blk.lshr(I64, &bits, "48");
    let is_heap_string = blk.icmp_eq(I64, &tag, crate::nanbox::STRING_TAG_TOP16_I64);
    let is_short_string = blk.icmp_eq(I64, &tag, crate::nanbox::SHORT_STRING_TAG_TOP16_I64);
    blk.or(I1, &is_heap_string, &is_short_string)
}

/// The tag guard's non-string arm: the universal dispatcher (or, for an
/// order-free `slice`, the shared Array-slice entry).
fn emit_tag_guarded_generic(
    ctx: &mut FnCtx<'_>,
    property: &str,
    args: &[Expr],
    call_byte_offset: u32,
    generic_recv: &str,
    generic_args: &[String],
) -> String {
    // The receiver tag already selected the guarded builtin arm. Its miss
    // uses the same dispatcher-only fallback as the other builtin guards;
    // an ordinary method site here duplicates lookup code and a `this` root.
    if property == "slice"
        && args.len() <= 2
        && crate::expr::method_site::method_site_enabled(ctx, property, args.len())
        && !crate::expr::method_site::args_may_observe_lookup(ctx, args)
    {
        // Share the remaining Array shape admission and native call out of
        // line; this replaces the dispatcher call without a second inline kind
        // tower.
        let undefined = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        let start = generic_args.first().unwrap_or(&undefined);
        let end = generic_args.get(1).unwrap_or(&undefined);
        crate::expr::calls::emit_call_location_at(ctx, call_byte_offset);
        ctx.block().call(
            DOUBLE,
            "js_array_slice_method",
            &[
                (DOUBLE, generic_recv),
                (DOUBLE, start),
                (DOUBLE, end),
                (I64, &args.len().to_string()),
            ],
        )
    } else if crate::builtin_proto::builtin_method_families(property) & !1 != 0 {
        // A key another builtin family holds too (`indexOf`, `includes`,
        // `at`, `concat`, ...): this arm's receiver is as often an array or
        // a user object (`vec.at(i)` on a class) as anything else, so it
        // takes the ordinary method site. Its probe calls a user method
        // directly, and a builtin receiver takes that builtin's entry.
        super::console_promise::emit_native_method_str_dispatch(
            ctx,
            property,
            call_byte_offset,
            generic_recv,
            generic_args,
        )
    } else {
        super::console_promise::emit_native_method_str_dispatch_plain(
            ctx,
            property,
            call_byte_offset,
            generic_recv,
            generic_args,
        )
    }
}

/// The tag-guarded call for a method with string entries (#11915). The
/// arguments are evaluated ONCE, before the tag test, and both arms read the
/// same rooted operands: the string arm calls the entry, the other arm the
/// dispatcher. With arguments whose evaluation could observe the method read
/// (`StringProof::EntryPreRead`), the byte is read before them and a
/// modified prototype sends a string receiver to the dispatcher too, which
/// performs the real `Get` and call; the string arm then calls the unchecked
/// body.
fn lower_tag_guarded_string_entry(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
    call_byte_offset: u32,
    recv_box: String,
    proof: crate::lower_string_method::StringProof,
) -> Result<String> {
    let patched_before =
        (proof == crate::lower_string_method::StringProof::EntryPreRead).then(|| {
            crate::builtin_proto::emit_patched_test(ctx, crate::builtin_proto::BuiltinProto::String)
        });
    let mut group = open_rooted_group(args.len() + 1);
    let recv_collects = any_operand_may_collect(ctx, args.iter());
    let rooted_recv = group.adopt_emitted(ctx, Repr::Boxed, &recv_box, recv_collects);
    for (i, arg) in args.iter().enumerate() {
        let collects = any_operand_may_collect(ctx, args[i + 1..].iter());
        group.lower(ctx, arg, collects)?;
    }
    let recv = group.reread_emitted(ctx, rooted_recv);
    let arg_vals = group.reread_all(ctx)?;

    let string_idx = ctx.new_block("anystr.string");
    let generic_idx = ctx.new_block("anystr.generic");
    let merge_idx = ctx.new_block("anystr.merge");
    let string_label = ctx.block_label(string_idx);
    let generic_label = ctx.block_label(generic_idx);
    let merge_label = ctx.block_label(merge_idx);
    let is_string = emit_is_string_tag(ctx, &recv);
    ctx.block()
        .cond_br(&is_string, &string_label, &generic_label);

    ctx.current_block = string_idx;
    let string_value = crate::lower_string_method::emit_string_entry_call(
        ctx,
        object,
        property,
        &recv,
        &arg_vals,
        patched_before.as_deref(),
        Some(&generic_label),
    )?;
    let string_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = generic_idx;
    let generic_value =
        emit_tag_guarded_generic(ctx, property, args, call_byte_offset, &recv, &arg_vals);
    let generic_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    let value = ctx.block().phi(
        DOUBLE,
        &[
            (string_value.as_str(), string_end.as_str()),
            (generic_value.as_str(), generic_end.as_str()),
        ],
    );
    group.release(ctx);
    Ok(value)
}

/// Try to lower a `Call { callee: PropertyGet { .. } }` via the
/// string/array/class/Map/Set/Promise/fetch/static/instance dispatch tower.
pub fn try_lower_property_get_method_call(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
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
    if property == "apply"
        && args.len() == 2
        && matches!(crate::type_analysis::static_type_of(ctx, &args[1]),
            Some(perry_hir::types::Type::Named(name)) if name == "__perry_arguments_object")
        && crate::expr::method_site::method_site_enabled(ctx, property, args.len())
    {
        return super::console_promise::lower_arguments_apply_site(
            ctx,
            object,
            args,
            call_byte_offset,
        )
        .map(Some);
    }
    // #11394: a method the program writes onto a builtin prototype must be
    // looked up before ANY arm below binds the call to a native builtin.
    if patched_proto::guards(ctx, object, property) {
        return patched_proto::lower(ctx, object, property, args, call_byte_offset).map(Some);
    }
    // #10943: an own property BEATS a builtin, and proving the receiver's KIND
    // proves nothing about that. The chain below is ORDERED, and every proven
    // receiver is claimed by an arm above the only one that can branch at
    // runtime, so the test has to happen here — once, above all of them, with
    // the receiver materialised so no arm re-evaluates it.
    if own_override_guard::guards(ctx, object, property) {
        return own_override_guard::lower(ctx, callee, object, property, args, call_byte_offset)
            .map(Some);
    }
    lower_method_call_chain(ctx, callee, object, property, args, call_byte_offset)
}

/// The ordered chain of specialised lowerings. Each arm proves something about
/// the receiver and lowers a direct call; the first to claim the call wins.
///
/// Reached either directly, or as the builtin arm of the own-override diamond
/// above — in which case the receiver is already materialised and every
/// `lower_expr(ctx, object)` below re-reads it instead of evaluating it again.
fn lower_method_call_chain(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    object: &Expr,
    property: &str,
    args: &[Expr],
    call_byte_offset: u32,
) -> Result<Option<String>> {
    // String/array method dispatch (Phase B.12) and class method
    // dispatch (Phase C.2). For PropertyGet receivers, dispatch based
    // on the receiver's static type.
    if let Some(value) =
        super::web_storage::try_lower_web_storage_method_call(ctx, object, property, args)?
    {
        return Ok(Some(value));
    }

    // Producer-proven immutable ESM object literals must run before the
    // builtin-name routes below: an adapter is allowed to own a method named
    // `set`, `toString`, `trim`, etc.
    if let Some(value) = completed_object::try_lower(ctx, object, property, args, call_byte_offset)?
    {
        return Ok(Some(value));
    }
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

    // #10476: the same guard for Date / Number method names. HIR folds a
    // Date intrinsic only for a proven Date and number_string.rs claims only
    // a proven number; any other receiver checks its runtime kind here.
    if let Some(value) = builtin_kind_guard::try_lower_kind_guarded_builtin_method(
        ctx,
        object,
        property,
        args,
        call_byte_offset,
    )? {
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

    if let Some(value) = imported_object::try_lower_dynamic_object_method_call(
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
