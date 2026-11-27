//! Typed-ABI clone decisions: which top-level functions get a typed clone,
//! and the native-reps record of every rejected one.

use perry_hir::Module as HirModule;

use super::{split_regions, typed_abi};

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
