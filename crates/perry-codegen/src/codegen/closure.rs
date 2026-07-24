//! Closure-body compilation. Split out of `codegen.rs` (now
//! `codegen/mod.rs`). Only contains `compile_closure`.

use std::collections::{HashMap, HashSet};

use anyhow::{anyhow, Context, Result};

use crate::collectors::{collect_let_ids, collect_ref_ids_in_stmts};
use crate::expr::FnCtx;
use crate::module::LlModule;
use crate::stmt;
use crate::strings::StringPool;
use crate::types::{LlvmType, DOUBLE, I1, I32, I64, PTR};

use super::opts::CrossModuleCtx;
use super::typed_abi::{
    emit_typed_arg_guard, emit_typed_arg_to_raw, generic_closure_body_name,
    lower_typed_f64_body_with_seed_locals_and_reps, lower_typed_i1_body_with_seed_locals,
    lower_typed_i32_body_with_seed_locals, lower_typed_string_body_with_seed_locals,
    typed_f64_closure_capture_reps, typed_f64_closure_name, typed_i1_closure_capture_reps,
    typed_i1_closure_name, typed_i32_closure_capture_reps, typed_i32_closure_name,
    typed_param_reps_for_params, typed_string_closure_capture_reps, typed_string_closure_name,
    TypedFunctionTrampolineKind, TypedParamRep,
};

fn emit_typed_closure_trampoline_fast_value(
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
            let mut typed_args: Vec<(LlvmType, &str)> = Vec::with_capacity(raw_args.len() + 1);
            typed_args.push((I64, "%this_closure"));
            typed_args.extend(
                raw_args
                    .iter()
                    .zip(arg_reps.iter())
                    .map(|(arg, rep)| (rep.llvm_ty(), arg.as_str())),
            );
            blk.call(DOUBLE, typed_name, &typed_args)
        }
        TypedFunctionTrampolineKind::I32 => {
            let raw_args: Vec<String> = arg_names
                .iter()
                .zip(arg_reps.iter())
                .map(|(arg, rep)| emit_typed_arg_to_raw(blk, *rep, arg))
                .collect();
            let mut typed_args: Vec<(LlvmType, &str)> = Vec::with_capacity(raw_args.len() + 1);
            typed_args.push((I64, "%this_closure"));
            typed_args.extend(
                raw_args
                    .iter()
                    .zip(arg_reps.iter())
                    .map(|(arg, rep)| (rep.llvm_ty(), arg.as_str())),
            );
            let raw_i32 = blk.call(I32, typed_name, &typed_args);
            crate::expr::i32_to_nanbox(blk, &raw_i32)
        }
        TypedFunctionTrampolineKind::I1 => {
            let raw_args: Vec<String> = arg_names
                .iter()
                .zip(arg_reps.iter())
                .map(|(arg, rep)| emit_typed_arg_to_raw(blk, *rep, arg))
                .collect();
            let mut typed_args: Vec<(LlvmType, &str)> = Vec::with_capacity(raw_args.len() + 1);
            typed_args.push((I64, "%this_closure"));
            typed_args.extend(
                raw_args
                    .iter()
                    .zip(arg_reps.iter())
                    .map(|(arg, rep)| (rep.llvm_ty(), arg.as_str())),
            );
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
            let mut typed_args: Vec<(LlvmType, &str)> = Vec::with_capacity(raw_args.len() + 1);
            typed_args.push((I64, "%this_closure"));
            typed_args.extend(
                raw_args
                    .iter()
                    .zip(arg_reps.iter())
                    .map(|(arg, rep)| (rep.llvm_ty(), arg.as_str())),
            );
            let raw_string = blk.call(I64, typed_name, &typed_args);
            blk.call(DOUBLE, "js_nanbox_string", &[(I64, &raw_string)])
        }
    }
}

fn emit_public_typed_closure_trampoline(
    llmod: &mut LlModule,
    func_id: perry_types::FuncId,
    closure_expr: &perry_hir::Expr,
    module_prefix: &str,
    generic_body_name: &str,
    kind: TypedFunctionTrampolineKind,
    string_capture_count: usize,
) -> Result<()> {
    let params = match closure_expr {
        perry_hir::Expr::Closure { params, .. } => params,
        _ => {
            return Err(anyhow!(
                "emit_public_typed_closure_trampoline: expected Expr::Closure"
            ))
        }
    };
    let public_name = format!("perry_closure_{}__{}", module_prefix, func_id);
    let typed_name = match kind {
        TypedFunctionTrampolineKind::F64 => typed_f64_closure_name(&public_name),
        TypedFunctionTrampolineKind::I32 => typed_i32_closure_name(&public_name),
        TypedFunctionTrampolineKind::I1 => typed_i1_closure_name(&public_name),
        TypedFunctionTrampolineKind::StringRef => typed_string_closure_name(&public_name),
    };
    let arg_reps = match kind {
        TypedFunctionTrampolineKind::F64 => typed_param_reps_for_params(params)
            .unwrap_or_else(|| vec![TypedParamRep::F64; params.len()]),
        TypedFunctionTrampolineKind::I32 => typed_param_reps_for_params(params)
            .unwrap_or_else(|| vec![TypedParamRep::I32; params.len()]),
        TypedFunctionTrampolineKind::I1 => typed_param_reps_for_params(params)
            .unwrap_or_else(|| vec![TypedParamRep::I1; params.len()]),
        TypedFunctionTrampolineKind::StringRef => typed_param_reps_for_params(params)
            .unwrap_or_else(|| vec![TypedParamRep::StringRef; params.len()]),
    };
    let mut llvm_params: Vec<(LlvmType, String)> = Vec::with_capacity(params.len() + 1);
    llvm_params.push((I64, "%this_closure".to_string()));
    for p in params {
        llvm_params.push((DOUBLE, format!("%arg{}", p.id)));
    }
    let arg_names: Vec<String> = params.iter().map(|p| format!("%arg{}", p.id)).collect();
    let wf = llmod.define_function(&public_name, DOUBLE, llvm_params);
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
        if string_capture_count > 0 {
            if let Some(capture_guard) =
                emit_typed_string_capture_guard(blk, "%this_closure", string_capture_count)
            {
                guard = Some(match guard {
                    Some(prev) => blk.and(I1, &prev, &capture_guard),
                    None => capture_guard,
                });
            }
        }
    }

    let Some(guard) = guard else {
        let value = emit_typed_closure_trampoline_fast_value(
            wf.block_mut(0).unwrap(),
            kind,
            &typed_name,
            &arg_names,
            &arg_reps,
        );
        wf.block_mut(0).unwrap().ret(DOUBLE, &value);
        return Ok(());
    };

    let fast_idx = wf.num_blocks();
    let fast_label = wf.create_block("typed_closure_public.fast").label.clone();
    let fallback_idx = wf.num_blocks();
    let fallback_label = wf
        .create_block("typed_closure_public.fallback")
        .label
        .clone();
    wf.block_mut(0)
        .unwrap()
        .cond_br(&guard, &fast_label, &fallback_label);

    let fast_value = emit_typed_closure_trampoline_fast_value(
        wf.block_mut(fast_idx).unwrap(),
        kind,
        &typed_name,
        &arg_names,
        &arg_reps,
    );
    wf.block_mut(fast_idx).unwrap().ret(DOUBLE, &fast_value);

    let mut call_args: Vec<(LlvmType, &str)> = Vec::with_capacity(arg_names.len() + 1);
    call_args.push((I64, "%this_closure"));
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
    Ok(())
}

fn load_typed_capture(
    blk: &mut crate::block::LlBlock,
    capture_index: usize,
    rep: TypedParamRep,
) -> String {
    let idx = capture_index.to_string();
    let captured_bits = blk.call(
        I64,
        "js_closure_get_capture_bits",
        &[(I64, "%this_closure"), (I32, &idx)],
    );
    let captured = blk.bitcast_i64_to_double(&captured_bits);
    match rep {
        TypedParamRep::F64 => blk.call(
            DOUBLE,
            "js_typed_f64_arg_to_raw",
            &[(DOUBLE, captured.as_str())],
        ),
        TypedParamRep::I32 => blk.call(
            I32,
            "js_typed_i32_arg_to_raw",
            &[(DOUBLE, captured.as_str())],
        ),
        TypedParamRep::I1 => {
            let raw_i32 = blk.call(
                I32,
                "js_typed_i1_arg_to_raw",
                &[(DOUBLE, captured.as_str())],
            );
            blk.icmp_ne(I32, &raw_i32, "0")
        }
        TypedParamRep::StringRef => blk.call(
            I64,
            "js_typed_string_arg_to_raw",
            &[(DOUBLE, captured.as_str())],
        ),
    }
}

pub(crate) fn emit_typed_string_capture_guard(
    blk: &mut crate::block::LlBlock,
    closure_handle: &str,
    capture_count: usize,
) -> Option<String> {
    let mut guard: Option<String> = None;
    for idx in 0..capture_count {
        let idx = idx.to_string();
        let captured_bits = blk.call(
            I64,
            "js_closure_get_capture_bits",
            &[(I64, closure_handle), (I32, &idx)],
        );
        let captured = blk.bitcast_i64_to_double(&captured_bits);
        let raw = blk.call(
            I32,
            "js_typed_string_arg_guard",
            &[(DOUBLE, captured.as_str())],
        );
        let ok = blk.icmp_ne(I32, &raw, "0");
        guard = Some(match guard {
            Some(prev) => blk.and(I1, &prev, &ok),
            None => ok,
        });
    }
    guard
}

pub(super) fn compile_typed_string_closure(
    llmod: &mut LlModule,
    func_id: perry_types::FuncId,
    closure_expr: &perry_hir::Expr,
    module_prefix: &str,
    module_local_types: &HashMap<u32, perry_types::Type>,
) -> Result<()> {
    let (params, body) = match closure_expr {
        perry_hir::Expr::Closure { params, body, .. } => (params, body),
        _ => {
            return Err(anyhow!(
                "compile_typed_string_closure: expected Expr::Closure"
            ))
        }
    };

    let generic_name = format!("perry_closure_{}__{}", module_prefix, func_id);
    let llvm_name = typed_string_closure_name(&generic_name);
    let mut llvm_params: Vec<(LlvmType, String)> = Vec::with_capacity(params.len() + 1);
    llvm_params.push((I64, "%this_closure".to_string()));
    let param_reps = typed_param_reps_for_params(params).ok_or_else(|| {
        anyhow!(
            "typed-string closure '{}' has unsupported parameter",
            func_id
        )
    })?;
    llvm_params.extend(
        params
            .iter()
            .zip(param_reps.iter())
            .map(|(p, rep)| (rep.llvm_ty(), format!("%arg{}", p.id))),
    );
    let lf = llmod.define_function(&llvm_name, I64, llvm_params);
    lf.linkage = "internal".to_string();
    lf.force_inline = true;
    let _ = lf.create_block("entry");

    let value = {
        let blk = lf.block_mut(0).unwrap();
        let mut seed_locals = HashMap::new();
        if let Some(captures) = typed_string_closure_capture_reps(closure_expr, module_local_types)
        {
            for (idx, (id, rep)) in captures.iter().enumerate() {
                seed_locals.insert(*id, load_typed_capture(blk, idx, *rep));
            }
        }
        lower_typed_string_body_with_seed_locals(blk, params, body, seed_locals)?
    };
    lf.block_mut(0).unwrap().ret(I64, &value);
    Ok(())
}

pub(super) fn compile_typed_f64_closure(
    llmod: &mut LlModule,
    func_id: perry_types::FuncId,
    closure_expr: &perry_hir::Expr,
    module_prefix: &str,
    module_local_types: &HashMap<u32, perry_types::Type>,
) -> Result<()> {
    let (params, body) = match closure_expr {
        perry_hir::Expr::Closure { params, body, .. } => (params, body),
        _ => return Err(anyhow!("compile_typed_f64_closure: expected Expr::Closure")),
    };

    let generic_name = format!("perry_closure_{}__{}", module_prefix, func_id);
    let llvm_name = typed_f64_closure_name(&generic_name);
    let mut llvm_params: Vec<(LlvmType, String)> = Vec::with_capacity(params.len() + 1);
    llvm_params.push((I64, "%this_closure".to_string()));
    let param_reps = typed_param_reps_for_params(params)
        .ok_or_else(|| anyhow!("typed-f64 closure '{}' has unsupported parameter", func_id))?;
    llvm_params.extend(
        params
            .iter()
            .zip(param_reps.iter())
            .map(|(p, rep)| (rep.llvm_ty(), format!("%arg{}", p.id))),
    );
    let lf = llmod.define_function(&llvm_name, DOUBLE, llvm_params);
    lf.linkage = "internal".to_string();
    lf.force_inline = true;
    let _ = lf.create_block("entry");

    let value = {
        let blk = lf.block_mut(0).unwrap();
        let mut seed_locals = HashMap::new();
        let mut seed_reps = HashMap::new();
        if let Some(captures) = typed_f64_closure_capture_reps(closure_expr, module_local_types) {
            for (idx, (id, rep)) in captures.iter().enumerate() {
                seed_locals.insert(*id, load_typed_capture(blk, idx, *rep));
                seed_reps.insert(*id, *rep);
            }
        }
        lower_typed_f64_body_with_seed_locals_and_reps(blk, params, body, seed_locals, seed_reps)?
    };
    lf.block_mut(0).unwrap().ret(DOUBLE, &value);
    Ok(())
}

pub(super) fn compile_typed_i1_closure(
    llmod: &mut LlModule,
    func_id: perry_types::FuncId,
    closure_expr: &perry_hir::Expr,
    module_prefix: &str,
    module_local_types: &HashMap<u32, perry_types::Type>,
) -> Result<()> {
    let (params, body) = match closure_expr {
        perry_hir::Expr::Closure { params, body, .. } => (params, body),
        _ => return Err(anyhow!("compile_typed_i1_closure: expected Expr::Closure")),
    };

    let generic_name = format!("perry_closure_{}__{}", module_prefix, func_id);
    let llvm_name = typed_i1_closure_name(&generic_name);
    let param_reps = typed_param_reps_for_params(params)
        .ok_or_else(|| anyhow!("typed-i1 closure '{}' has unsupported parameter", func_id))?;
    let mut llvm_params: Vec<(LlvmType, String)> = Vec::with_capacity(params.len() + 1);
    llvm_params.push((I64, "%this_closure".to_string()));
    llvm_params.extend(
        params
            .iter()
            .zip(param_reps.iter())
            .map(|(p, rep)| (rep.llvm_ty(), format!("%arg{}", p.id))),
    );
    let lf = llmod.define_function(&llvm_name, I1, llvm_params);
    lf.linkage = "internal".to_string();
    lf.force_inline = true;
    let _ = lf.create_block("entry");

    let value = {
        let blk = lf.block_mut(0).unwrap();
        let mut seed_locals = HashMap::new();
        let mut seed_reps = HashMap::new();
        if let Some(captures) = typed_i1_closure_capture_reps(closure_expr, module_local_types) {
            for (idx, (id, rep)) in captures.iter().enumerate() {
                seed_locals.insert(*id, load_typed_capture(blk, idx, *rep));
                seed_reps.insert(*id, *rep);
            }
        }
        lower_typed_i1_body_with_seed_locals(blk, params, body, seed_locals, seed_reps)?
    };
    lf.block_mut(0).unwrap().ret(I1, &value);
    Ok(())
}

pub(super) fn compile_typed_i32_closure(
    llmod: &mut LlModule,
    func_id: perry_types::FuncId,
    closure_expr: &perry_hir::Expr,
    module_prefix: &str,
    module_local_types: &HashMap<u32, perry_types::Type>,
) -> Result<()> {
    let (params, body) = match closure_expr {
        perry_hir::Expr::Closure { params, body, .. } => (params, body),
        _ => return Err(anyhow!("compile_typed_i32_closure: expected Expr::Closure")),
    };

    let generic_name = format!("perry_closure_{}__{}", module_prefix, func_id);
    let llvm_name = typed_i32_closure_name(&generic_name);
    let mut llvm_params: Vec<(LlvmType, String)> = Vec::with_capacity(params.len() + 1);
    llvm_params.push((I64, "%this_closure".to_string()));
    let param_reps = typed_param_reps_for_params(params)
        .ok_or_else(|| anyhow!("typed-i32 closure '{}' has unsupported parameter", func_id))?;
    llvm_params.extend(
        params
            .iter()
            .zip(param_reps.iter())
            .map(|(p, rep)| (rep.llvm_ty(), format!("%arg{}", p.id))),
    );
    let lf = llmod.define_function(&llvm_name, I32, llvm_params);
    lf.linkage = "internal".to_string();
    lf.force_inline = true;
    let _ = lf.create_block("entry");

    let value = {
        let blk = lf.block_mut(0).unwrap();
        let mut seed_locals = HashMap::new();
        if let Some(captures) = typed_i32_closure_capture_reps(closure_expr, module_local_types) {
            for (idx, (id, rep)) in captures.iter().enumerate() {
                seed_locals.insert(*id, load_typed_capture(blk, idx, *rep));
            }
        }
        lower_typed_i32_body_with_seed_locals(blk, params, body, seed_locals)?
    };
    lf.block_mut(0).unwrap().ret(I32, &value);
    Ok(())
}

/// Compile a closure body as a top-level LLVM function.
///
/// Signature: `double perry_closure_<modprefix>__<func_id>(i64 this_closure,
/// double arg0, double arg1, …)`. The first parameter is the closure
/// pointer (raw i64); the remaining params are the closure's own
/// declared parameters.
///
/// Inside the body, captured variables (`closure.captures`) are mapped
/// to capture indices and accessed via the runtime
/// `js_closure_get/set_capture_f64(this_closure, idx)` calls. The
/// `closure_captures` field on `FnCtx` carries the LocalId → capture
/// index map; `current_closure_ptr` carries the closure pointer SSA
/// value name.
#[allow(clippy::too_many_arguments)]
pub(super) fn compile_closure(
    llmod: &mut LlModule,
    func_id: perry_types::FuncId,
    closure_expr: &perry_hir::Expr,
    func_names: &HashMap<u32, String>,
    strings: &mut StringPool,
    classes: &HashMap<String, &perry_hir::Class>,
    methods: &HashMap<(String, String), String>,
    module_globals: &HashMap<u32, String>,
    import_function_prefixes: &HashMap<String, String>,
    enums: &HashMap<(String, String), perry_hir::EnumValue>,
    static_field_globals: &HashMap<(String, String), String>,
    class_ids: &HashMap<String, u32>,
    func_signatures: &HashMap<u32, (usize, bool, bool, bool)>,
    func_synthetic_arguments: &std::collections::HashSet<u32>,
    module_prefix: &str,
    module_boxed_vars: &std::collections::HashSet<u32>,
    // #6369: receiver-type oracle (module-wide `Stmt::Let` types, unfiltered).
    // Seeds `FnCtx.local_types` so a binding captured from an enclosing scope
    // keeps its declared type at its read sites. NOT the typed-ABI capture
    // map — the typed closure clones take `module_local_types` instead.
    module_receiver_types: &HashMap<u32, perry_types::Type>,
    closure_rest_params: &HashMap<u32, usize>,
    cross_module: &CrossModuleCtx,
) -> Result<()> {
    // Destructure the closure expression. We trust that the caller
    // passes only `Expr::Closure` here (from `collect_closures_*`).
    let (
        params,
        body,
        captures,
        captures_this,
        captures_new_target,
        enclosing_class,
        is_async,
        is_strict,
    ) = match closure_expr {
        perry_hir::Expr::Closure {
            params,
            body,
            captures,
            captures_this,
            captures_new_target,
            enclosing_class,
            is_async,
            is_strict,
            ..
        } => (
            params,
            body,
            captures,
            *captures_this,
            *captures_new_target,
            enclosing_class.clone(),
            *is_async,
            *is_strict,
        ),
        _ => return Err(anyhow!("compile_closure: expected Expr::Closure")),
    };

    let public_llvm_name = format!("perry_closure_{}__{}", module_prefix, func_id);
    let typed_public_trampoline = if cross_module.typed_f64_closures.contains(&func_id) {
        Some(TypedFunctionTrampolineKind::F64)
    } else if cross_module.typed_i32_closures.contains(&func_id) {
        Some(TypedFunctionTrampolineKind::I32)
    } else if cross_module.typed_i1_closures.contains(&func_id) {
        Some(TypedFunctionTrampolineKind::I1)
    } else if cross_module.typed_string_closures.contains(&func_id) {
        Some(TypedFunctionTrampolineKind::StringRef)
    } else {
        None
    };
    let llvm_name = if typed_public_trampoline.is_some() {
        generic_closure_body_name(&public_llvm_name)
    } else {
        public_llvm_name.clone()
    };

    // Param list: i64 this_closure, then each param as double.
    let mut llvm_params: Vec<(LlvmType, String)> = Vec::with_capacity(params.len() + 1);
    llvm_params.push((I64, "%this_closure".to_string()));
    for p in params {
        llvm_params.push((DOUBLE, format!("%arg{}", p.id)));
    }

    let ic_base = llmod.ic_counter;
    let buffer_alias_base = llmod.buffer_alias_counter;
    let lf = llmod.define_function(&llvm_name, DOUBLE, llvm_params);
    if typed_public_trampoline.is_some() {
        lf.linkage = "internal".to_string();
    }

    // gh #6206 / #6081: closures/arrows compiled WITHOUT a shadow frame left
    // their pointer-typed params/locals invisible to the exact-roots copying
    // minor (production skips the conservative native-stack scan), so an
    // evacuating GC fired mid-body swept values reachable only from the
    // closure's own frame — the referrer then read freed-and-reused memory.
    // Emit the same frame the top-level function path gets (function.rs).
    let shadow_slot_map = if super::helpers::shadow_stack_enabled() {
        let flat_const_ids: std::collections::HashSet<u32> =
            cross_module.flat_const_arrays.keys().copied().collect();
        let m = crate::collectors::collect_pointer_typed_locals(params, body, &flat_const_ids);
        lf.enable_shadow_frame(m.len() as u32);
        m
    } else {
        std::collections::HashMap::new()
    };
    let shadow_slot_clears_after_stmt =
        crate::collectors::collect_shadow_slot_clear_points(body, &shadow_slot_map);

    let _ = lf.create_block("entry");

    let mut closure_boxed_vars = module_boxed_vars.clone();
    super::arguments::add_arguments_mapped_boxes(params, &mut closure_boxed_vars);

    // Allocate slots for the closure's own params (captures don't get
    // alloca slots — they're accessed via the runtime).
    let locals: HashMap<u32, String> = {
        let blk = lf.block_mut(0).unwrap();
        let mut map = HashMap::new();
        for p in params {
            let arg_name = format!("%arg{}", p.id);
            let slot = super::arguments::store_param_slot(blk, p, &closure_boxed_vars, &arg_name);
            if let Some(slot_idx) = shadow_slot_map.get(&p.id).copied() {
                blk.call_void(
                    "js_shadow_slot_bind",
                    &[(I32, &slot_idx.to_string()), (PTR, &slot)],
                );
            }
            map.insert(p.id, slot);
        }
        map
    };

    // Start with the closure's own params as local_types, then
    // merge in the module-wide map so captured-from-outer ids have
    // their types available inside the body. Without this, closures
    // that capture an array `items` and do `items.length` miss the
    // typed fast path and return undefined.
    let mut local_types: HashMap<u32, perry_types::Type> =
        params.iter().map(|p| (p.id, p.ty.clone())).collect();
    for (id, ty) in module_receiver_types.iter() {
        local_types.entry(*id).or_insert_with(|| ty.clone());
    }

    // Build the capture map: each captured LocalId gets the index it
    // occupies in the closure's capture array. Identical logic to the
    // `compute_auto_captures` helper used by the closure creation site
    // — they MUST agree on the slot indices, otherwise the body reads
    // captures from the wrong slots. Sorting the auto-detected ids
    // gives deterministic indexing across both call sites.
    //
    // Filter module globals out of the explicit captures list — same
    // reason as in `compute_auto_captures` (closures auto-load module
    // globals through `@perry_global_*`). Without this, the body and
    // creation sites disagree on capture indices and a globalized
    // block-scoped let captured by a closure ends up with a
    // value-instead-of-box-pointer in its capture slot.
    let mut auto_captures: Vec<u32> = captures
        .iter()
        .copied()
        .filter(|id| !module_globals.contains_key(id))
        .collect();
    {
        let mut referenced: std::collections::HashSet<u32> = std::collections::HashSet::new();
        collect_ref_ids_in_stmts(body, &mut referenced);
        let mut inner_lets: std::collections::HashSet<u32> = std::collections::HashSet::new();
        collect_let_ids(body, &mut inner_lets);
        let param_ids: std::collections::HashSet<u32> = params.iter().map(|p| p.id).collect();
        let already: std::collections::HashSet<u32> = auto_captures.iter().copied().collect();
        let mut sorted: Vec<u32> = referenced.into_iter().collect();
        sorted.sort();
        for id in sorted {
            if !param_ids.contains(&id)
                && !inner_lets.contains(&id)
                && !already.contains(&id)
                && !module_globals.contains_key(&id)
            {
                auto_captures.push(id);
            }
        }
    }
    let closure_captures: HashMap<u32, u32> = auto_captures
        .iter()
        .enumerate()
        .map(|(i, id)| (*id, i as u32))
        .collect();

    // `this` capture. Object-literal methods get `captures_this=true`
    // AND the creation site (lower_object_literal) patches a reserved
    // capture slot at index `auto_captures.len()` with the containing
    // object pointer. At function entry we read that slot and store it
    // into the `this` alloca so `Expr::This` loads the real receiver.
    //
    // Arrow-in-class leftover path (`enclosing_class.is_some()` without
    // the object-literal patch) keeps the old 0.0 sentinel — reads
    // return a bogus value but don't crash.
    let new_target_stack = if captures_new_target {
        let new_target_cap_idx = auto_captures.len() as u32;
        let blk = lf.block_mut(0).unwrap();
        let slot = blk.alloca(DOUBLE);
        let idx_str = new_target_cap_idx.to_string();
        let bits = blk.call(
            I64,
            "js_closure_get_capture_bits",
            &[(I64, "%this_closure"), (I32, &idx_str)],
        );
        let v = blk.bitcast_i64_to_double(&bits);
        blk.store(DOUBLE, &v, &slot);
        vec![slot]
    } else {
        Vec::new()
    };

    let this_stack = if captures_this || enclosing_class.is_some() {
        let this_cap_idx = (auto_captures.len() + usize::from(captures_new_target)) as u32;
        let blk = lf.block_mut(0).unwrap();
        let slot = blk.alloca(DOUBLE);
        if captures_this {
            let idx_str = this_cap_idx.to_string();
            let bits = blk.call(
                I64,
                "js_closure_get_capture_bits",
                &[(I64, "%this_closure"), (I32, &idx_str)],
            );
            let v = blk.bitcast_i64_to_double(&bits);
            blk.store(DOUBLE, &v, &slot);
        } else {
            blk.store(DOUBLE, "0.0", &slot);
        }
        vec![slot]
    } else {
        Vec::new()
    };
    let class_stack = match enclosing_class.clone() {
        Some(c) => vec![c],
        None => Vec::new(),
    };

    // Boxed vars inside the closure body: mutable captures from the
    // closure's own let-bindings. We don't add the captured-from-outer
    // ids here because those are already boxed in the outer function;
    // the closure body just sees them via the capture mechanism.
    let clamp_fn_ids: std::collections::HashSet<u32> = cross_module
        .clamp3_functions
        .union(&cross_module.clamp_u8_functions)
        .chain(cross_module.returns_int_functions.iter())
        .copied()
        .collect();
    let flat_const_ids: std::collections::HashSet<u32> =
        cross_module.flat_const_arrays.keys().copied().collect();
    let native_facts = crate::collectors::collect_native_region_fact_graph(
        body,
        &[],
        &flat_const_ids,
        &clamp_fn_ids,
        &cross_module.clamp3_functions,
        &closure_boxed_vars,
        module_globals,
        // #6369: declared types of module-scope bindings this closure captures.
        &local_types,
        classes,
        &cross_module.compile_time_constants,
        &cross_module.module_dispatch,
    );

    let mut ctx = FnCtx {
        func: lf,
        module_slug: crate::expr::native_region_slug(strings.module_prefix()),
        source_function: format!("closure_{}", func_id),
        source_function_slug: crate::expr::native_region_slug(&format!("closure_{}", func_id)),
        active_region_id: None,
        native_facts: &native_facts,
        locals,
        local_types,
        const_string_locals: std::collections::HashMap::new(),
        const_number_locals: std::collections::HashMap::new(),
        current_block: 0,
        discard_expr_value: false,
        func_names,
        strings,
        loop_targets: Vec::new(),
        label_targets: HashMap::new(),
        pending_labels: Vec::new(),
        classes,
        this_stack,
        new_target_stack,
        class_stack,
        inline_ctor_return: Vec::new(),
        methods,
        module_globals,
        import_function_prefixes,
        import_function_origin_names: &cross_module.import_function_origin_names,
        import_function_v8_specifiers: &cross_module.import_function_v8_specifiers,
        // Issue #841: node:submodule named-import + namespace registries.
        import_function_node_submodule: &cross_module.import_function_node_submodule,
        namespace_node_submodules: &cross_module.namespace_node_submodules,
        namespace_v8_specifiers: &cross_module.namespace_v8_specifiers,
        closure_captures,
        current_closure_ptr: Some("%this_closure".to_string()),
        enums,
        // Async closures (arrow functions declared `async () => ...`)
        // must wrap their return values in `js_promise_resolved` so the
        // call site sees a NaN-boxed Promise pointer — same contract as
        // regular async functions. Consumers like the Fastify server
        // runtime inspect the returned value with `js_is_promise` and
        // break if a raw object pointer (or any non-Promise) is handed
        // back. Issue #125.
        is_async_fn: is_async,
        is_strict_fn: is_strict,
        static_field_globals,
        class_ids,
        class_keys_globals: &cross_module.class_keys_globals,
        class_field_counts: &cross_module.class_field_counts,
        class_init_chains: &cross_module.class_init_chains,
        imported_class_ctors: &cross_module.imported_class_ctors,
        func_signatures,
        func_synthetic_arguments,
        func_returns_class: &cross_module.func_returns_class,
        boxed_vars: closure_boxed_vars,
        prealloc_boxes: std::collections::HashSet::new(),
        tdz_boxes: std::collections::HashSet::new(),
        compiler_private_async_i32_control_locals: &cross_module
            .compiler_private_async_i32_control_locals,
        compiler_private_async_i1_control_locals: &cross_module
            .compiler_private_async_i1_control_locals,
        closure_rest_params,
        local_closure_func_ids: HashMap::new(),
        local_closure_param_counts: HashMap::new(),
        option_object_locals: HashMap::new(),
        object_literal_locals: HashSet::new(),
        namespace_imports: &cross_module.namespace_imports,
        namespace_member_prefixes: &cross_module.namespace_member_prefixes,
        namespace_member_origin_names: &cross_module.namespace_member_origin_names,
        imported_async_funcs: &cross_module.imported_async_funcs,
        local_async_funcs: &cross_module.local_async_funcs,
        local_generator_funcs: &cross_module.local_generator_funcs,
        async_step_closures: &cross_module.async_step_closures,
        funcs_reading_dynamic_this: &cross_module.funcs_reading_dynamic_this,
        type_aliases: &cross_module.type_aliases,
        imported_func_param_counts: &cross_module.imported_func_param_counts,
        imported_func_has_rest: &cross_module.imported_func_has_rest,
        imported_func_synthetic_arguments: &cross_module.imported_func_synthetic_arguments,
        method_param_counts: &cross_module.method_param_counts,
        method_has_rest: &cross_module.method_has_rest,
        imported_func_return_types: &cross_module.imported_func_return_types,
        ffi_signatures: &cross_module.ffi_signatures,
        ffi_aliases: &cross_module.ffi_aliases,
        imported_class_sources: &cross_module.imported_class_sources,
        imported_class_original_names: &cross_module.imported_class_original_names,
        interfaces: &cross_module.interfaces,
        try_depth: 0,
        pending_declares: Vec::new(),
        integer_locals: native_facts.integer_locals(),
        unsigned_i32_locals: native_facts.unsigned_i32_locals(),
        shadow_slot_map,
        persistent_shadow_slots: std::collections::HashSet::new(),
        shadow_slot_clears_after_stmt,
        arena_state_slot: None,
        class_keys_slots: HashMap::new(),
        cached_lengths: HashMap::new(),
        bounded_index_pairs: Vec::new(),
        packed_f64_loop_facts: Vec::new(),
        masked_window_array_facts: Vec::new(),
        masked_region_scalar_locals: std::collections::HashSet::new(),
        class_field_loop_facts: Vec::new(),
        i32_counter_slots: HashMap::new(),
        i1_local_slots: HashMap::new(),
        index_used_locals: native_facts.index_used_locals(),
        strictly_i32_bounded_locals: native_facts.strictly_i32_bounded_locals(),
        i18n: &cross_module.i18n,
        dynamic_import_path_to_prefix: &cross_module.dynamic_import_path_to_prefix,
        local_class_aliases: HashMap::new(),
        local_class_field_aliases: HashMap::new(),
        local_id_to_name: HashMap::new(),
        local_value_aliases: HashMap::new(),
        imported_vars: &cross_module.imported_vars,
        compile_time_constants: native_facts.compile_time_constants(),
        target_triple: &cross_module.target_triple,
        app_metadata: &cross_module.app_metadata,
        scalar_replaced: std::collections::HashMap::new(),
        pod_records: std::collections::HashMap::new(),
        pod_views: std::collections::HashMap::new(),
        scalar_replaced_arrays: std::collections::HashMap::new(),
        scalar_replaced_split_part_lengths: std::collections::HashMap::new(),
        scalar_replaced_uppercase_sources: std::collections::HashMap::new(),
        scalar_ctor_target: Vec::new(),
        non_escaping_news: native_facts.non_escaping_news().clone(),
        non_escaping_new_used_fields: native_facts.non_escaping_new_used_fields().clone(),
        non_escaping_arrays: native_facts.non_escaping_arrays().clone(),
        non_escaping_array_used_indices: native_facts.non_escaping_array_used_indices().clone(),
        non_escaping_array_length_only_indices: native_facts
            .non_escaping_array_length_only_indices()
            .clone(),
        fusible_uppercase_locals: native_facts.fusible_uppercase_locals().clone(),
        non_escaping_object_literals: native_facts.non_escaping_object_literals().clone(),
        non_escaping_object_literal_used_fields: native_facts
            .non_escaping_object_literal_used_fields()
            .clone(),
        flat_const_arrays: &cross_module.flat_const_arrays,
        array_row_aliases: HashMap::new(),
        clamp3_functions: &cross_module.clamp3_functions,
        clamp_u8_functions: &cross_module.clamp_u8_functions,
        integer_returning_functions: &cross_module.returns_int_functions,
        i32_identity_functions: &cross_module.i32_identity_functions,
        typed_f64_functions: &cross_module.typed_f64_functions,
        typed_i32_functions: &cross_module.typed_i32_functions,
        typed_string_functions: &cross_module.typed_string_functions,
        typed_i1_functions: &cross_module.typed_i1_functions,
        typed_i1_function_param_reps: &cross_module.typed_i1_function_param_reps,
        typed_f64_methods: &cross_module.typed_f64_methods,
        typed_i32_methods: &cross_module.typed_i32_methods,
        typed_i1_methods: &cross_module.typed_i1_methods,
        typed_string_methods: &cross_module.typed_string_methods,
        typed_i1_method_param_reps: &cross_module.typed_i1_method_param_reps,
        typed_f64_closures: &cross_module.typed_f64_closures,
        typed_i32_closures: &cross_module.typed_i32_closures,
        typed_i1_closures: &cross_module.typed_i1_closures,
        typed_i1_closure_param_reps: &cross_module.typed_i1_closure_param_reps,
        typed_string_closures: &cross_module.typed_string_closures,
        typed_string_closure_capture_counts: &cross_module.typed_string_closure_capture_counts,
        was_unrolled: false,
        ic_site_counter: ic_base,
        ic_globals: Vec::new(),
        typed_parse_rodata: Vec::new(),
        typed_parse_counter: 0,
        buffer_data_slots: HashMap::new(),
        buffer_view_slots: HashMap::new(),
        native_arena_owner_aliases: HashMap::new(),
        native_arena_ambiguous_owner_aliases: HashSet::new(),
        disable_buffer_fast_path: cross_module.disable_buffer_fast_path,
        program_shadows_buffer_read_method: cross_module.program_shadows_buffer_read_method,
        min_length_bounds: HashMap::new(),
        bounded_buffer_index_pairs: Vec::new(),
        guarded_buffer_index_pairs: Vec::new(),
        buffer_hazard_reasons: HashMap::new(),
        native_i32_aliases: HashMap::new(),
        int_range_aliases: HashMap::new(),
        int_range_facts: Vec::new(),
        next_loop_proof_scope_id: 0,
        nonnegative_integer_locals: HashSet::new(),
        native_rep_records: Vec::new(),
        known_noalias_buffer_locals: native_facts.known_noalias_buffer_locals(),
        buffer_alias_base,
    };

    super::arguments::materialize_arguments_object(
        &mut ctx,
        params,
        super::arguments::ArgumentsCallee::CurrentClosure,
    );

    if is_async {
        stmt::lower_async_rejecting_stmts(&mut ctx, body)
            .with_context(|| format!("lowering async closure body func_id={}", func_id))?;
    } else {
        stmt::lower_stmts(&mut ctx, body)
            .with_context(|| format!("lowering closure body func_id={}", func_id))?;
    }

    if !ctx.block().is_terminated() {
        let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        if is_async {
            let handle = ctx
                .block()
                .call(I64, "js_promise_resolved", &[(DOUBLE, &undef)]);
            let boxed = crate::expr::nanbox_pointer_inline_pub(ctx.block(), &handle);
            ctx.block().ret(DOUBLE, &boxed);
        } else {
            ctx.block().ret(DOUBLE, &undef);
        }
    }
    let ic_globals = std::mem::take(&mut ctx.ic_globals);
    let typed_parse_rodata = std::mem::take(&mut ctx.typed_parse_rodata);
    let ic_end = ctx.ic_site_counter;
    let pending = std::mem::take(&mut ctx.pending_declares);
    let buffer_alias_used = ctx.buffer_data_slots.len() as u32;
    let native_rep_records = std::mem::take(&mut ctx.native_rep_records);
    drop(ctx);
    llmod.ic_counter = ic_end;
    llmod.buffer_alias_counter += buffer_alias_used;
    llmod.native_rep_records.extend(native_rep_records);
    for (name, ret, params) in pending {
        llmod.declare_function(&name, ret, &params);
    }
    for ic_name in &ic_globals {
        llmod.add_raw_global(format!(
            "@{} = private global [2 x i64] zeroinitializer",
            ic_name
        ));
    }
    for raw in &typed_parse_rodata {
        llmod.add_raw_global(raw.clone());
    }
    if let Some(kind) = typed_public_trampoline {
        let string_capture_count = if matches!(kind, TypedFunctionTrampolineKind::StringRef) {
            cross_module
                .typed_string_closure_capture_counts
                .get(&func_id)
                .copied()
                .unwrap_or(0)
        } else {
            0
        };
        emit_public_typed_closure_trampoline(
            llmod,
            func_id,
            closure_expr,
            module_prefix,
            &llvm_name,
            kind,
            string_capture_count,
        )?;
    }
    Ok(())
}
