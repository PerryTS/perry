use std::collections::HashSet;

use perry_hir::Param;

use crate::block::LlBlock;
use crate::expr::{nanbox_pointer_inline, FnCtx};
use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I32, I64, PTR};

pub(crate) enum ArgumentsCallee<'a> {
    Undefined,
    FunctionWrapper(&'a str),
    CurrentClosure,
}

pub(crate) fn add_arguments_mapped_boxes(params: &[Param], boxed_vars: &mut HashSet<u32>) {
    for (_, param_id) in mapped_arguments_params(params) {
        boxed_vars.insert(param_id);
    }
}

pub(crate) fn store_param_slot(
    blk: &mut LlBlock,
    param: &Param,
    boxed_vars: &HashSet<u32>,
    arg_name: &str,
) -> String {
    let boxed_param = boxed_vars.contains(&param.id) && param.arguments_object.is_none();
    let slot = blk.alloca(if boxed_param { I64 } else { DOUBLE });
    if boxed_param {
        let arg_bits = blk.bitcast_double_to_i64(arg_name);
        let box_ptr = blk.call(I64, "js_box_alloc_bits", &[(I64, &arg_bits)]);
        blk.store(I64, &box_ptr, &slot);
    } else {
        blk.store(DOUBLE, arg_name, &slot);
    }
    slot
}

pub(crate) fn materialize_arguments_object(
    ctx: &mut FnCtx<'_>,
    params: &[Param],
    callee: ArgumentsCallee<'_>,
) {
    let Some(synth_param) = params.iter().find(|p| p.arguments_object.is_some()) else {
        return;
    };
    let Some(meta) = synth_param.arguments_object.as_ref() else {
        return;
    };
    let Some(arguments_slot) = ctx.locals.get(&synth_param.id).cloned() else {
        return;
    };
    let restricted = if meta.restricted_callee { "1" } else { "0" };
    let raw_args = ctx.block().load(DOUBLE, &arguments_slot);
    let callee_value = if meta.restricted_callee {
        double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
    } else {
        match callee {
            ArgumentsCallee::Undefined => {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            }
            ArgumentsCallee::FunctionWrapper(wrapper) => {
                let wrap_ref = format!("@{}", wrapper);
                let closure_ptr =
                    ctx.block()
                        .call(I64, "js_closure_alloc_singleton", &[(PTR, &wrap_ref)]);
                nanbox_pointer_inline(ctx.block(), &closure_ptr)
            }
            ArgumentsCallee::CurrentClosure => {
                // #7055: through the shadow-rooted slot when there is one — the
                // raw `%this_closure` register is not a GC root.
                let ptr = crate::expr::try_current_closure_ptr_value(ctx)
                    .unwrap_or_else(|| "%this_closure".to_string());
                nanbox_pointer_inline(ctx.block(), &ptr)
            }
        }
    };
    let args_obj = ctx.block().call(
        I64,
        "js_arguments_object_alloc",
        &[
            (DOUBLE, &raw_args),
            (DOUBLE, &callee_value),
            (I32, restricted),
        ],
    );
    for (arg_index, param_id) in mapped_arguments_params(params) {
        if let Some(param_slot) = ctx.locals.get(&param_id).cloned() {
            let box_ptr = ctx.block().load(I64, &param_slot);
            ctx.block().call_void(
                "js_arguments_object_map_index",
                &[
                    (I64, &args_obj),
                    (I32, &arg_index.to_string()),
                    (I64, &box_ptr),
                ],
            );
        }
    }
    let boxed_args = nanbox_pointer_inline(ctx.block(), &args_obj);
    ctx.block().store(DOUBLE, &boxed_args, &arguments_slot);
}

fn mapped_arguments_params(params: &[Param]) -> Vec<(u32, u32)> {
    params
        .iter()
        .filter_map(|p| p.arguments_object.as_ref())
        .flat_map(|meta| meta.mapped_parameter_ids.iter().copied())
        .collect()
}

/// Does `property`, resolved against `class_name`'s ancestry, declare a USER
/// `...rest` parameter — as opposed to (or in addition to) the trailing
/// `arguments` slot #677 synthesizes?
///
/// #8040/#8162. Both spellings lower as `Param { is_rest: true }`, so
/// `method_has_rest` is true for either, and `method_has_synthetic_arguments`
/// only names the synthesized slot — the PAIR still cannot distinguish
/// "synthesized `arguments` only" from "user rest AND synthesized `arguments`",
/// and those fill a different number of trailing slots (`m(a, ...rest)` with an
/// `arguments` read is `[a, rest, arguments]`: TWO arrays, from two offsets).
/// The discriminator is `arguments_object`, which the synthesized parameter
/// carries and nothing else does.
///
/// Read off the class HIR, so a class the current module has no HIR for (an
/// imported class) reports `false`, leaving those call sites on the
/// one-trailing-slot behavior they had — `method_has_synthetic_arguments`
/// still covers the imported synth-only shape via its interface bit.
pub(crate) fn method_has_user_rest(
    ctx: &crate::expr::FnCtx<'_>,
    class_name: &str,
    property: &str,
) -> bool {
    let mut walk = Some(class_name.to_string());
    while let Some(cur) = walk {
        let class = ctx.classes.get(&cur);
        if let Some(f) = class.and_then(|c| c.methods.iter().find(|m| m.name == *property)) {
            return f
                .params
                .iter()
                .any(|p| p.is_rest && p.arguments_object.is_none());
        }
        walk = class.and_then(|c| c.extends_name.clone());
    }
    false
}
