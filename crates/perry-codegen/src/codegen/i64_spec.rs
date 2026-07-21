//! Integer-specialization pass for `compile_module`.
//!
//! Extracted verbatim from the `compile_module` body (pure code move, no
//! behavior change). For pure numeric recursive functions (like fibonacci),
//! emits an i64 variant that uses integer registers and integer arithmetic;
//! the f64 wrapper calls fptosi → i64_fn → sitofp. Returns the set of FuncIds
//! that were specialized so the main compile loop can skip re-emitting them.

use std::collections::HashMap;

use perry_hir::Module as HirModule;

use crate::module::LlModule;
use crate::types::{LlvmType, DOUBLE, I64};

// Collector and boxing-analysis walkers live in dedicated modules.

/// Emit i64-specialized bodies (+ f64 wrappers) for integer-specializable
/// functions. Returns the set of specialized FuncIds.
pub(crate) fn emit_i64_specializations(
    llmod: &mut LlModule,
    hir: &HirModule,
    func_names: &HashMap<u32, String>,
    module_globals: &HashMap<u32, String>,
) -> std::collections::HashSet<u32> {
    let mut i64_specialized: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for f in &hir.functions {
        // Skip integer specialization for functions that access module globals.
        // The i64 body emitter can't handle module global loads (it produces
        // `ret 0` instead of reading the global), creating a broken stub
        // that shadows the real compiled function.
        let uses_module_globals = f.body.iter().any(|s| {
            fn walks(s: &perry_hir::Stmt, mg: &HashMap<u32, String>) -> bool {
                match s {
                    perry_hir::Stmt::Return(Some(perry_hir::Expr::LocalGet(id))) => {
                        mg.contains_key(id)
                    }
                    perry_hir::Stmt::Expr(perry_hir::Expr::LocalGet(id)) => mg.contains_key(id),
                    _ => false,
                }
            }
            walks(s, module_globals)
        });
        // Skip clamp-shaped functions: their FuncRef call sites with provably
        // i32 arguments are intrinsified to smax/smin and never call this
        // symbol, so the only remaining callers are exactly the ones whose
        // arguments are NOT integers (fractional doubles, NaN-boxed pointers)
        // — and clamp3 returns an argument verbatim, so the wrapper's
        // unconditional `fptosi` miscompiles every one of them (#4785 bug
        // class: `(number).method is not a function`). Those callers need
        // the real f64 body.
        let is_clamp_shape =
            crate::collectors::detect_clamp3(f).is_some() || crate::collectors::detect_clamp_u8(f);
        if crate::collectors::is_integer_specializable(f) && !uses_module_globals && !is_clamp_shape
        {
            if let Some(llvm_name) = func_names.get(&f.id) {
                let i64_name = format!("{}_i64", llvm_name);
                crate::collectors::emit_i64_function(llmod, f, &i64_name);
                // Emit the f64 wrapper that calls the i64 version.
                // Mark as alwaysinline so LLVM exposes the integer ops
                // to callers — critical for vectorizing clamp patterns.
                let params: Vec<(LlvmType, String)> = f
                    .params
                    .iter()
                    .map(|p| (DOUBLE, format!("%arg{}", p.id)))
                    .collect();
                let wrapper = llmod.define_function(llvm_name, DOUBLE, params);
                wrapper.force_inline = true;
                let _ = wrapper.create_block("entry");
                let blk = wrapper.block_mut(0).unwrap();
                let mut i64_args: Vec<(LlvmType, String)> = Vec::new();
                for p in &f.params {
                    let i64_v = blk.fptosi(DOUBLE, &format!("%arg{}", p.id), I64);
                    i64_args.push((I64, i64_v));
                }
                let refs: Vec<(LlvmType, &str)> =
                    i64_args.iter().map(|(t, v)| (*t, v.as_str())).collect();
                let i64_result = blk.call(I64, &i64_name, &refs);
                let f64_result = blk.sitofp(I64, &i64_result, DOUBLE);
                blk.ret(DOUBLE, &f64_result);
                i64_specialized.insert(f.id);
            }
        }
    }
    i64_specialized
}
