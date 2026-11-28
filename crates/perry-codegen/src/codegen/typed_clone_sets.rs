//! Typed-ABI clone decisions: which top-level functions and closures get a
//! typed clone, and the native-reps record of every rejected one.

use perry_hir::Module as HirModule;

use super::{
    split_regions, typed_abi, typed_f64_closure_name, typed_i1_closure_name,
    typed_i32_closure_name, typed_string_closure_name, CrossModuleCtx,
};

/// The typed clone sets of a module's top-level functions, each field named
/// for the local it binds.
pub(super) struct TypedFunctionSets {
    pub(super) typed_f64_functions: std::collections::HashSet<u32>,
    pub(super) typed_i32_functions: std::collections::HashSet<u32>,
    pub(super) typed_i1_functions: std::collections::HashSet<u32>,
    pub(super) typed_string_functions: std::collections::HashSet<u32>,
    pub(super) typed_i1_function_param_reps:
        std::collections::HashMap<u32, Vec<typed_abi::TypedParamRep>>,
}

fn should_record_typed_clone_rejection(reason: typed_abi::TypedCloneRejectionReason) -> bool {
    if std::env::var_os("PERRY_NATIVE_REPS_ALL_TYPED_CLONE_REJECTIONS").is_some() {
        return !matches!(reason, typed_abi::TypedCloneRejectionReason::NotClosure);
    }
    !matches!(
        reason,
        typed_abi::TypedCloneRejectionReason::NotClosure
            | typed_abi::TypedCloneRejectionReason::ReturnTypeNotF64
            | typed_abi::TypedCloneRejectionReason::ReturnTypeNotI32
            | typed_abi::TypedCloneRejectionReason::ReturnTypeNotI1
            | typed_abi::TypedCloneRejectionReason::ReturnTypeNotString
            | typed_abi::TypedCloneRejectionReason::NoReceiverField
    )
}

pub(super) fn record_typed_clone_rejection(
    records: &mut Vec<crate::native_value::NativeRepRecord>,
    source_function: impl Into<String>,
    consumer: &'static str,
    reason: typed_abi::TypedCloneRejectionReason,
    notes: Vec<String>,
) {
    if !should_record_typed_clone_rejection(reason) {
        return;
    }
    let source_function = source_function.into();
    // `--opt-report` (#6952): surface the specialized-ABI (RFC Phase 2)
    // decision, which is the only place params and returns get a
    // representation today. The `typed_*_clone_decision` consumers are the
    // older per-type clone mechanism and would report the same function up
    // to four times, so they stay out of the report and keep going to the
    // native-reps artifact only.
    if consumer == "spec_abi_entry_decision" && crate::opt_report::enabled() {
        let (why, tier, issue) = reason.opt_report_reason();
        crate::opt_report::deny_named(
            &source_function,
            crate::opt_report::RegionKind::Function,
            crate::opt_report::Denial {
                position: crate::opt_report::Position::Param,
                name: "(parameters + return)",
                local_id: None,
                analysis: crate::opt_report::Analysis::SpecAbi,
                rule: reason.as_str(),
                reason: why,
                tier,
                issue,
                loop_depth: 0,
                detail: None,
                byte_offset: None,
            },
        );
    }
    records.push(crate::native_value::typed_clone_rejection_record(
        source_function,
        consumer,
        reason.as_str(),
        notes,
    ));
}

pub(super) fn classify_typed_functions(
    hir: &HirModule,
    typed_clone_rejection_records: &mut Vec<crate::native_value::NativeRepRecord>,
) -> TypedFunctionSets {
    let mut typed_f64_functions = std::collections::HashSet::new();
    let mut typed_i32_functions = std::collections::HashSet::new();
    let mut typed_i1_functions = std::collections::HashSet::new();
    let mut typed_string_functions = std::collections::HashSet::new();
    let mut typed_i1_function_param_reps = std::collections::HashMap::new();
    for f in &hir.functions {
        // A split region is one body by design: no typed-ABI clone.
        if split_regions::is_region_function(f) {
            continue;
        }
        match typed_abi::typed_f64_function_rejection_reason(f) {
            None => {
                typed_f64_functions.insert(f.id);
                if let Some(reps) = typed_abi::typed_param_reps_for_params(&f.params) {
                    typed_i1_function_param_reps.insert(f.id, reps);
                }
            }
            Some(reason) => record_typed_clone_rejection(
                typed_clone_rejection_records,
                f.name.clone(),
                "typed_f64_function_clone_decision",
                reason,
                vec![
                    "typed_clone_kind=typed_f64_function".to_string(),
                    format!("function_id={}", f.id),
                    format!("symbol={}", f.name),
                ],
            ),
        }
        match typed_abi::typed_i32_function_rejection_reason(f) {
            None => {
                typed_i32_functions.insert(f.id);
                if let Some(reps) = typed_abi::typed_param_reps_for_params(&f.params) {
                    typed_i1_function_param_reps.insert(f.id, reps);
                }
            }
            Some(reason) => record_typed_clone_rejection(
                typed_clone_rejection_records,
                f.name.clone(),
                "typed_i32_function_clone_decision",
                reason,
                vec![
                    "typed_clone_kind=typed_i32_function".to_string(),
                    format!("function_id={}", f.id),
                    format!("symbol={}", f.name),
                ],
            ),
        }
        match typed_abi::typed_i1_function_rejection_reason(f) {
            None => {
                typed_i1_functions.insert(f.id);
                if let Some(reps) = typed_abi::typed_param_reps_for_params(&f.params) {
                    typed_i1_function_param_reps.insert(f.id, reps);
                }
            }
            Some(reason) => record_typed_clone_rejection(
                typed_clone_rejection_records,
                f.name.clone(),
                "typed_i1_function_clone_decision",
                reason,
                vec![
                    "typed_clone_kind=typed_i1_function".to_string(),
                    format!("function_id={}", f.id),
                    format!("symbol={}", f.name),
                ],
            ),
        }
        match typed_abi::typed_string_function_rejection_reason(f) {
            None => {
                typed_string_functions.insert(f.id);
                if let Some(reps) = typed_abi::typed_param_reps_for_params(&f.params) {
                    typed_i1_function_param_reps.insert(f.id, reps);
                }
            }
            Some(reason) => record_typed_clone_rejection(
                typed_clone_rejection_records,
                f.name.clone(),
                "typed_string_function_clone_decision",
                reason,
                vec![
                    "typed_clone_kind=typed_string_function".to_string(),
                    format!("function_id={}", f.id),
                    format!("symbol={}", f.name),
                ],
            ),
        }
    }
    TypedFunctionSets {
        typed_f64_functions,
        typed_i32_functions,
        typed_i1_functions,
        typed_string_functions,
        typed_i1_function_param_reps,
    }
}

pub(super) fn classify_typed_closures(
    cross_module: &mut CrossModuleCtx<'_>,
    closures: &[(perry_hir::types::FuncId, perry_hir::Expr)],
    typed_abi_local_types: &std::collections::HashMap<u32, perry_hir::types::Type>,
    typed_clone_rejection_records: &mut Vec<crate::native_value::NativeRepRecord>,
    module_prefix: &str,
) {
    cross_module.typed_f64_closures.clear();
    cross_module.typed_i32_closures.clear();
    cross_module.typed_i1_closures.clear();
    cross_module.typed_string_closures.clear();
    cross_module.typed_closure_capture_reps.clear();
    cross_module.typed_i1_closure_param_reps.clear();
    for (func_id, expr) in closures {
        // A closure region is one body by design: no typed-ABI clone.
        if cross_module.region_closures.contains(func_id) {
            continue;
        }
        if let Some(captures) =
            typed_abi::typed_f64_closure_capture_reps(expr, &typed_abi_local_types)
        {
            cross_module
                .typed_closure_capture_reps
                .insert(*func_id, captures.into_iter().map(|(_, rep)| rep).collect());
        }
        match typed_abi::typed_f64_closure_rejection_reason_with_types(expr, &typed_abi_local_types)
        {
            None => {
                cross_module.typed_f64_closures.insert(*func_id);
                if let perry_hir::Expr::Closure { params, .. } = expr {
                    if let Some(reps) = typed_abi::typed_param_reps_for_params(params) {
                        cross_module
                            .typed_i1_closure_param_reps
                            .insert(*func_id, reps);
                    }
                }
            }
            Some(reason) => record_typed_clone_rejection(
                typed_clone_rejection_records,
                format!("closure#{func_id}"),
                "typed_f64_closure_clone_decision",
                reason,
                vec![
                    "typed_clone_kind=typed_f64_closure".to_string(),
                    format!("closure_func_id={func_id}"),
                    format!(
                        "symbol={}",
                        typed_f64_closure_name(&crate::symbol_names::closure(
                            &module_prefix,
                            func_id
                        ))
                    ),
                ],
            ),
        }
        match typed_abi::typed_i1_closure_rejection_reason_with_types(expr, &typed_abi_local_types)
        {
            None => {
                cross_module.typed_i1_closures.insert(*func_id);
                if let perry_hir::Expr::Closure { params, .. } = expr {
                    if let Some(reps) = typed_abi::typed_param_reps_for_params(params) {
                        cross_module
                            .typed_i1_closure_param_reps
                            .insert(*func_id, reps);
                    }
                }
            }
            Some(reason) => record_typed_clone_rejection(
                typed_clone_rejection_records,
                format!("closure#{func_id}"),
                "typed_i1_closure_clone_decision",
                reason,
                vec![
                    "typed_clone_kind=typed_i1_closure".to_string(),
                    format!("closure_func_id={func_id}"),
                    format!(
                        "symbol={}",
                        typed_i1_closure_name(&crate::symbol_names::closure(
                            &module_prefix,
                            func_id
                        ))
                    ),
                ],
            ),
        }
        match typed_abi::typed_i32_closure_rejection_reason_with_types(expr, &typed_abi_local_types)
        {
            None => {
                cross_module.typed_i32_closures.insert(*func_id);
                if let perry_hir::Expr::Closure { params, .. } = expr {
                    if let Some(reps) = typed_abi::typed_param_reps_for_params(params) {
                        cross_module
                            .typed_i1_closure_param_reps
                            .insert(*func_id, reps);
                    }
                }
            }
            Some(reason) => record_typed_clone_rejection(
                typed_clone_rejection_records,
                format!("closure#{func_id}"),
                "typed_i32_closure_clone_decision",
                reason,
                vec![
                    "typed_clone_kind=typed_i32_closure".to_string(),
                    format!("closure_func_id={func_id}"),
                    format!(
                        "symbol={}",
                        typed_i32_closure_name(&crate::symbol_names::closure(
                            &module_prefix,
                            func_id
                        ))
                    ),
                ],
            ),
        }
        match typed_abi::typed_string_closure_rejection_reason_with_types(
            expr,
            &typed_abi_local_types,
        ) {
            None => {
                cross_module.typed_string_closures.insert(*func_id);
                if let perry_hir::Expr::Closure { params, .. } = expr {
                    if let Some(reps) = typed_abi::typed_param_reps_for_params(params) {
                        cross_module
                            .typed_i1_closure_param_reps
                            .insert(*func_id, reps);
                    }
                }
            }
            Some(reason) => record_typed_clone_rejection(
                typed_clone_rejection_records,
                format!("closure#{func_id}"),
                "typed_string_closure_clone_decision",
                reason,
                vec![
                    "typed_clone_kind=typed_string_closure".to_string(),
                    format!("closure_func_id={func_id}"),
                    format!(
                        "symbol={}",
                        typed_string_closure_name(&crate::symbol_names::closure(
                            &module_prefix,
                            func_id
                        ))
                    ),
                ],
            ),
        }
    }
}
