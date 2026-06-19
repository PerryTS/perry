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
            ArgumentsCallee::CurrentClosure => nanbox_pointer_inline(ctx.block(), "%this_closure"),
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
