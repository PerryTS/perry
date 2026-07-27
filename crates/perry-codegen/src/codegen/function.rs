//! User-function compilation. Split out of `codegen.rs` (now
//! `codegen/mod.rs`). Behavior is unchanged — this only contains
//! `compile_function`.

use std::collections::{HashMap, HashSet};

use anyhow::{anyhow, Context, Result};
use perry_hir::Function;

use crate::expr::FnCtx;
use crate::module::LlModule;
use crate::native_value::{AliasState, BufferElem, BufferIndexUnit, BufferViewSlot, LengthSource};
use crate::stmt;
use crate::strings::StringPool;
use crate::types::{LlvmType, DOUBLE, I1, I32, I64, I8, PTR};

use super::helpers::shadow_stack_enabled;
use super::helpers::{inline_hot_small_enabled, inline_hot_small_size_cap, INLINE_HOT_SMALL_MIN};
use super::opts::CrossModuleCtx;
use super::spec_abi::{
    spec_function_name, spec_rep_llvm_ty, spec_ta_kind_class_name, spec_ta_kind_elem_width,
    SpecFnPlan,
};
use super::typed_abi::{
    emit_typed_arg_guard, emit_typed_arg_to_raw, generic_function_body_name, lower_typed_f64_body,
    lower_typed_i1_body, lower_typed_i32_body, lower_typed_string_body, typed_f64_function_name,
    typed_i1_function_name, typed_i32_function_name, typed_param_reps_for_params,
    typed_string_function_name, TypedFunctionTrampolineKind, TypedParamRep,
};

/// Compile the internal typed-f64 clone for a conservatively eligible user
/// function. `compile_function` emits both the public JSValue trampoline and
/// the internal generic fallback body; guarded direct FuncRef sites can still
/// call this clone directly.
pub(super) fn compile_typed_f64_function(
    llmod: &mut LlModule,
    f: &Function,
    func_names: &HashMap<u32, String>,
) -> Result<()> {
    let generic_name = func_names
        .get(&f.id)
        .cloned()
        .ok_or_else(|| anyhow!("function name not resolved for {}", f.name))?;
    let llvm_name = typed_f64_function_name(&generic_name);
    let param_reps = typed_param_reps_for_params(&f.params)
        .ok_or_else(|| anyhow!("typed-f64 function '{}' has unsupported parameter", f.name))?;
    let params: Vec<(LlvmType, String)> = f
        .params
        .iter()
        .zip(param_reps.iter())
        .map(|(p, rep)| (rep.llvm_ty(), format!("%arg{}", p.id)))
        .collect();
    let lf = llmod.define_function(&llvm_name, DOUBLE, params);
    lf.linkage = "internal".to_string();
    lf.force_inline = true;
    let _ = lf.create_block("entry");

    let value = {
        let blk = lf.block_mut(0).unwrap();
        lower_typed_f64_body(blk, &f.params, &f.body)?
    };
    lf.block_mut(0).unwrap().ret(DOUBLE, &value);
    Ok(())
}

/// Compile the internal typed-i32 clone for a conservatively eligible user
/// function. The public JSValue trampoline guards/unboxes Int32-compatible
/// arguments, calls this raw clone, and boxes the i32 result at the ABI edge.
pub(super) fn compile_typed_i32_function(
    llmod: &mut LlModule,
    f: &Function,
    func_names: &HashMap<u32, String>,
) -> Result<()> {
    let generic_name = func_names
        .get(&f.id)
        .cloned()
        .ok_or_else(|| anyhow!("function name not resolved for {}", f.name))?;
    let llvm_name = typed_i32_function_name(&generic_name);
    let param_reps = typed_param_reps_for_params(&f.params)
        .ok_or_else(|| anyhow!("typed-i32 function '{}' has unsupported parameter", f.name))?;
    let params: Vec<(LlvmType, String)> = f
        .params
        .iter()
        .zip(param_reps.iter())
        .map(|(p, rep)| (rep.llvm_ty(), format!("%arg{}", p.id)))
        .collect();
    let lf = llmod.define_function(&llvm_name, I32, params);
    lf.linkage = "internal".to_string();
    lf.force_inline = true;
    let _ = lf.create_block("entry");

    let value = {
        let blk = lf.block_mut(0).unwrap();
        lower_typed_i32_body(blk, &f.params, &f.body)?
    };
    lf.block_mut(0).unwrap().ret(I32, &value);
    Ok(())
}

/// Compile the internal typed-i1 clone for a conservatively eligible user
/// function. `compile_function` emits both the public JSValue trampoline and
/// the internal generic fallback body; guarded direct FuncRef sites can still
/// call this clone directly.
pub(super) fn compile_typed_i1_function(
    llmod: &mut LlModule,
    f: &Function,
    func_names: &HashMap<u32, String>,
) -> Result<()> {
    let generic_name = func_names
        .get(&f.id)
        .cloned()
        .ok_or_else(|| anyhow!("function name not resolved for {}", f.name))?;
    let llvm_name = typed_i1_function_name(&generic_name);
    let param_reps = typed_param_reps_for_params(&f.params)
        .ok_or_else(|| anyhow!("typed-i1 function '{}' has unsupported parameter", f.name))?;
    let params: Vec<(LlvmType, String)> = f
        .params
        .iter()
        .zip(param_reps.iter())
        .map(|(p, rep)| (rep.llvm_ty(), format!("%arg{}", p.id)))
        .collect();
    let lf = llmod.define_function(&llvm_name, I1, params);
    lf.linkage = "internal".to_string();
    lf.force_inline = true;
    let _ = lf.create_block("entry");

    let value = {
        let blk = lf.block_mut(0).unwrap();
        lower_typed_i1_body(blk, &f.params, &f.body)?
    };
    lf.block_mut(0).unwrap().ret(I1, &value);
    Ok(())
}

/// Compile the internal typed-string clone for a conservatively eligible user
/// function. The clone passes raw `StringHeader*` handles as i64 and leaves
/// boxing to the public JSValue trampoline.
pub(super) fn compile_typed_string_function(
    llmod: &mut LlModule,
    f: &Function,
    func_names: &HashMap<u32, String>,
) -> Result<()> {
    let generic_name = func_names
        .get(&f.id)
        .cloned()
        .ok_or_else(|| anyhow!("function name not resolved for {}", f.name))?;
    let llvm_name = typed_string_function_name(&generic_name);
    let param_reps = typed_param_reps_for_params(&f.params).ok_or_else(|| {
        anyhow!(
            "typed-string function '{}' has unsupported parameter",
            f.name
        )
    })?;
    let params: Vec<(LlvmType, String)> = f
        .params
        .iter()
        .zip(param_reps.iter())
        .map(|(p, rep)| (rep.llvm_ty(), format!("%arg{}", p.id)))
        .collect();
    let lf = llmod.define_function(&llvm_name, I64, params);
    lf.linkage = "internal".to_string();
    lf.force_inline = true;
    let _ = lf.create_block("entry");

    let value = {
        let blk = lf.block_mut(0).unwrap();
        lower_typed_string_body(blk, &f.params, &f.body)?
    };
    lf.block_mut(0).unwrap().ret(I64, &value);
    Ok(())
}

fn emit_typed_public_trampoline_fast_value(
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

fn emit_public_typed_function_trampoline(
    llmod: &mut LlModule,
    f: &Function,
    public_name: &str,
    generic_body_name: &str,
    kind: TypedFunctionTrampolineKind,
) {
    let typed_name = match kind {
        TypedFunctionTrampolineKind::F64 => typed_f64_function_name(public_name),
        TypedFunctionTrampolineKind::I32 => typed_i32_function_name(public_name),
        TypedFunctionTrampolineKind::I1 => typed_i1_function_name(public_name),
        TypedFunctionTrampolineKind::StringRef => typed_string_function_name(public_name),
    };
    let arg_reps = match kind {
        TypedFunctionTrampolineKind::F64 => typed_param_reps_for_params(&f.params)
            .unwrap_or_else(|| vec![TypedParamRep::F64; f.params.len()]),
        TypedFunctionTrampolineKind::I32 => typed_param_reps_for_params(&f.params)
            .unwrap_or_else(|| vec![TypedParamRep::I32; f.params.len()]),
        TypedFunctionTrampolineKind::I1 => typed_param_reps_for_params(&f.params)
            .unwrap_or_else(|| vec![TypedParamRep::I1; f.params.len()]),
        TypedFunctionTrampolineKind::StringRef => typed_param_reps_for_params(&f.params)
            .unwrap_or_else(|| vec![TypedParamRep::StringRef; f.params.len()]),
    };
    let params: Vec<(LlvmType, String)> = f
        .params
        .iter()
        .map(|p| (DOUBLE, format!("%arg{}", p.id)))
        .collect();
    let arg_names: Vec<String> = f.params.iter().map(|p| format!("%arg{}", p.id)).collect();
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
        let value = emit_typed_public_trampoline_fast_value(
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
    let fast_label = wf.create_block("typed_public.fast").label.clone();
    let fallback_idx = wf.num_blocks();
    let fallback_label = wf.create_block("typed_public.fallback").label.clone();
    wf.block_mut(0)
        .unwrap()
        .cond_br(&guard, &fast_label, &fallback_label);

    let fast_value = emit_typed_public_trampoline_fast_value(
        wf.block_mut(fast_idx).unwrap(),
        kind,
        &typed_name,
        &arg_names,
        &arg_reps,
    );
    wf.block_mut(fast_idx).unwrap().ret(DOUBLE, &fast_value);

    let call_args: Vec<(LlvmType, &str)> =
        arg_names.iter().map(|arg| (DOUBLE, arg.as_str())).collect();
    let fallback_value =
        wf.block_mut(fallback_idx)
            .unwrap()
            .call(DOUBLE, generic_body_name, &call_args);
    wf.block_mut(fallback_idx)
        .unwrap()
        .ret(DOUBLE, &fallback_value);
}

/// Compile a single user function into the module.
pub(super) fn compile_function(
    llmod: &mut LlModule,
    f: &Function,
    func_names: &HashMap<u32, String>,
    strings: &mut StringPool,
    classes: &HashMap<String, &perry_hir::Class>,
    methods: &HashMap<(String, String), String>,
    module_globals: &HashMap<u32, String>,
    module_global_types: &HashMap<u32, perry_hir::types::Type>,
    import_function_prefixes: &HashMap<String, String>,
    enums: &HashMap<(String, String), perry_hir::EnumValue>,
    static_field_globals: &HashMap<(String, String), String>,
    class_ids: &HashMap<String, u32>,
    func_signatures: &HashMap<u32, (usize, bool, bool, bool)>,
    func_synthetic_arguments: &std::collections::HashSet<u32>,
    module_boxed_vars: &std::collections::HashSet<u32>,
    closure_rest_params: &HashMap<u32, usize>,
    cross_module: &CrossModuleCtx,
    typed_public_trampoline: Option<TypedFunctionTrampolineKind>,
    spec_entry: Option<&SpecFnPlan>,
) -> Result<()> {
    let public_llvm_name = func_names
        .get(&f.id)
        .cloned()
        .ok_or_else(|| anyhow!("function name not resolved for {}", f.name))?;
    let llvm_name = if let Some(plan) = spec_entry {
        // Spec entries are an ADDITIONAL internal symbol next to the ordinary
        // public body — never combined with the typed_abi trampoline scheme
        // (mutual exclusion is enforced at plan selection).
        debug_assert!(typed_public_trampoline.is_none());
        debug_assert_eq!(plan.reps.len(), f.params.len());
        spec_function_name(&public_llvm_name, &plan.reps)
    } else if typed_public_trampoline.is_some() {
        generic_function_body_name(&public_llvm_name)
    } else {
        public_llvm_name.clone()
    };

    // Phase A assumes all user-function params are `double`. Parameter
    // registers are named `%arg{LocalId}` so the body can store them into
    // alloca slots keyed by the same HIR LocalId. Specialized entries
    // (representation-selection Phase 2) instead type each parameter register
    // by its rep — raw i32, raw f64, or raw typed-array header pointer (i64).
    let params: Vec<(LlvmType, String)> = match spec_entry {
        Some(plan) => f
            .params
            .iter()
            .zip(plan.reps.iter())
            .map(|(p, rep)| (spec_rep_llvm_ty(*rep), format!("%arg{}", p.id)))
            .collect(),
        None => f
            .params
            .iter()
            .map(|p| (DOUBLE, format!("%arg{}", p.id)))
            .collect(),
    };

    let ic_base = llmod.ic_counter;
    let buffer_alias_base = llmod.buffer_alias_counter;
    let lf = llmod.define_function(&llvm_name, DOUBLE, params);
    if typed_public_trampoline.is_some() || spec_entry.is_some() {
        lf.linkage = "internal".to_string();
    }

    // Gen-GC Phase A sub-phase 3a: opt-in shadow-frame emission
    // for user functions. Pointer-typed param + local slots are
    // assigned pre-lowering via `collect_pointer_typed_locals`;
    // the frame is sized to hold all of them. Sub-phase 3b emits
    // the slot-set calls at Let/LocalSet sites to actually
    // populate the frame with live values; today the slots stay
    // zero (the tracer doesn't consume them yet — Phase A ship
    // criterion is "shadow stack is built but not yet consumed").
    let shadow_slot_map = if shadow_stack_enabled() {
        let flat_const_ids: std::collections::HashSet<u32> =
            cross_module.flat_const_arrays.keys().copied().collect();
        let m =
            crate::collectors::collect_pointer_typed_locals(&f.params, &f.body, &flat_const_ids);
        lf.enable_shadow_frame(m.len() as u32);
        m
    } else {
        std::collections::HashMap::new()
    };
    let shadow_slot_clears_after_stmt =
        crate::collectors::collect_shadow_slot_clear_points(&f.body, &shadow_slot_map);

    // Small leaf functions (≤ 8 statements) get alwaysinline so LLVM
    // exposes their operations to the caller's optimizer context — critical
    // for vectorizing clamp helpers and similar patterns. Excluded:
    // async/generator functions, AND functions rewritten by the
    // async-to-generator pre-pass (was_plain_async=true). Inlining the
    // rewritten wrapper into its caller breaks GC-root coverage of the
    // step closure's iter capture, hanging async chains (issue #447).
    if f.body.len() <= 8 && !f.is_async && !f.is_generator && !f.was_plain_async {
        lf.force_inline = true;
    }
    // Inline-hot-small (PERRY_INLINE_HOT_SMALL, default ON): bias — do not
    // force — LLVM toward inlining a *small* function that has a *hot* (in-loop)
    // call site. `inlinehint` only raises LLVM's inline threshold for this
    // callee; its `-O3` growth budget still refuses cold/oversized inlines, so
    // cold utility functions never bloat the binary (that is the anti-bloat
    // property, proved by the binary-size gate). We skip `alwaysinline`
    // functions (the hint would be redundant), async/generator forms, and rely
    // on `to_ir`'s `has_try`-first attribute precedence to keep any function
    // whose body later turns out to need `noinline` (try/setjmp/volatile) out.
    if !lf.force_inline
        && inline_hot_small_enabled()
        && (INLINE_HOT_SMALL_MIN..=inline_hot_small_size_cap()).contains(&f.body.len())
        && !f.is_async
        && !f.is_generator
        && !f.was_plain_async
        && cross_module.hot_loop_callees.contains(&f.id)
    {
        lf.inline_hint = true;
    }
    let _ = lf.create_block("entry");

    let mut boxed_vars = module_boxed_vars.clone();
    super::arguments::add_arguments_mapped_boxes(&f.params, &mut boxed_vars);

    // Store each param into an alloca slot, collecting LocalId → slot
    // mappings. We release the &mut LlBlock at scope end before handing
    // the function over to the FnCtx lowering pass.
    //
    // Specialized entries bind per-rep:
    //  - `I32` → straight into a canonical i32 slot (Phase 1 `SlotRep`
    //    mechanism; the raw `%arg` i32 stores with ZERO conversions and no
    //    double slot / no shadow binding — a number is never a GC root).
    //    Plan selection guarantees the param is never reassigned and never
    //    closure-referenced, so the canonical slot is trivially sound.
    //  - `TaPtr` → NaN-box the raw header pointer ONCE into the ordinary
    //    boxed double slot (all generic body paths stay correct) and keep the
    //    shadow-slot GC root binding — the header must stay live (typed-array
    //    storage never MOVES, but an unrooted header could still be swept).
    //  - `Boxed`/`F64` → exactly today's binding (a raw f64 IS its box).
    let mut spec_i32_param_slots: HashMap<u32, String> = HashMap::new();
    let mut bound_param_slots: HashSet<u32> = HashSet::new();
    let locals: HashMap<u32, String> = {
        let blk = lf.block_mut(0).unwrap();
        let mut map = HashMap::new();
        for (idx, p) in f.params.iter().enumerate() {
            let arg_name = format!("%arg{}", p.id);
            match spec_entry.map(|plan| plan.reps[idx]) {
                Some(crate::collectors::SpecParamRep::I32) => {
                    if crate::expr::canonical_i32_locals_enabled() {
                        let slot = blk.alloca(I32);
                        blk.store(I32, &arg_name, &slot);
                        spec_i32_param_slots.insert(p.id, slot);
                    } else {
                        // Phase-1 kill switch: keep the boxed protocol; one
                        // conversion at entry instead of one per call site.
                        let as_f64 = blk.sitofp(I32, &arg_name, DOUBLE);
                        let slot = blk.alloca(DOUBLE);
                        blk.store(DOUBLE, &as_f64, &slot);
                        map.insert(p.id, slot);
                    }
                    continue;
                }
                Some(crate::collectors::SpecParamRep::TaPtr { .. }) => {
                    let boxed = crate::expr::nanbox_pointer_inline(blk, &arg_name);
                    let slot = blk.alloca(DOUBLE);
                    blk.store(DOUBLE, &boxed, &slot);
                    // No callee-side shadow binding: every route into this
                    // entry is a Tier-A call whose argument is a proven
                    // never-reassigned rooted binding (module-global root or
                    // caller-frame slot) that stays live for the whole call,
                    // and typed-array storage is non-movable — the callee
                    // root would be redundant TLS traffic on the hot path.
                    map.insert(p.id, slot);
                    continue;
                }
                _ => {}
            }
            let slot = super::arguments::store_param_slot(blk, p, &boxed_vars, &arg_name);
            if let Some(slot_idx) = shadow_slot_map.get(&p.id).copied() {
                bound_param_slots.insert(slot_idx);
                blk.call_void(
                    "js_shadow_slot_bind",
                    &[(I32, &slot_idx.to_string()), (PTR, &slot)],
                );
            }
            map.insert(p.id, slot);
        }
        map
    };

    // Param types feed local_types so type-aware dispatch (e.g. string
    // concat detection on a `: string` parameter) works inside the body.
    // Also seed with module global types so functions that access module
    // globals see the correct declared types (e.g., Named("Editor")).
    let mut local_types: HashMap<u32, perry_hir::types::Type> = module_global_types
        .iter()
        .map(|(k, v)| (*k, v.clone()))
        .collect();
    for p in &f.params {
        local_types.insert(p.id, p.ty.clone());
    }
    // Specialized entry: stamp the proven reps over the (usually `Any`)
    // declared types BEFORE the native-fact collectors run, so every type
    // predicate (int-valued locals, typed-array receiver classification,
    // integer index proofs) sees the by-construction proof. Sound because
    // plan selection demoted any reassigned/closure-referenced param to
    // `Boxed` — a stamped param provably holds this rep for the whole body.
    if let Some(plan) = spec_entry {
        for (p, rep) in f.params.iter().zip(plan.reps.iter()) {
            match rep {
                crate::collectors::SpecParamRep::I32 => {
                    local_types.insert(p.id, perry_hir::types::Type::Int32);
                }
                crate::collectors::SpecParamRep::TaPtr { kind, .. } => {
                    if let Some(class) = spec_ta_kind_class_name(*kind) {
                        local_types.insert(p.id, perry_hir::types::Type::Named(class.to_string()));
                    }
                }
                crate::collectors::SpecParamRep::Boxed | crate::collectors::SpecParamRep::F64 => {}
            }
        }
    }

    // Pre-walk: which locals need to be boxed? A local is boxed when
    // it's captured by a closure AND written by someone (either the
    // enclosing function or inside a closure). Box-backing lets multiple
    // closures share the same mutable cell — critical for the common
    // `let x = 0; return { get: () => x, set: (n) => x = n }` pattern.
    // Pre-walk: which locals are provably integer-valued? Used by
    // `BinaryOp::Mod` to emit integer modulo instead of libm `fmod()`.
    let clamp_fn_ids: std::collections::HashSet<u32> = cross_module
        .clamp3_functions
        .union(&cross_module.clamp_u8_functions)
        .chain(cross_module.returns_int_functions.iter())
        .copied()
        .collect();
    let flat_const_ids: std::collections::HashSet<u32> =
        cross_module.flat_const_arrays.keys().copied().collect();
    // Spec-entry TaPtr params carry pre-pass-proven constant element counts —
    // feed them to the fact collectors (in-bounds proofs for the wrap-i32
    // additive admission).
    let spec_ta_lens: HashMap<u32, i64> = spec_entry
        .map(|plan| {
            f.params
                .iter()
                .zip(plan.reps.iter())
                .filter_map(|(p, rep)| match rep {
                    crate::collectors::SpecParamRep::TaPtr {
                        const_len: Some(len),
                        ..
                    } => Some((p.id, *len)),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    let native_facts = crate::collectors::collect_native_region_fact_graph_with_spec_lens(
        &f.body,
        &f.params,
        &flat_const_ids,
        &clamp_fn_ids,
        &cross_module.clamp3_functions,
        &boxed_vars,
        module_globals,
        // #6369: declared types of module-scope bindings this body reads through.
        &local_types,
        classes,
        &cross_module.compile_time_constants,
        &cross_module.module_dispatch,
        &spec_ta_lens,
    );

    if let Some(plan) = spec_entry {
        if std::env::var("PERRY_REPSEL_DEBUG").as_deref() == Ok("1") {
            eprintln!(
                "repsel: spec entry '{}' tuple=[{}] [{}]",
                f.name,
                plan.reps
                    .iter()
                    .map(|r| r.label())
                    .collect::<Vec<_>>()
                    .join(","),
                strings.module_prefix()
            );
        }
    }
    // Representation-selection Phase 1: canonical-i32 locals are allowed in
    // plain synchronous function bodies only. Async / generator /
    // `was_plain_async` bodies route locals through shared cells (the
    // async-to-generator transform), which the canonical model must not touch.
    let repsel_allows = crate::expr::canonical_i32_locals_enabled()
        && !f.is_async
        && !f.is_generator
        && !f.was_plain_async;
    let repsel_closure_refs = if repsel_allows {
        crate::expr::collect_closure_referenced_locals(&f.body)
    } else {
        std::collections::HashSet::new()
    };

    let mut ctx = FnCtx {
        func: lf,
        module_slug: crate::expr::native_region_slug(strings.module_prefix()),
        source_function: f.name.clone(),
        source_function_slug: crate::expr::native_region_slug(&f.name),
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
        this_stack: Vec::new(),
        inline_ctor_return: Vec::new(),
        new_target_stack: Vec::new(),
        class_stack: Vec::new(),
        methods,
        module_globals,
        import_function_prefixes,
        import_function_origin_names: &cross_module.import_function_origin_names,
        import_function_v8_specifiers: &cross_module.import_function_v8_specifiers,
        // Issue #841: node:submodule named-import + namespace registries.
        import_function_node_submodule: &cross_module.import_function_node_submodule,
        namespace_node_submodules: &cross_module.namespace_node_submodules,
        namespace_v8_specifiers: &cross_module.namespace_v8_specifiers,
        closure_captures: HashMap::new(),
        current_closure_ptr: None,
        enums,
        is_async_fn: f.is_async,
        is_strict_fn: f.is_strict,
        static_field_globals,
        class_ids,
        class_keys_globals: &cross_module.class_keys_globals,
        class_field_counts: &cross_module.class_field_counts,
        class_init_chains: &cross_module.class_init_chains,
        imported_class_ctors: &cross_module.imported_class_ctors,
        func_signatures,
        func_synthetic_arguments,
        func_returns_class: &cross_module.func_returns_class,
        boxed_vars,
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
        shadow_slot_map,
        persistent_shadow_slots: std::collections::HashSet::new(),
        shadow_slot_clears_after_stmt,
        shadow_slots_bound: bound_param_slots,
        arena_state_slot: None,
        class_keys_slots: HashMap::new(),
        cached_lengths: HashMap::new(),
        bounded_index_pairs: Vec::new(),
        packed_f64_loop_facts: Vec::new(),
        masked_window_array_facts: Vec::new(),
        masked_region_scalar_locals: std::collections::HashSet::new(),
        suppressed_cleared_shadow_slots: std::collections::HashSet::new(),
        class_field_loop_facts: Vec::new(),
        // Specialized entries seed the canonical-i32 registry with their raw
        // i32 params (empty otherwise — identical to the pre-phase behavior).
        local_slot_reps: spec_i32_param_slots
            .keys()
            .map(|id| (*id, crate::expr::SlotRep::I32))
            .collect(),
        i32_counter_slots: spec_i32_param_slots,
        repsel_context_allows_canonical_i32: repsel_allows,
        repsel_closure_ref_locals: repsel_closure_refs,
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
        was_unrolled: f.was_unrolled,
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

    let wrapper_name = format!("__perry_wrap_{}", public_llvm_name);
    super::arguments::materialize_arguments_object(
        &mut ctx,
        &f.params,
        super::arguments::ArgumentsCallee::FunctionWrapper(&wrapper_name),
    );

    // Issue #92 follow-up: pre-register `buffer_data_slots` entries for
    // `Buffer`-typed function parameters so that the readInt32BE/etc.
    // intrinsic fast path in `lower_call.rs` fires on
    // `function decode(row: Buffer) { row.readInt32BE(off) }` — the real
    // Postgres-driver hot-path shape, not just the `const buf = Buffer.alloc(N)`
    // micro-benchmark. Skipped when the param is reassigned (has_any_mutation
    // covers LocalSet/Update/ARRAY_MUTATORS — `buf = ...`, `buf.fill(...)` etc.)
    // because a cached data_ptr would go stale, and skipped for boxed params
    // (same reason via cross-closure mutation). Uint8Array-typed params are
    // deliberately excluded: a pre-existing crash surfaces when the same
    // program defines both a Buffer-param and a Uint8Array-param function and
    // then invokes them in sequence (reproducible on main without any of
    // this extension's changes). Tracked separately; Buffer coverage alone
    // hits the Postgres decode path which is the target workload here.
    for p in &f.params {
        let is_buffer_typed = matches!(
            &p.ty,
            perry_hir::types::Type::Named(n) if n == "Buffer"
        );
        if !is_buffer_typed {
            continue;
        }
        if ctx.boxed_vars.contains(&p.id) {
            continue;
        }
        if crate::collectors::has_any_mutation(&f.body, p.id) {
            continue;
        }
        let Some(param_slot) = ctx.locals.get(&p.id).cloned() else {
            continue;
        };
        let blk = ctx.block();
        let arg_val = blk.load(DOUBLE, &param_slot);
        let handle = crate::expr::unbox_to_i64(blk, &arg_val);
        let handle_ptr = blk.inttoptr(I64, &handle);
        let data_ptr = blk.gep(I8, &handle_ptr, &[(I32, "8")]);
        let buf_slot = ctx.func.alloca_entry(PTR);
        ctx.block().store(PTR, &data_ptr, &buf_slot);
        let scope_idx = ctx.buffer_alias_base + ctx.buffer_data_slots.len() as u32;
        ctx.buffer_data_slots
            .insert(p.id, (buf_slot.clone(), scope_idx));
        ctx.buffer_view_slots.insert(
            p.id,
            BufferViewSlot {
                data_slot: buf_slot,
                length_slot: None,
                scope_idx: Some(scope_idx),
                elem: BufferElem::U8,
                element_width_bytes: 1,
                index_unit: BufferIndexUnit::Byte,
                view_byte_offset: Some(0),
                length_offset_from_data: -8,
                alias: AliasState::Unknown,
                length_source: Some(LengthSource::Unknown),
                native_owned: None,
                // Declared-type hoist only — the construction form is unknown,
                // so no inline-storage proof.
                storage_inline_proven: false,
            },
        );
    }

    // Representation-selection Phase 2: bind each `TaPtr` param as a PROVEN
    // BufferViewSlot with the data pointer hoisted ONCE at entry (modeled on
    // the Buffer-param hoist above). The call-site pre-pass proved a fresh
    // non-view construction, so kind/data/length are fixed for the array's
    // lifetime: element accesses lower through the strong bare-load machinery
    // (`ta_int_elem_load_is_i32_provable`, `lower_typed_array_load`, the
    // proven-view checked tier) with bounds checks against the entry length —
    // and NEVER through the per-site guarded fast paths (`ta_param_f64_read`
    // skips receivers with a registered view slot). GC note: the header is
    // rooted via the boxed param slot's shadow binding above; the hoisted
    // data pointer stays valid because typed-array storage is non-movable
    // (`gc/types.rs`: `GC_TYPE_TYPED_ARRAY`/`GC_TYPE_BUFFER` `movable: false`)
    // and a non-view typed array cannot be detached or resized.
    if let Some(plan) = spec_entry {
        for (p, rep) in f.params.iter().zip(plan.reps.iter()) {
            let crate::collectors::SpecParamRep::TaPtr { kind, const_len } = rep else {
                continue;
            };
            let Some((elem, width)) = spec_ta_kind_elem_width(*kind) else {
                continue;
            };
            let Some(param_slot) = ctx.locals.get(&p.id).cloned() else {
                continue;
            };
            let blk = ctx.block();
            let arg_val = blk.load(DOUBLE, &param_slot);
            let handle = crate::expr::unbox_to_i64(blk, &arg_val);
            let handle_ptr = blk.inttoptr(I64, &handle);
            // TypedArrayHeader layout: length at +0, data at +16.
            let data_ptr = blk.gep(I8, &handle_ptr, &[(I32, "16")]);
            let data_slot = ctx.func.alloca_entry(PTR);
            ctx.block().store(PTR, &data_ptr, &data_slot);
            let scope_idx = ctx.buffer_alias_base + ctx.buffer_data_slots.len() as u32;
            ctx.buffer_data_slots
                .insert(p.id, (data_slot.clone(), scope_idx));
            ctx.buffer_view_slots.insert(
                p.id,
                BufferViewSlot {
                    data_slot,
                    length_slot: None,
                    scope_idx: Some(scope_idx),
                    elem,
                    element_width_bytes: width,
                    index_unit: BufferIndexUnit::Element,
                    view_byte_offset: Some(0),
                    length_offset_from_data: -16,
                    // Distinct `TaPtr` args are distinct fresh allocations
                    // (the Tier A call-site match rejects duplicate locals),
                    // so pairwise noalias holds by construction.
                    alias: AliasState::NoAliasProven,
                    length_source: Some(match const_len {
                        Some(len) => LengthSource::Constant(*len),
                        None => LengthSource::Unknown,
                    }),
                    native_owned: None,
                    storage_inline_proven: true,
                },
            );
        }
    }

    if f.is_async {
        stmt::lower_async_rejecting_top_level_stmts(&mut ctx, &f.body)
            .with_context(|| format!("lowering async body of '{}'", f.name))?;
    } else {
        stmt::lower_top_level_stmts(&mut ctx, &f.body)
            .with_context(|| format!("lowering body of '{}'", f.name))?;
    }

    // A function that falls off the end without an explicit `return`
    // returns `undefined` in JS — emit the NaN-boxed TAG_UNDEFINED
    // value so the LLVM verifier has a terminator AND user code that
    // does `f() === undefined` / `f() !== undefined` observes the
    // correct value. For async functions, wrap undefined in a
    // resolved promise so callers can await the result.
    if !ctx.block().is_terminated() {
        let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        if f.is_async {
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
    drop(ctx); // releases &mut LlFunction borrow on llmod
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
        emit_public_typed_function_trampoline(llmod, f, &public_llvm_name, &llvm_name, kind);
    }
    Ok(())
}
