//! Proxy / Reflect metaprogramming.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::{anyhow, bail, Result};
#[allow(unused_imports)]
use perry_hir::{BinaryOp, CompareOp, Expr, UnaryOp, UpdateOp};
#[allow(unused_imports)]
use perry_types::Type as HirType;

#[allow(unused_imports)]
use crate::lower_call::{lower_call, lower_native_method_call, lower_new};
#[allow(unused_imports)]
use crate::lower_conditional::{lower_conditional, lower_logical, lower_truthy};
#[allow(unused_imports)]
use crate::lower_string_method::{
    flatten_string_add_chain, lower_string_coerce_concat, lower_string_concat,
    lower_string_concat_chain, lower_string_self_append,
};
#[allow(unused_imports)]
use crate::nanbox::{double_literal, POINTER_MASK_I64};
#[allow(unused_imports)]
use crate::type_analysis::{
    compute_auto_captures, is_array_expr, is_bigint_expr, is_bool_expr, is_map_expr,
    is_numeric_expr, is_set_expr, is_string_expr, is_url_search_params_expr, receiver_class_name,
};
#[allow(unused_imports)]
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

#[allow(unused_imports)]
use super::{
    buffer_alias_metadata_suffix, can_lower_expr_as_i32, emit_layout_note_slot_on_block,
    emit_shadow_slot_clear, emit_shadow_slot_update_for_expr, emit_string_literal_global,
    emit_v8_export_call, emit_v8_member_method_call, emit_write_barrier,
    emit_write_barrier_slot_on_block, expr_is_known_non_pointer_shadow_value,
    extract_array_of_object_shape, i32_bool_to_nanbox, import_origin_suffix,
    is_global_this_builtin_function_name, is_global_this_builtin_name, is_known_finite,
    lower_array_literal, lower_channel_reduction, lower_expr, lower_expr_as_i32,
    lower_index_set_fast, lower_js_args_array, lower_object_literal, lower_stream_super_init,
    lower_url_string_getter, nanbox_bigint_inline, nanbox_pointer_inline,
    nanbox_pointer_inline_pub, nanbox_string_inline, proxy_build_args_array, try_flat_const_2d_int,
    try_lower_flat_const_index_get, try_match_channel_reduction, try_static_class_name,
    unbox_str_handle, unbox_to_i64, variant_name, ChannelReduction, FlatConstInfo, FnCtx,
    I18nLowerCtx,
};

/// `p.call(thisArg, ...rest)` / `p.apply(thisArg, argsArray)` where `p` is a
/// Proxy (#3656). The HIR lowers the callee to `ProxyGet(p, "call"|"apply")`,
/// which would otherwise read `.call`/`.apply` off the *target* and invoke the
/// target directly. Per `Function.prototype.{call,apply}` semantics the `this`
/// of the invocation is the proxy, so the call must route through the proxy's
/// `[[Call]]` (the `apply` trap) with `thisArg` bound. Returns `None` when the
/// callee isn't a proxy `.call`/`.apply` so the normal dispatch proceeds.
pub(crate) fn try_lower_proxy_fn_call_apply(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    let Expr::ProxyGet { proxy, key } = callee else {
        return Ok(None);
    };
    let is_apply = match key.as_ref() {
        Expr::String(s) if s == "apply" => true,
        Expr::String(s) if s == "call" => false,
        _ => return Ok(None),
    };
    let p = lower_expr(ctx, proxy)?;
    let this_arg = match args.first() {
        Some(a) => lower_expr(ctx, a)?,
        None => double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
    };
    let arr_box = if is_apply {
        // 2nd arg is the already-built argument array (a JSValue). When absent,
        // synthesize an empty array so the trap receives a real argArray.
        match args.get(1) {
            Some(a) => lower_expr(ctx, a)?,
            None => {
                let arr_handle = proxy_build_args_array(ctx, &[])?;
                let blk = ctx.block();
                nanbox_pointer_inline(blk, &arr_handle)
            }
        }
    } else {
        let rest: Vec<Expr> = args.iter().skip(1).cloned().collect();
        let arr_handle = proxy_build_args_array(ctx, &rest)?;
        let blk = ctx.block();
        nanbox_pointer_inline(blk, &arr_handle)
    };
    Ok(Some(ctx.block().call(
        DOUBLE,
        "js_proxy_apply",
        &[(DOUBLE, &p), (DOUBLE, &this_arg), (DOUBLE, &arr_box)],
    )))
}

fn put_value_static_property_fast_path(
    ctx: &FnCtx<'_>,
    target: &Expr,
    key: &Expr,
    receiver: &Expr,
) -> Option<String> {
    let Expr::String(property) = key else {
        return None;
    };
    match (target, receiver) {
        (Expr::LocalGet(id), Expr::LocalGet(receiver_id)) if id == receiver_id => {
            let pod_field = ctx.pod_records.get(id).is_some_and(|local| {
                local
                    .layout
                    .fields
                    .iter()
                    .any(|field| field.name == *property)
            });
            let scalar_field = ctx
                .scalar_replaced
                .get(id)
                .is_some_and(|fields| fields.contains_key(property));
            if pod_field || scalar_field {
                return Some(property.clone());
            }
            receiver_class_name(ctx, target)
                .and_then(|class_name| {
                    crate::type_analysis::class_field_global_index(ctx, &class_name, property)
                })
                .map(|_| property.clone())
        }
        (Expr::This, Expr::This) => {
            if ctx
                .scalar_ctor_target
                .last()
                .and_then(|tid| ctx.scalar_replaced.get(tid))
                .is_some_and(|fields| fields.contains_key(property))
            {
                return Some(property.clone());
            }
            receiver_class_name(ctx, target)
                .and_then(|class_name| {
                    crate::type_analysis::class_field_global_index(ctx, &class_name, property)
                })
                .map(|_| property.clone())
        }
        _ if same_side_effect_free_receiver(target, receiver) => {
            let class_name = receiver_class_name(ctx, target)?;
            crate::type_analysis::class_field_global_index(ctx, &class_name, property)
                .map(|_| property.clone())
        }
        _ => None,
    }
}

fn same_side_effect_free_receiver(target: &Expr, receiver: &Expr) -> bool {
    match (target, receiver) {
        (Expr::LocalGet(id), Expr::LocalGet(receiver_id)) => id == receiver_id,
        (Expr::This, Expr::This) => true,
        (
            Expr::PropertyGet { object, property },
            Expr::PropertyGet {
                object: receiver_object,
                property: receiver_property,
            },
        ) => {
            property == receiver_property
                && same_side_effect_free_receiver(object.as_ref(), receiver_object.as_ref())
        }
        _ => false,
    }
}

fn same_put_value_receiver_expr(target: &Expr, receiver: &Expr) -> bool {
    match (target, receiver) {
        (Expr::Undefined, Expr::Undefined)
        | (Expr::Null, Expr::Null)
        | (Expr::This, Expr::This) => true,
        (Expr::Bool(a), Expr::Bool(b)) => a == b,
        (Expr::Number(a), Expr::Number(b)) => a.to_bits() == b.to_bits(),
        (Expr::Integer(a), Expr::Integer(b)) => a == b,
        (Expr::BigInt(a), Expr::BigInt(b))
        | (Expr::String(a), Expr::String(b))
        | (Expr::NativeModuleRef(a), Expr::NativeModuleRef(b)) => a == b,
        (Expr::LocalGet(a), Expr::LocalGet(b)) => a == b,
        (Expr::GlobalGet(a), Expr::GlobalGet(b)) => a == b,
        (Expr::FuncRef(a), Expr::FuncRef(b)) => a == b,
        (
            Expr::ExternFuncRef {
                name: a_name,
                param_types: a_params,
                return_type: a_return,
            },
            Expr::ExternFuncRef {
                name: b_name,
                param_types: b_params,
                return_type: b_return,
            },
        ) => a_name == b_name && a_params == b_params && a_return == b_return,
        (
            Expr::Call {
                callee: a_callee,
                args: a_args,
                type_args: a_type_args,
            },
            Expr::Call {
                callee: b_callee,
                args: b_args,
                type_args: b_type_args,
            },
        ) => {
            a_type_args == b_type_args
                && same_put_value_receiver_expr(a_callee, b_callee)
                && a_args.len() == b_args.len()
                && a_args
                    .iter()
                    .zip(b_args.iter())
                    .all(|(a, b)| same_put_value_receiver_expr(a, b))
        }
        (
            Expr::NativeMethodCall {
                module: a_module,
                class_name: a_class,
                object: a_object,
                method: a_method,
                args: a_args,
            },
            Expr::NativeMethodCall {
                module: b_module,
                class_name: b_class,
                object: b_object,
                method: b_method,
                args: b_args,
            },
        ) => {
            a_module == b_module
                && a_class == b_class
                && a_method == b_method
                && match (a_object, b_object) {
                    (Some(a), Some(b)) => same_put_value_receiver_expr(a, b),
                    (None, None) => true,
                    _ => false,
                }
                && a_args.len() == b_args.len()
                && a_args
                    .iter()
                    .zip(b_args.iter())
                    .all(|(a, b)| same_put_value_receiver_expr(a, b))
        }
        (
            Expr::PropertyGet {
                object: a_object,
                property: a_property,
            },
            Expr::PropertyGet {
                object: b_object,
                property: b_property,
            },
        ) => a_property == b_property && same_put_value_receiver_expr(a_object, b_object),
        (
            Expr::IndexGet {
                object: a_object,
                index: a_index,
            },
            Expr::IndexGet {
                object: b_object,
                index: b_index,
            },
        ) => {
            same_put_value_receiver_expr(a_object, b_object)
                && same_put_value_receiver_expr(a_index, b_index)
        }
        _ => false,
    }
}

fn is_numeric_string_key(key: &str) -> bool {
    !key.is_empty()
        && key.chars().all(|c| c.is_ascii_digit())
        && !(key.len() > 1 && key.starts_with('0'))
}

fn put_value_index_fast_path(ctx: &FnCtx<'_>, target: &Expr, key: &Expr, receiver: &Expr) -> bool {
    if !same_side_effect_free_receiver(target, receiver) || !is_array_expr(ctx, target) {
        return false;
    }
    match key {
        Expr::String(key) => is_numeric_string_key(key),
        _ => true,
    }
}

fn try_lower_process_env_put_value_set(
    ctx: &mut FnCtx<'_>,
    target: &Expr,
    key: &Expr,
    value: &Expr,
    receiver: &Expr,
) -> Result<Option<String>> {
    if !matches!(target, Expr::ProcessEnv) || !matches!(receiver, Expr::ProcessEnv) {
        return Ok(None);
    }

    let key_handle = match key {
        Expr::String(property) => {
            let key_idx = ctx.strings.intern(property);
            let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
            let blk = ctx.block();
            let key_box = blk.load(DOUBLE, &key_handle_global);
            unbox_to_i64(blk, &key_box)
        }
        _ => {
            let key_box = lower_expr(ctx, key)?;
            let blk = ctx.block();
            let property_key = blk.call(DOUBLE, "js_to_property_key", &[(DOUBLE, &key_box)]);
            unbox_str_handle(blk, &property_key)
        }
    };
    let val_double = lower_expr(ctx, value)?;
    ctx.block()
        .call_void("js_setenv", &[(I64, &key_handle), (DOUBLE, &val_double)]);
    Ok(Some(val_double))
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::ProxyNew { target, handler } => {
            let t = lower_expr(ctx, target)?;
            let h = lower_expr(ctx, handler)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_proxy_new", &[(DOUBLE, &t), (DOUBLE, &h)]))
        }
        Expr::ProxyGet { proxy, key } => {
            let p = lower_expr(ctx, proxy)?;
            let k = lower_expr(ctx, key)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_proxy_get", &[(DOUBLE, &p), (DOUBLE, &k)]))
        }
        Expr::ProxySet { proxy, key, value } => {
            let p = lower_expr(ctx, proxy)?;
            let k = lower_expr(ctx, key)?;
            let v = lower_expr(ctx, value)?;
            let _ = ctx.block().call(
                DOUBLE,
                "js_proxy_set",
                &[(DOUBLE, &p), (DOUBLE, &k), (DOUBLE, &v)],
            );
            Ok(v)
        }
        Expr::ProxyHas { proxy, key } => {
            let p = lower_expr(ctx, proxy)?;
            let k = lower_expr(ctx, key)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_proxy_has", &[(DOUBLE, &p), (DOUBLE, &k)]))
        }
        Expr::ProxyDelete { proxy, key } => {
            let p = lower_expr(ctx, proxy)?;
            let k = lower_expr(ctx, key)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_proxy_delete", &[(DOUBLE, &p), (DOUBLE, &k)]))
        }
        Expr::ProxyApply { proxy, args } => {
            let p = lower_expr(ctx, proxy)?;
            let arr_handle = proxy_build_args_array(ctx, args)?;
            let blk = ctx.block();
            let arr_box = nanbox_pointer_inline(blk, &arr_handle);
            let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            Ok(ctx.block().call(
                DOUBLE,
                "js_proxy_apply",
                &[(DOUBLE, &p), (DOUBLE, &undef), (DOUBLE, &arr_box)],
            ))
        }
        Expr::ProxyConstruct { proxy, args } => {
            let p = lower_expr(ctx, proxy)?;
            let arr_handle = proxy_build_args_array(ctx, args)?;
            let blk = ctx.block();
            let arr_box = nanbox_pointer_inline(blk, &arr_handle);
            let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            Ok(ctx.block().call(
                DOUBLE,
                "js_proxy_construct",
                &[(DOUBLE, &p), (DOUBLE, &arr_box), (DOUBLE, &undef)],
            ))
        }
        Expr::ProxyRevocable { target, handler } => {
            // #2846: return a real `{ proxy, revoke }` record so `typeof
            // rec.revoke === "function"`, `rec.proxy.a` forwards, and the
            // revoke function survives aliasing/storage.
            let t = lower_expr(ctx, target)?;
            let h = lower_expr(ctx, handler)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_proxy_revocable", &[(DOUBLE, &t), (DOUBLE, &h)]))
        }
        Expr::ProxyRevoke(proxy) => {
            let p = lower_expr(ctx, proxy)?;
            ctx.block().call_void("js_proxy_revoke", &[(DOUBLE, &p)]);
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }
        Expr::ReflectGet {
            target,
            key,
            receiver,
        } => {
            // #2766: pass the optional receiver through; the runtime defaults
            // an `undefined` receiver to the target and binds it as `this` for
            // accessor getters.
            let t = lower_expr(ctx, target)?;
            let k = lower_expr(ctx, key)?;
            let r = lower_expr(ctx, receiver)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_reflect_get",
                &[(DOUBLE, &t), (DOUBLE, &k), (DOUBLE, &r)],
            ))
        }
        Expr::ReflectSet {
            target,
            key,
            value,
            receiver,
        } => {
            // Pass the optional receiver through; the runtime defaults an
            // `undefined` receiver to the target. A receiver distinct from an
            // Integer-Indexed target redirects the write to the receiver per
            // OrdinarySet (test262 internals/Set/key-is-valid-index-reflect-set).
            let t = lower_expr(ctx, target)?;
            let k = lower_expr(ctx, key)?;
            let v = lower_expr(ctx, value)?;
            let r = lower_expr(ctx, receiver)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_reflect_set",
                &[(DOUBLE, &t), (DOUBLE, &k), (DOUBLE, &v), (DOUBLE, &r)],
            ))
        }
        Expr::PutValueSet {
            target,
            key,
            value,
            receiver,
            strict,
        } => {
            if let Some(value) =
                try_lower_process_env_put_value_set(ctx, target, key, value, receiver)?
            {
                return Ok(value);
            }
            if let Expr::String(property) = key.as_ref() {
                if matches!(property.as_str(), "caller" | "arguments")
                    && same_side_effect_free_receiver(target, receiver)
                {
                    return super::property_set::lower(
                        ctx,
                        &Expr::PropertySet {
                            object: target.clone(),
                            property: property.clone(),
                            value: value.clone(),
                        },
                    );
                }
            }
            if let Some(property) = put_value_static_property_fast_path(ctx, target, key, receiver)
            {
                return super::property_set::lower(
                    ctx,
                    &Expr::PropertySet {
                        object: target.clone(),
                        property,
                        value: value.clone(),
                    },
                );
            }
            if put_value_index_fast_path(ctx, target, key, receiver) {
                return super::index_set::lower(
                    ctx,
                    &Expr::IndexSet {
                        object: target.clone(),
                        index: key.clone(),
                        value: value.clone(),
                    },
                );
            }
            let t = lower_expr(ctx, target)?;
            let k = lower_expr(ctx, key)?;
            let v = lower_expr(ctx, value)?;
            let r = if same_put_value_receiver_expr(target, receiver) {
                t.clone()
            } else {
                lower_expr(ctx, receiver)?
            };
            let strict_i32 = if *strict { "1" } else { "0" };
            Ok(ctx.block().call(
                DOUBLE,
                "js_put_value_set",
                &[
                    (DOUBLE, &t),
                    (DOUBLE, &k),
                    (DOUBLE, &v),
                    (DOUBLE, &r),
                    (I32, strict_i32),
                ],
            ))
        }
        Expr::ReflectHas { target, key } => {
            let t = lower_expr(ctx, target)?;
            let k = lower_expr(ctx, key)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_reflect_has", &[(DOUBLE, &t), (DOUBLE, &k)]))
        }
        Expr::ReflectDelete { target, key } => {
            let t = lower_expr(ctx, target)?;
            let k = lower_expr(ctx, key)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_reflect_delete", &[(DOUBLE, &t), (DOUBLE, &k)]))
        }
        Expr::ReflectOwnKeys(target) => {
            let t = lower_expr(ctx, target)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_reflect_own_keys", &[(DOUBLE, &t)]))
        }
        Expr::ReflectApply {
            func,
            this_arg,
            args,
        } => {
            let f = lower_expr(ctx, func)?;
            let ta = lower_expr(ctx, this_arg)?;
            let a = lower_expr(ctx, args)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_reflect_apply",
                &[(DOUBLE, &f), (DOUBLE, &ta), (DOUBLE, &a)],
            ))
        }
        Expr::ReflectConstruct {
            target,
            args,
            new_target,
        } => {
            let t = lower_expr(ctx, target)?;
            let a = lower_expr(ctx, args)?;
            let nt = lower_expr(ctx, new_target)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_reflect_construct",
                &[(DOUBLE, &t), (DOUBLE, &a), (DOUBLE, &nt)],
            ))
        }
        Expr::ReflectDefineProperty {
            target,
            key,
            descriptor,
        } => {
            let t = lower_expr(ctx, target)?;
            let k = lower_expr(ctx, key)?;
            let d = lower_expr(ctx, descriptor)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_reflect_define_property",
                &[(DOUBLE, &t), (DOUBLE, &k), (DOUBLE, &d)],
            ))
        }
        Expr::ReflectGetOwnPropertyDescriptor { target, key } => {
            let t = lower_expr(ctx, target)?;
            let k = lower_expr(ctx, key)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_reflect_get_own_property_descriptor",
                &[(DOUBLE, &t), (DOUBLE, &k)],
            ))
        }
        Expr::ReflectSetPrototypeOf { target, proto } => {
            // #2761: Reflect-specific boolean result (false on rejected change)
            // + TypeError on bad args, distinct from Object.setPrototypeOf.
            let t = lower_expr(ctx, target)?;
            let p = lower_expr(ctx, proto)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_reflect_set_prototype_of",
                &[(DOUBLE, &t), (DOUBLE, &p)],
            ))
        }
        Expr::ReflectGetPrototypeOf(target) => {
            // #2757: return the actual [[Prototype]] (shared with
            // Object.getPrototypeOf), not the target object itself. The
            // `=== Class.prototype` comparison is still folded to a constant
            // bool at lowering time (lower_expr.rs); this path handles every
            // other (value-returning) use.
            let t = lower_expr(ctx, target)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_reflect_get_prototype_of", &[(DOUBLE, &t)]))
        }
        Expr::ReflectIsExtensible(target) => {
            // #2762: Reflect-specific — boolean result + TypeError on
            // non-object, distinct from Object.isExtensible.
            let t = lower_expr(ctx, target)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_reflect_is_extensible", &[(DOUBLE, &t)]))
        }
        Expr::ReflectPreventExtensions(target) => {
            // #2762: Reflect-specific — boolean result + TypeError on
            // non-object, distinct from Object.preventExtensions (which
            // returns the object).
            let t = lower_expr(ctx, target)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_reflect_prevent_extensions", &[(DOUBLE, &t)]))
        }
        Expr::ReflectDefineMetadata {
            key,
            value,
            target,
            property_key,
        } => {
            let k = lower_expr(ctx, key)?;
            let v = lower_expr(ctx, value)?;
            let t = lower_expr(ctx, target)?;
            let p = property_key
                .as_ref()
                .map(|p| lower_expr(ctx, p))
                .transpose()?
                .unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            Ok(ctx.block().call(
                DOUBLE,
                "js_reflect_define_metadata",
                &[(DOUBLE, &k), (DOUBLE, &v), (DOUBLE, &t), (DOUBLE, &p)],
            ))
        }
        Expr::ReflectGetMetadata {
            key,
            target,
            property_key,
        } => {
            let k = lower_expr(ctx, key)?;
            let t = lower_expr(ctx, target)?;
            let p = property_key
                .as_ref()
                .map(|p| lower_expr(ctx, p))
                .transpose()?
                .unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            Ok(ctx.block().call(
                DOUBLE,
                "js_reflect_get_metadata",
                &[(DOUBLE, &k), (DOUBLE, &t), (DOUBLE, &p)],
            ))
        }
        Expr::ReflectGetOwnMetadata {
            key,
            target,
            property_key,
        } => {
            let k = lower_expr(ctx, key)?;
            let t = lower_expr(ctx, target)?;
            let p = property_key
                .as_ref()
                .map(|p| lower_expr(ctx, p))
                .transpose()?
                .unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            Ok(ctx.block().call(
                DOUBLE,
                "js_reflect_get_own_metadata",
                &[(DOUBLE, &k), (DOUBLE, &t), (DOUBLE, &p)],
            ))
        }
        Expr::ReflectHasMetadata {
            key,
            target,
            property_key,
        } => {
            let k = lower_expr(ctx, key)?;
            let t = lower_expr(ctx, target)?;
            let p = property_key
                .as_ref()
                .map(|p| lower_expr(ctx, p))
                .transpose()?
                .unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            Ok(ctx.block().call(
                DOUBLE,
                "js_reflect_has_metadata",
                &[(DOUBLE, &k), (DOUBLE, &t), (DOUBLE, &p)],
            ))
        }
        Expr::ReflectHasOwnMetadata {
            key,
            target,
            property_key,
        } => {
            let k = lower_expr(ctx, key)?;
            let t = lower_expr(ctx, target)?;
            let p = property_key
                .as_ref()
                .map(|p| lower_expr(ctx, p))
                .transpose()?
                .unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            Ok(ctx.block().call(
                DOUBLE,
                "js_reflect_has_own_metadata",
                &[(DOUBLE, &k), (DOUBLE, &t), (DOUBLE, &p)],
            ))
        }
        Expr::ReflectGetMetadataKeys {
            target,
            property_key,
        } => {
            let t = lower_expr(ctx, target)?;
            let p = property_key
                .as_ref()
                .map(|p| lower_expr(ctx, p))
                .transpose()?
                .unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            Ok(ctx.block().call(
                DOUBLE,
                "js_reflect_get_metadata_keys",
                &[(DOUBLE, &t), (DOUBLE, &p)],
            ))
        }
        Expr::ReflectGetOwnMetadataKeys {
            target,
            property_key,
        } => {
            let t = lower_expr(ctx, target)?;
            let p = property_key
                .as_ref()
                .map(|p| lower_expr(ctx, p))
                .transpose()?
                .unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            Ok(ctx.block().call(
                DOUBLE,
                "js_reflect_get_own_metadata_keys",
                &[(DOUBLE, &t), (DOUBLE, &p)],
            ))
        }
        Expr::ReflectDeleteMetadata {
            key,
            target,
            property_key,
        } => {
            let k = lower_expr(ctx, key)?;
            let t = lower_expr(ctx, target)?;
            let p = property_key
                .as_ref()
                .map(|p| lower_expr(ctx, p))
                .transpose()?
                .unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            Ok(ctx.block().call(
                DOUBLE,
                "js_reflect_delete_metadata",
                &[(DOUBLE, &k), (DOUBLE, &t), (DOUBLE, &p)],
            ))
        }

        // Issue #100: compile-time-resolved dynamic `import()`.
        //
        // The resolver in `collect_modules` already registered each
        // target path as a regular import edge (marked `is_dynamic`),
        // so the target's `__perry_init_<prefix>` runs as part of the
        // eager init chain BEFORE this dispatch site fires. The
        // populator at the end of that init has built the target's
        // `@__perry_ns_<prefix>` global; we just load it here, wrap in
        // a resolved Promise, and return.
        //
        // Single-path: emit a static load + `js_promise_resolved`.
        // Multi-path: evaluate the runtime path string, compare against
        // each compile-time constant via `js_string_equals`, and
        // dispatch to that target's namespace global. Falls through to
        // `js_promise_rejected(TypeError)` on no-match.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
