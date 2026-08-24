//! Stable boxed-ABI entry wrappers for specialized method bodies.

use perry_hir::Function;

use crate::module::LlModule;
use crate::types::{LlvmType, DOUBLE, I1, I32, I64};

use super::typed_abi::{
    emit_typed_arg_guard, emit_typed_arg_to_raw, typed_f64_method_name, typed_i1_method_name,
    typed_i32_method_name, typed_param_reps_for_params, typed_string_method_name,
    TypedFunctionTrampolineKind, TypedParamRep,
};

fn emit_typed_fast_value(
    blk: &mut crate::block::LlBlock,
    kind: TypedFunctionTrampolineKind,
    typed_name: &str,
    arg_names: &[String],
    arg_reps: &[TypedParamRep],
) -> String {
    match kind {
        TypedFunctionTrampolineKind::F64 => {
            let raw_args: Vec<String> = arg_names
                .iter()
                .zip(arg_reps.iter())
                .map(|(arg, rep)| emit_typed_arg_to_raw(blk, *rep, arg))
                .collect();
            let typed_args: Vec<(LlvmType, &str)> = raw_args
                .iter()
                .zip(arg_reps.iter())
                .map(|(arg, rep)| (rep.llvm_ty(), arg.as_str()))
                .collect();
            blk.call(DOUBLE, typed_name, &typed_args)
        }
        TypedFunctionTrampolineKind::I32 => {
            let raw_args: Vec<String> = arg_names
                .iter()
                .zip(arg_reps.iter())
                .map(|(arg, rep)| emit_typed_arg_to_raw(blk, *rep, arg))
                .collect();
            let typed_args: Vec<(LlvmType, &str)> = raw_args
                .iter()
                .zip(arg_reps.iter())
                .map(|(arg, rep)| (rep.llvm_ty(), arg.as_str()))
                .collect();
            let raw_i32 = blk.call(I32, typed_name, &typed_args);
            crate::expr::i32_to_nanbox(blk, &raw_i32)
        }
        TypedFunctionTrampolineKind::I1 => {
            let raw_args: Vec<String> = arg_names
                .iter()
                .zip(arg_reps.iter())
                .map(|(arg, rep)| emit_typed_arg_to_raw(blk, *rep, arg))
                .collect();
            let typed_args: Vec<(LlvmType, &str)> = raw_args
                .iter()
                .zip(arg_reps.iter())
                .map(|(arg, rep)| (rep.llvm_ty(), arg.as_str()))
                .collect();
            let typed_i1 = blk.call(I1, typed_name, &typed_args);
            let typed_i32 = blk.zext(I1, &typed_i1, I32);
            crate::expr::i32_bool_to_nanbox(blk, &typed_i32)
        }
        TypedFunctionTrampolineKind::StringRef => {
            let raw_args: Vec<String> = arg_names
                .iter()
                .zip(arg_reps.iter())
                .map(|(arg, rep)| emit_typed_arg_to_raw(blk, *rep, arg))
                .collect();
            let typed_args: Vec<(LlvmType, &str)> = raw_args
                .iter()
                .zip(arg_reps.iter())
                .map(|(arg, rep)| (rep.llvm_ty(), arg.as_str()))
                .collect();
            let raw_string = blk.call(I64, typed_name, &typed_args);
            blk.call(DOUBLE, "js_nanbox_string", &[(I64, &raw_string)])
        }
    }
}

pub(super) fn emit_public_typed(
    llmod: &mut LlModule,
    method: &Function,
    public_name: &str,
    generic_body_name: &str,
    kind: TypedFunctionTrampolineKind,
) {
    let typed_name = match kind {
        TypedFunctionTrampolineKind::F64 => typed_f64_method_name(public_name),
        TypedFunctionTrampolineKind::I32 => typed_i32_method_name(public_name),
        TypedFunctionTrampolineKind::I1 => typed_i1_method_name(public_name),
        TypedFunctionTrampolineKind::StringRef => typed_string_method_name(public_name),
    };
    let arg_reps = match kind {
        TypedFunctionTrampolineKind::F64 => typed_param_reps_for_params(&method.params)
            .unwrap_or_else(|| vec![TypedParamRep::F64; method.params.len()]),
        TypedFunctionTrampolineKind::I32 => typed_param_reps_for_params(&method.params)
            .unwrap_or_else(|| vec![TypedParamRep::I32; method.params.len()]),
        TypedFunctionTrampolineKind::I1 => typed_param_reps_for_params(&method.params)
            .unwrap_or_else(|| vec![TypedParamRep::I1; method.params.len()]),
        TypedFunctionTrampolineKind::StringRef => typed_param_reps_for_params(&method.params)
            .unwrap_or_else(|| vec![TypedParamRep::StringRef; method.params.len()]),
    };
    let mut params: Vec<(LlvmType, String)> = Vec::with_capacity(method.params.len() + 1);
    params.push((DOUBLE, "%this_arg".to_string()));
    for p in &method.params {
        params.push((DOUBLE, format!("%arg{}", p.id)));
    }
    let arg_names: Vec<String> = method
        .params
        .iter()
        .map(|p| format!("%arg{}", p.id))
        .collect();
    let wf = llmod.define_function(public_name, DOUBLE, params);
    let _ = wf.create_block("entry");

    let mut guard: Option<String> = None;
    {
        let blk = wf.block_mut(0).unwrap();
        for (arg, rep) in arg_names.iter().zip(arg_reps.iter()) {
            let ok = emit_typed_arg_guard(blk, *rep, arg);
            guard = Some(match guard {
                Some(prev) => blk.and(I1, &prev, &ok),
                None => ok,
            });
        }
    }

    let Some(guard) = guard else {
        let value = emit_typed_fast_value(
            wf.block_mut(0).unwrap(),
            kind,
            &typed_name,
            &arg_names,
            &arg_reps,
        );
        wf.block_mut(0).unwrap().ret(DOUBLE, &value);
        return;
    };

    let fast_idx = wf.num_blocks();
    let fast_label = wf.create_block("typed_method_public.fast").label.clone();
    let fallback_idx = wf.num_blocks();
    let fallback_label = wf
        .create_block("typed_method_public.fallback")
        .label
        .clone();
    wf.block_mut(0)
        .unwrap()
        .cond_br(&guard, &fast_label, &fallback_label);

    let fast_value = emit_typed_fast_value(
        wf.block_mut(fast_idx).unwrap(),
        kind,
        &typed_name,
        &arg_names,
        &arg_reps,
    );
    wf.block_mut(fast_idx).unwrap().ret(DOUBLE, &fast_value);

    let mut call_args: Vec<(LlvmType, &str)> = Vec::with_capacity(arg_names.len() + 1);
    call_args.push((DOUBLE, "%this_arg"));
    for arg in &arg_names {
        call_args.push((DOUBLE, arg.as_str()));
    }
    let fallback_value =
        wf.block_mut(fallback_idx)
            .unwrap()
            .call(DOUBLE, generic_body_name, &call_args);
    wf.block_mut(fallback_idx)
        .unwrap()
        .ret(DOUBLE, &fallback_value);
}

pub(super) fn emit_public_generic(
    llmod: &mut LlModule,
    method: &Function,
    public_name: &str,
    generic_body_name: &str,
) {
    let mut params: Vec<(LlvmType, String)> = Vec::with_capacity(method.params.len() + 1);
    params.push((DOUBLE, "%this_arg".to_string()));
    for p in &method.params {
        params.push((DOUBLE, format!("%arg{}", p.id)));
    }
    let wf = llmod.define_function(public_name, DOUBLE, params);
    let _ = wf.create_block("entry");
    let mut arg_names: Vec<String> = Vec::with_capacity(method.params.len() + 1);
    arg_names.push("%this_arg".to_string());
    for p in &method.params {
        arg_names.push(format!("%arg{}", p.id));
    }
    let call_args: Vec<(LlvmType, &str)> =
        arg_names.iter().map(|arg| (DOUBLE, arg.as_str())).collect();
    let value = wf
        .block_mut(0)
        .unwrap()
        .call(DOUBLE, generic_body_name, &call_args);
    wf.block_mut(0).unwrap().ret(DOUBLE, &value);
}

pub(super) fn guarded_undefined_name(base_name: &str, param_index: usize) -> String {
    format!("{base_name}$undef{param_index}")
}

/// Emit the stable boxed-ABI wrapper for an exact-`undefined` method version.
/// The optional annotation only selected the candidate; this bit comparison is
/// the runtime proof consumed by the private clone.
pub(super) fn emit_guarded_undefined(
    llmod: &mut LlModule,
    method: &Function,
    wrapper_name: &str,
    generic_body_name: &str,
    param_index: usize,
) {
    let clone_name = guarded_undefined_name(wrapper_name, param_index);
    let mut params: Vec<(LlvmType, String)> = Vec::with_capacity(method.params.len() + 1);
    params.push((DOUBLE, "%this_arg".to_string()));
    for p in &method.params {
        params.push((DOUBLE, format!("%arg{}", p.id)));
    }
    let wf = llmod.define_function(wrapper_name, DOUBLE, params);
    let _ = wf.create_block("entry");
    let guarded_arg = format!("%arg{}", method.params[param_index].id);
    let arg_bits = wf.block_mut(0).unwrap().bitcast_double_to_i64(&guarded_arg);
    let is_undefined =
        wf.block_mut(0)
            .unwrap()
            .icmp_eq(I64, &arg_bits, crate::nanbox::TAG_UNDEFINED_I64);
    let fast_idx = wf.num_blocks();
    let fast_label = wf.create_block("undefined_method.fast").label.clone();
    let generic_idx = wf.num_blocks();
    let generic_label = wf.create_block("undefined_method.generic").label.clone();
    wf.block_mut(0)
        .unwrap()
        .cond_br(&is_undefined, &fast_label, &generic_label);

    let mut arg_names: Vec<String> = Vec::with_capacity(method.params.len() + 1);
    arg_names.push("%this_arg".to_string());
    for p in &method.params {
        arg_names.push(format!("%arg{}", p.id));
    }
    let call_args: Vec<(LlvmType, &str)> =
        arg_names.iter().map(|arg| (DOUBLE, arg.as_str())).collect();
    let fast_value = wf
        .block_mut(fast_idx)
        .unwrap()
        .call(DOUBLE, &clone_name, &call_args);
    wf.block_mut(fast_idx).unwrap().ret(DOUBLE, &fast_value);
    let generic_value =
        wf.block_mut(generic_idx)
            .unwrap()
            .call(DOUBLE, generic_body_name, &call_args);
    wf.block_mut(generic_idx)
        .unwrap()
        .ret(DOUBLE, &generic_value);
}
