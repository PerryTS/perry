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
    func_id: perry_hir::types::FuncId,
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
    func_id: perry_hir::types::FuncId,
    closure_expr: &perry_hir::Expr,
    module_prefix: &str,
    module_local_types: &HashMap<u32, perry_hir::types::Type>,
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
    func_id: perry_hir::types::FuncId,
    closure_expr: &perry_hir::Expr,
    module_prefix: &str,
    module_local_types: &HashMap<u32, perry_hir::types::Type>,
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
    func_id: perry_hir::types::FuncId,
    closure_expr: &perry_hir::Expr,
    module_prefix: &str,
    module_local_types: &HashMap<u32, perry_hir::types::Type>,
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
    func_id: perry_hir::types::FuncId,
    closure_expr: &perry_hir::Expr,
    module_prefix: &str,
    module_local_types: &HashMap<u32, perry_hir::types::Type>,
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
    func_id: perry_hir::types::FuncId,
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
    module_receiver_types: &HashMap<u32, perry_hir::types::Type>,
    // Reassignments from every executable body in the module. Captured locals
    // inherit module-wide receiver types, so their invalidation scope must be
    // module-wide too.
    module_reassigned_locals: &HashSet<u32>,
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
    let mut local_types: HashMap<u32, perry_hir::types::Type> =
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
    // `--opt-report` (#6952): closures are the position #7034 §8 found most
    // of the guard sites in, so they get their own scope with the source
    // function name when one is known.
    let opt_report_name = func_names
        .get(&func_id)
        .cloned()
        .unwrap_or_else(|| format!("closure#{func_id}"));
    let _opt_report_scope = crate::opt_report::enter_closure(&opt_report_name, func_id);
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

    // Representation-selection Phase 1 context gate (see codegen/function.rs).
    // Async-step closures (CPS-rewritten `async` closures — the rewrite clears
    // `is_async`) and generator wrapper funcs route body locals through shared
    // cells, so canonical-i32 storage is disallowed there.
    let repsel_allows = crate::expr::canonical_i32_locals_enabled()
        && !is_async
        && !cross_module.async_step_closures.contains(&func_id)
        && !cross_module.local_generator_funcs.contains(&func_id);
    // Phase 3a: same context restrictions, independent env gate.
    let repsel_str_allows = crate::expr::canonical_str_locals_enabled()
        && !is_async
        && !cross_module.async_step_closures.contains(&func_id)
        && !cross_module.local_generator_funcs.contains(&func_id);
    let repsel_closure_refs = if repsel_allows || repsel_str_allows {
        crate::expr::collect_closure_referenced_locals(body)
    } else {
        std::collections::HashSet::new()
    };
    let repsel_str_ineligible = if repsel_str_allows {
        crate::expr::collect_canonical_str_ineligible_locals(body)
    } else {
        std::collections::HashSet::new()
    };

    let mut reassigned_locals = module_reassigned_locals.clone();
    reassigned_locals.extend(crate::collectors::reassigned_locals(body));

    // #7055: spill the closure's own `%this_closure` pointer into a
    // shadow-rooted entry alloca and read every capture back through it.
    //
    // `%this_closure` is an LLVM parameter — a register value no root
    // enumeration can see. The shipped moving young collection runs at a loop
    // back-edge poll (`js_gc_loop_safepoint`) with PRECISE roots and no
    // conservative native-stack scan, so a closure relocated while its own body
    // is running leaves that register pointing into from-space. From-space is
    // reset at the end of the same cycle and immediately reused by the mutator,
    // after which `js_closure_get_capture_bits` reads a foreign object's
    // `capture_count`, decides the index is out of range, and returns **0** —
    // turning every later boxed-capture read into `undefined` and every write
    // into a silent no-op. In an `async fn` that swallowed the generator's own
    // `__gen_state` store, so the next `await` resumed into the state it had
    // just finished and one loop iteration ran twice.
    //
    // Rooting it here makes the closure a first-class precise root: the
    // collector rewrites this slot along with every other shadow slot, and
    // `current_closure_ptr_value` reloads from it at each capture access.
    //
    // Only closures that actually read captures pay for it. A capture-less
    // closure (`(a, b) => a - b` handed to `sort`) never emits a
    // `js_closure_get_capture_bits` call in its body, so the pointer is dead on
    // arrival — and reserving a slot there would force a `js_shadow_frame_push`
    // /`pop` pair onto bodies that need no frame at all. The `this` /
    // `new.target` capture reads are exempt for a different reason: they run in
    // the entry-block prologue, ahead of any statement that could collect.
    let current_closure_slot = if closure_captures.is_empty() {
        None
    } else {
        lf.reserve_shadow_slot().map(|idx| {
            let blk = lf.block_mut(0).expect("closure body has an entry block");
            let slot = blk.alloca(I64);
            let tagged = blk.or(I64, "%this_closure", crate::nanbox::POINTER_TAG_I64);
            blk.store(I64, &tagged, &slot);
            blk.call_void(
                "js_shadow_slot_bind",
                &[(I32, &idx.to_string()), (PTR, &slot)],
            );
            slot
        })
    };

    let mut ctx = FnCtx {
        func: lf,
        module_slug: crate::expr::native_region_slug(strings.module_prefix()),
        source_function: format!("closure_{}", func_id),
        source_function_slug: crate::expr::native_region_slug(&format!("closure_{}", func_id)),
        active_region_id: None,
        native_facts: &native_facts,
        locals,
        local_types,
        reassigned_locals,
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
        current_closure_slot,
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
        not_bigint_locals: native_facts.not_bigint_locals(),
        unsigned_i32_locals: native_facts.unsigned_i32_locals(),
        // Conservative: treat every slot as possibly-bound (param binds are
        // emitted before FnCtx exists here), so clears never get skipped.
        shadow_slots_bound: shadow_slot_map.values().copied().collect(),
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
        suppressed_cleared_shadow_slots: std::collections::HashSet::new(),
        class_field_loop_facts: Vec::new(),
        i32_counter_slots: HashMap::new(),
        local_slot_reps: HashMap::new(),
        repsel_context_allows_canonical_i32: repsel_allows,
        repsel_closure_ref_locals: repsel_closure_refs,
        repsel_context_allows_canonical_str: repsel_str_allows,
        repsel_str_ineligible_locals: repsel_str_ineligible,
        spec_abi_functions: &cross_module.spec_abi_functions,
        spec_ta_bindings: &cross_module.spec_ta_bindings,
        spec_ta_ready: std::collections::HashSet::new(),
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
        scalar_slot_shadow_slots: std::collections::HashMap::new(),
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
        pshape_methods: &cross_module.pshape_methods,
        proven_this: None,
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
            "@{} = private global [8 x i64] zeroinitializer",
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

#[cfg(test)]
mod tests {
    use perry_hir::types::Type;
    use perry_hir::{Expr, Function, Module as HirModule, Stmt, UpdateOp};

    /// Compile a one-closure module to LLVM IR text.
    ///
    /// `outer() { let x; const f = () => { x = 2; x++; return x; }; return f; }`
    /// — the smallest shape that exercises all three capture accessors: a read
    /// (`js_closure_get_capture_bits`), a write
    /// (`js_closure_set_capture_bits`), and a read-modify-write whose coercion
    /// (`js_to_numeric`) can run a user `valueOf` and therefore collect between
    /// the read and the write.
    fn one_capture_closure_ir() -> String {
        let closure = Expr::Closure {
            func_id: 1,
            params: Vec::new(),
            return_type: Type::Any,
            body: vec![
                Stmt::Expr(Expr::LocalSet(0, Box::new(Expr::Number(2.0)))),
                Stmt::Expr(Expr::Update {
                    id: 0,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                Stmt::Return(Some(Expr::LocalGet(0))),
            ],
            captures: vec![0],
            mutable_captures: vec![0],
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: true,
            is_async: false,
            is_generator: false,
            is_strict: true,
        };
        let mut hir = HirModule::new("closure_self_root_test");
        hir.functions.push(Function {
            id: 0,
            name: "outer".to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Any,
            body: vec![
                Stmt::Let {
                    id: 0,
                    name: "x".to_string(),
                    ty: Type::Any,
                    mutable: true,
                    init: None,
                },
                Stmt::Let {
                    id: 2,
                    name: "f".to_string(),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(closure),
                },
                Stmt::Return(Some(Expr::LocalGet(2))),
            ],
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        });
        let opts = crate::CompileOptions {
            emit_ir_only: true,
            ..Default::default()
        };
        let bytes = crate::compile_module(&hir, opts).expect("closure test module compiles");
        String::from_utf8(bytes).expect("LLVM IR is UTF-8")
    }

    /// Same module, but the captured id has **no declaration site** in the
    /// enclosing function, so `collect_boxed_vars` does not box it and the
    /// closure body takes the *unboxed* capture accessors — the pair whose
    /// writer (`js_closure_set_capture_bits`) does no bounds check at all.
    fn unboxed_capture_update_ir() -> String {
        let closure = Expr::Closure {
            func_id: 1,
            params: Vec::new(),
            return_type: Type::Any,
            body: vec![
                Stmt::Expr(Expr::Update {
                    id: 0,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                Stmt::Return(Some(Expr::LocalGet(0))),
            ],
            captures: vec![0],
            mutable_captures: Vec::new(),
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: true,
            is_async: false,
            is_generator: false,
            is_strict: true,
        };
        let mut hir = HirModule::new("closure_unboxed_update_test");
        hir.functions.push(Function {
            id: 0,
            name: "outer".to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Any,
            body: vec![
                Stmt::Let {
                    id: 2,
                    name: "f".to_string(),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(closure),
                },
                Stmt::Return(Some(Expr::LocalGet(2))),
            ],
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        });
        let opts = crate::CompileOptions {
            emit_ir_only: true,
            ..Default::default()
        };
        let bytes = crate::compile_module(&hir, opts).expect("unboxed-capture module compiles");
        String::from_utf8(bytes).expect("LLVM IR is UTF-8")
    }

    /// #7055: the closure's own `%this_closure` pointer must be a PRECISE GC
    /// root, and capture accesses must read it back from that root.
    ///
    /// `%this_closure` is an LLVM parameter, i.e. a register the collector
    /// cannot see. The shipped moving young collection runs at loop back-edge
    /// polls with precise roots and no conservative native-stack scan, so a
    /// closure relocated while its own body runs leaves that register pointing
    /// into from-space — which is reset and reused before the body's next
    /// capture access. `js_closure_get_capture_bits` then reads a foreign
    /// object's `capture_count`, judges the index out of range and returns 0,
    /// so every later boxed-capture read yields `undefined` and every write is
    /// dropped.
    ///
    /// Teeth: with the fix reverted the body reads captures straight off the
    /// parameter and the `js_closure_get_capture_bits(i64 %this_closure`
    /// assertion below fails.
    #[test]
    fn closure_body_roots_its_own_closure_pointer_and_reads_captures_through_it() {
        let ir = one_capture_closure_ir();
        // The public `perry_closure_*` symbol can be a typed trampoline over a
        // straight-line `__typed_f64` clone; the real body is the one that
        // carries a shadow frame. (The typed clone lowers arithmetic-only,
        // loop-free, call-free statements — `lower_typed_f64_body_*` bails on
        // anything else — so it contains no safepoint and its `%this_closure`
        // register cannot go stale.)
        let body = ir
            .split("define ")
            .find(|f| {
                let name_starts_here = f.starts_with("double @perry_closure_")
                    || f.starts_with("internal double @perry_closure_");
                name_starts_here && f.contains("@js_shadow_frame_enter")
            })
            .unwrap_or_else(|| panic!("no shadow-framed closure body in IR:\n{ir}"));

        // The prologue NaN-boxes `%this_closure` into an alloca and binds that
        // alloca to a shadow-stack slot, so the collector marks and rewrites it.
        let tagged = format!("or i64 %this_closure, {}", crate::nanbox::POINTER_TAG_I64);
        assert!(
            body.contains(&tagged),
            "closure prologue must NaN-box %this_closure for the shadow slot; body:\n{body}"
        );
        assert!(
            body.contains("@js_shadow_slot_bind"),
            "closure prologue must bind the closure-pointer slot as a GC root; body:\n{body}"
        );

        // And no capture access may use the raw (unrooted) parameter.
        assert!(
            !body.contains("@js_closure_get_capture_bits(i64 %this_closure"),
            "capture reads must reload the closure pointer from its rooted \
             slot, not use the %this_closure register; body:\n{body}"
        );
        // Belt and braces, and honestly labelled: this fixture does NOT emit the
        // unboxed capture writer. `collect_boxed_vars` boxes every declared
        // local a nested closure mutates (and `collect_boxed_param_ids` covers
        // params), so a mutating capture always lowers to `get_capture_bits` +
        // `js_box_set_bits`, never to `js_closure_set_capture_bits`. This
        // assertion guards the unboxed writer against a future change to that
        // boxing rule; it is not evidence about today's output.
        assert!(
            !body.contains("@js_closure_set_capture_bits(i64 %this_closure"),
            "capture writes must reload the closure pointer from its rooted \
             slot, not use the %this_closure register; body:\n{body}"
        );

        // Not vacuous: the fixture really does read, write and read-modify-write
        // the capture, and the read-modify-write really does emit the ToNumeric
        // coercion — the collect-capable call the reload below has to survive.
        let accesses = body.matches("@js_closure_get_capture_bits(").count();
        assert!(
            accesses >= 2,
            "fixture must access the capture more than once (read + write), or \
             the per-access reload assertion below is vacuous; body:\n{body}"
        );
        assert!(
            body.contains("@js_box_set_bits"),
            "fixture must WRITE the capture, not only read it; body:\n{body}"
        );
        assert!(
            body.contains("@js_to_numeric"),
            "the captured `x++` must emit its ToNumeric coercion; body:\n{body}"
        );

        // The invariant, stated positively: EVERY capture access re-reads the
        // rooted slot, so no access can be reached through a pointer loaded
        // before an intervening collection. Pre-fix this count is 0.
        let slot = {
            let bind = body
                .find("@js_shadow_slot_bind(")
                .map(|i| &body[i..])
                .and_then(|t| t.split_once("ptr "))
                .map(|(_, rest)| rest)
                .unwrap_or_else(|| panic!("no shadow-slot bind in body:\n{body}"));
            bind.split(')').next().expect("bind operand").to_string()
        };
        let reload = format!("load i64, ptr {slot}");
        assert!(
            body.matches(reload.as_str()).count() >= accesses,
            "every one of the {accesses} capture accesses must reload the \
             closure pointer from its rooted slot {slot}; body:\n{body}"
        );

        // #7055 (CodeRabbit): the sharp case — `js_to_numeric` runs a user
        // `valueOf`, i.e. arbitrary JS that can reach a loop poll and relocate
        // this closure. The capture access that FOLLOWS it must come from a
        // fresh load, never from a pointer materialized before the call.
        let coerce_at = body.find("@js_to_numeric").expect("coercion call");
        let next_access = body[coerce_at..]
            .find("@js_closure_get_capture_bits(")
            .map(|i| coerce_at + i)
            .unwrap_or_else(|| {
                panic!("fixture must access the capture after the coercion; body:\n{body}")
            });
        assert!(
            body[coerce_at..next_access].contains(reload.as_str()),
            "a capture access after `js_to_numeric` (which can run a user \
             `valueOf` and relocate the closure) must re-read the closure \
             pointer from its rooted slot {slot}; body:\n{body}"
        );
    }

    /// #7055 (CodeRabbit 🟠 Major): `js_to_numeric` runs a user `valueOf` —
    /// arbitrary JS that can reach a `js_gc_loop_safepoint` and relocate this
    /// closure — and the *unboxed* capture writer
    /// `js_closure_set_capture_bits` does NOT validate its pointer (unlike the
    /// reader, which bounds-checks and returns 0). Writing through a closure
    /// pointer materialized before the coercion would therefore store into
    /// whatever the mutator has since placed at that recycled from-space
    /// address. The write must use a pointer re-read from the rooted slot.
    ///
    /// Reachability, stated plainly: today `collect_boxed_vars` boxes every
    /// declared local a nested closure mutates and `collect_boxed_param_ids`
    /// covers params, so TypeScript cannot currently produce a mutating
    /// *unboxed* capture — the fixture reaches this arm by omitting the
    /// declaration site. The arm is live code with a memory-corrupting failure
    /// mode if that boxing rule ever narrows, which is what this pins.
    #[test]
    fn unboxed_capture_write_reloads_the_closure_pointer_after_the_coercion() {
        let ir = unboxed_capture_update_ir();
        let body = ir
            .split("define ")
            .find(|f| {
                let name_starts_here = f.starts_with("double @perry_closure_")
                    || f.starts_with("internal double @perry_closure_");
                name_starts_here && f.contains("@js_closure_set_capture_bits(")
            })
            .unwrap_or_else(|| panic!("no unboxed capture write in IR:\n{ir}"));

        // Non-vacuous: this really is the unboxed arm, and the coercion really
        // is emitted between the capture read and the capture write.
        assert!(
            !body.contains("@js_box_set_bits"),
            "fixture must take the UNBOXED capture arm; body:\n{body}"
        );
        let coerce_at = body
            .find("@js_to_numeric")
            .unwrap_or_else(|| panic!("captured `x++` must emit its coercion; body:\n{body}"));
        let write_at = body
            .find("@js_closure_set_capture_bits(")
            .expect("capture write");
        assert!(
            coerce_at < write_at,
            "fixture must coerce before it writes; body:\n{body}"
        );

        let slot = {
            let bind = body
                .find("@js_shadow_slot_bind(")
                .map(|i| &body[i..])
                .and_then(|t| t.split_once("ptr "))
                .map(|(_, rest)| rest)
                .unwrap_or_else(|| panic!("no shadow-slot bind in body:\n{body}"));
            bind.split(')').next().expect("bind operand").to_string()
        };
        assert!(
            body[coerce_at..write_at].contains(&format!("load i64, ptr {slot}")),
            "the unboxed capture write must re-read the closure pointer from \
             its rooted slot {slot} after `js_to_numeric`; body:\n{body}"
        );
    }
}
