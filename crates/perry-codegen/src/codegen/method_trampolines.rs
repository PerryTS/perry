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

/// Emit the stable boxed-ABI entry for a method whose selected parameters are
/// consumed as non-negative signed-i32 array indices by `$idx_u31`.
///
/// The source type is only a nomination: plain JavaScript packages erase these
/// parameters to `Any`, and even a declared `number` can hold a negative,
/// fraction, `-0`, string, Symbol, or BigInt at runtime. Every selected live
/// argument therefore passes the existing exact-i32 guard and a non-negative
/// check before the clone is entered. Any miss calls the original boxed body
/// with the original bits, preserving arbitrary property-key semantics.
pub(super) fn emit_guarded_nonnegative_index(
    llmod: &mut LlModule,
    method: &Function,
    public_name: &str,
    generic_body_name: &str,
    index_param_ids: &[u32],
    expected_class_id: u32,
    expected_shape_global: &str,
    falsy_field_default: Option<&super::param_guard::GuardedFalsyFieldDefaultMethodCandidate>,
) {
    debug_assert!(!index_param_ids.is_empty());
    let clone_name = super::typed_abi::nonnegative_index_method_name(public_name, index_param_ids);
    // The private clone has already been lowered.  Use its real generated IR
    // size—not source statement count—to decide whether this guard plus clone
    // may flatten before statepoint rewriting.  This makes tiny indexed leaves
    // disappear at their direct call sites while keeping large mutation bodies
    // behind one native call boundary.
    let statements = method.body.len();
    let preinline = llmod
        .function_estimated_ir_bytes(&clone_name)
        .is_some_and(|ir_bytes| {
            super::helpers::guarded_specialization_admits_preinline(ir_bytes, statements)
        });
    let target_triple = llmod.target_triple.clone();
    let mut params: Vec<(LlvmType, String)> = Vec::with_capacity(method.params.len() + 1);
    params.push((DOUBLE, "%this_arg".to_string()));
    for p in &method.params {
        params.push((DOUBLE, format!("%arg{}", p.id)));
    }
    let wf = llmod.define_function(public_name, DOUBLE, params);
    wf.pre_statepoint_inline = preinline;
    let _ = wf.create_block("entry");

    let fast_idx = wf.num_blocks();
    let fast_label = wf
        .create_block("nonnegative_index_method.fast")
        .label
        .clone();
    let generic_idx = wf.num_blocks();
    let generic_label = wf
        .create_block("nonnegative_index_method.generic")
        .label
        .clone();

    // The conversion helper's contract requires an already-guarded value.
    // Build a short-circuiting proof chain so a non-number never reaches it,
    // even in debug runtimes where that precondition is asserted.
    let mut guard_block_idx = 0;
    for (index, param_id) in index_param_ids.iter().enumerate() {
        let arg = format!("%arg{param_id}");
        let (exact_i32, raw_i32) = super::typed_abi::emit_typed_i32_guard_and_raw(
            wf.block_mut(guard_block_idx).unwrap(),
            &arg,
        );
        let admitted_idx = wf.num_blocks();
        let admitted_label = wf
            .create_block(&format!("nonnegative_index_method.arg{index}.i32"))
            .label
            .clone();
        wf.block_mut(guard_block_idx)
            .unwrap()
            .cond_br(&exact_i32, &admitted_label, &generic_label);

        let nonnegative = wf
            .block_mut(admitted_idx)
            .unwrap()
            .icmp_sge(I32, &raw_i32, "0");
        if index + 1 == index_param_ids.len() {
            wf.block_mut(admitted_idx)
                .unwrap()
                .cond_br(&nonnegative, &fast_label, &generic_label);
        } else {
            guard_block_idx = wf.num_blocks();
            let next_guard_label = wf
                .create_block(&format!("nonnegative_index_method.arg{}.guard", index + 1))
                .label
                .clone();
            wf.block_mut(admitted_idx).unwrap().cond_br(
                &nonnegative,
                &next_guard_label,
                &generic_label,
            );
        }
    }

    let mut arg_names: Vec<String> = Vec::with_capacity(method.params.len() + 1);
    arg_names.push("%this_arg".to_string());
    for p in &method.params {
        arg_names.push(format!("%arg{}", p.id));
    }
    let call_args: Vec<(LlvmType, &str)> =
        arg_names.iter().map(|arg| (DOUBLE, arg.as_str())).collect();
    if let Some(candidate) = falsy_field_default {
        // The index proof gets us here first. Keep the mutable default proof
        // as a second guarded diamond: exact omitted argument, exact ordinary
        // receiver layout, and exact live canonical-false slot. This wrapper
        // runs no user code between the slot load and the private call.
        let ordinary_idx = wf.num_blocks();
        let ordinary_label = wf
            .create_block("falsy_field_default.ordinary")
            .label
            .clone();
        let deref_idx = wf.num_blocks();
        let deref_label = wf.create_block("falsy_field_default.deref").label.clone();
        let field_idx = wf.num_blocks();
        let field_label = wf.create_block("falsy_field_default.field").label.clone();
        let specialized_idx = wf.num_blocks();
        let specialized_label = wf
            .create_block("falsy_field_default.specialized")
            .label
            .clone();

        let guarded_arg = format!("%arg{}", method.params[candidate.param_index].id);
        let recv_handle = {
            let blk = wf.block_mut(fast_idx).unwrap();
            let arg_bits = blk.bitcast_double_to_i64(&guarded_arg);
            let omitted = blk.icmp_eq(I64, &arg_bits, crate::nanbox::TAG_UNDEFINED_I64);
            let recv_bits = blk.bitcast_double_to_i64("%this_arg");
            let recv_handle = blk.and(I64, &recv_bits, crate::nanbox::POINTER_MASK_I64);
            let tag = blk.lshr(I64, &recv_bits, "48");
            let tagged = blk.icmp_eq(I64, &tag, "32765");
            let heap_floor =
                crate::target_layout::heap_addr_lower_bound_inclusive(&target_triple).to_string();
            let heap_ceiling =
                crate::target_layout::heap_addr_upper_bound_exclusive(&target_triple).to_string();
            let above_floor = blk.icmp_uge(I64, &recv_handle, &heap_floor);
            let below_ceiling = blk.icmp_ult(I64, &recv_handle, &heap_ceiling);
            let in_heap = blk.and(I1, &above_floor, &below_ceiling);
            let safe = blk.and(I1, &tagged, &in_heap);
            let enter = blk.and(I1, &omitted, &safe);
            blk.cond_br(&enter, &deref_label, &ordinary_label);
            recv_handle
        };

        let obj_ptr = {
            let blk = wf.block_mut(deref_idx).unwrap();
            let obj_ptr = blk.inttoptr(I64, &recv_handle);
            let gc_header_ptr = blk.gep(crate::types::I8, &obj_ptr, &[(I64, "-8")]);
            let gc_header = blk.load(I32, &gc_header_ptr);
            let guarded_gc = blk.and(I32, &gc_header, "142639359");
            let gc_ok = blk.icmp_eq(I32, &guarded_gc, "2");
            let class_shape = blk.load(I64, &obj_ptr);
            let expected_shape = blk.load(I32, &format!("@{expected_shape_global}"));
            let expected_shape_i64 = blk.zext(I32, &expected_shape, I64);
            let expected_shape_high = blk.shl(I64, &expected_shape_i64, "32");
            let expected = blk.or(I64, &expected_shape_high, &expected_class_id.to_string());
            let shape_matches = blk.icmp_eq(I64, &class_shape, &expected);
            let shape_rel = blk.add(I32, &expected_shape, "-2147483648");
            let shape_valid = blk.icmp_ult(I32, &shape_rel, "1073741824");
            let exact_layout = blk.and(I1, &gc_ok, &shape_matches);
            let exact_layout = blk.and(I1, &exact_layout, &shape_valid);
            blk.cond_br(&exact_layout, &field_label, &ordinary_label);
            obj_ptr
        };

        {
            let blk = wf.block_mut(field_idx).unwrap();
            let byte_offset = (16 + candidate.field_index * 8).to_string();
            let field_ptr = blk.gep(crate::types::I8, &obj_ptr, &[(I64, &byte_offset)]);
            let field = blk.load(DOUBLE, &field_ptr);
            let field_bits = blk.bitcast_double_to_i64(&field);
            let is_false = blk.icmp_eq(I64, &field_bits, crate::nanbox::TAG_FALSE_I64);
            blk.cond_br(&is_false, &specialized_label, &ordinary_label);
        }

        let specialized_name = guarded_falsy_field_default_name(&clone_name, candidate.param_index);
        let specialized_value =
            wf.block_mut(specialized_idx)
                .unwrap()
                .call(DOUBLE, &specialized_name, &call_args);
        wf.block_mut(specialized_idx)
            .unwrap()
            .ret(DOUBLE, &specialized_value);

        let ordinary_value =
            wf.block_mut(ordinary_idx)
                .unwrap()
                .call(DOUBLE, &clone_name, &call_args);
        wf.block_mut(ordinary_idx)
            .unwrap()
            .ret(DOUBLE, &ordinary_value);
    } else {
        let fast_value = wf
            .block_mut(fast_idx)
            .unwrap()
            .call(DOUBLE, &clone_name, &call_args);
        wf.block_mut(fast_idx).unwrap().ret(DOUBLE, &fast_value);
    }
    let generic_value =
        wf.block_mut(generic_idx)
            .unwrap()
            .call(DOUBLE, generic_body_name, &call_args);
    wf.block_mut(generic_idx)
        .unwrap()
        .ret(DOUBLE, &generic_value);
}

pub(super) fn guarded_falsy_field_default_name(base_name: &str, param_index: usize) -> String {
    format!("{base_name}$default_false{param_index}")
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
