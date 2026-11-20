//! Raw numeric capture values cross the private typed-body boundary once.
//!
//! The public entry already classifies every capture together with its
//! arguments. Passing the admitted values prevents a second load/INT32
//! normalization in the body. Other entry paths keep their existing ABI.
use super::typed_abi::{
    emit_typed_arg_guard, emit_typed_arg_to_raw, lower_typed_f64_body_with_seed_locals_and_reps,
    typed_f64_closure_capture_reps, typed_f64_closure_name, typed_param_reps_for_params,
    TypedParamRep,
};
use crate::types::{LlvmType, DOUBLE, I1, I32, I64};
use crate::{block::LlBlock, module::LlModule};
use anyhow::{anyhow, Result};
use perry_hir::{Expr, Param};
use std::collections::HashMap;

pub(super) fn supports(params: &[Param], captures: &[TypedParamRep]) -> bool {
    !captures.is_empty()
        && captures.iter().all(|r| matches!(r, TypedParamRep::F64))
        && typed_param_reps_for_params(params)
            .is_some_and(|reps| reps.iter().all(|r| matches!(r, TypedParamRep::F64)))
}
pub(super) fn name(typed_name: &str) -> String {
    crate::fn_info::normalized_capture_body_symbol(typed_name)
}
pub(super) fn load_typed_capture(
    blk: &mut LlBlock,
    header_size: u64,
    index: usize,
    rep: TypedParamRep,
) -> String {
    let word = super::closure::load_entry_capture_word(blk, header_size, index);
    let value = blk.bitcast_i64_to_double(&word);
    emit_typed_arg_to_raw(blk, rep, &value)
}
pub(super) fn compile(
    llmod: &mut LlModule,
    func_id: u32,
    closure: &Expr,
    prefix: &str,
    local_types: &HashMap<u32, perry_hir::types::Type>,
) -> Result<()> {
    let Expr::Closure { params, body, .. } = closure else {
        return Err(anyhow!("expected closure"));
    };
    let Some(captures) = typed_f64_closure_capture_reps(closure, local_types) else {
        return Ok(());
    };
    let reps: Vec<_> = captures.iter().map(|(_, r)| *r).collect();
    if !supports(params, &reps) {
        return Ok(());
    }
    let typed_name = typed_f64_closure_name(&crate::fn_info::closure_body_symbol(prefix, func_id));
    let mut llvm_params: Vec<(LlvmType, String)> = vec![(I64, "%this_closure".into())];
    llvm_params.extend(params.iter().map(|p| (DOUBLE, format!("%arg{}", p.id))));
    llvm_params.extend(
        captures
            .iter()
            .map(|(id, _)| (DOUBLE, format!("%capture{id}"))),
    );
    let f = llmod.define_function(&name(&typed_name), DOUBLE, llvm_params);
    f.linkage = "internal".into();
    f.force_inline = true;
    let blk = f.create_block("entry");
    let values = captures
        .iter()
        .map(|(id, _)| (*id, format!("%capture{id}")))
        .collect();
    let reps = captures.iter().copied().collect();
    let value = lower_typed_f64_body_with_seed_locals_and_reps(blk, params, body, values, reps)?;
    f.block_mut(0).unwrap().ret(DOUBLE, &value);
    f.pre_statepoint_inline =
        super::helpers::guarded_specialization_fits_preinline_budget(f.estimated_ir_bytes());
    Ok(())
}

pub(super) fn fits_preinline_budget(module: &LlModule, name: &str) -> bool {
    module
        .function_estimated_ir_bytes(name)
        .is_some_and(super::helpers::guarded_specialization_fits_preinline_budget)
}

pub(crate) fn emit_typed_capture_guard(
    blk: &mut crate::block::LlBlock,
    closure_handle: &str,
    capture_reps: &[TypedParamRep],
) -> Option<String> {
    let mut guard: Option<String> = None;
    for (idx, rep) in capture_reps.iter().enumerate() {
        let idx = idx.to_string();
        let captured_bits = blk.call(
            I64,
            "js_closure_get_capture_bits",
            &[(I64, closure_handle), (I32, &idx)],
        );
        let captured = blk.bitcast_i64_to_double(&captured_bits);
        let ok = emit_typed_arg_guard(blk, *rep, &captured);
        guard = Some(match guard {
            Some(prev) => blk.and(I1, &prev, &ok),
            None => ok,
        });
    }
    guard
}

#[cfg(test)]
mod tests {
    #[test]
    fn numeric_capture_admission_is_visible_before_statepoints() {
        let ast = perry_parser::parse_typescript(
            "function make(n: number) { return (x: number) => (x * 7 + n) % 101; } console.log(make(3)(4));",
            "capture_contract.ts",
        ).unwrap();
        let hir = perry_hir::lower_module(&ast, "capture_contract", "capture_contract.ts").unwrap();
        let id = perry_hir::emitted_closures(&hir)[0].0;
        let ir = String::from_utf8(
            crate::compile_module(
                &hir,
                crate::CompileOptions {
                    emit_ir_only: true,
                    ..Default::default()
                },
            )
            .unwrap(),
        )
        .unwrap();
        let header = ir
            .lines()
            .find(|l| {
                l.starts_with("define ")
                    && l.contains("@perry_closure_")
                    && l.contains(&format!("__{id}("))
            })
            .unwrap_or_else(|| panic!("missing public capture admission: {ir}"));
        let symbol = header.split('@').nth(1).unwrap().split('(').next().unwrap();
        assert!(header.contains("alwaysinline"), "{header}");
        assert!(
            ir.contains(".$normalized_captures("),
            "private scalar body must be live: {ir}"
        );
        assert!(
            ir.contains("typed_closure_public"),
            "actual value guards must remain: {ir}"
        );
        assert!(
            ir.contains(&format!("@{symbol}$generic(")),
            "boxed continuation must remain: {ir}"
        );
        assert!(
            ir.contains("@perry_checked_number_remainder("),
            "exact remainder admission must remain: {ir}"
        );
    }
}
