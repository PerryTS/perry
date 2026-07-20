//! High-level Intermediate Representation (HIR) for Perry
//!
//! The HIR is a typed, simplified representation of TypeScript code
//! that is easier to analyze and transform than the raw AST.

pub mod analysis;
pub mod audit;
pub mod cap_fields;
pub mod capability;
mod class_accessors;
pub mod deferral;
pub(crate) mod destructuring;
pub mod dynamic_import;
pub mod egress;
pub(crate) mod enums;
pub mod error;
pub mod eval_classifier;
pub mod ir;
pub mod js_transform;
pub(crate) mod jsx;
pub mod lockdown;
pub mod lower;
pub(crate) mod lower_decl;
pub(crate) mod lower_patterns;
pub(crate) mod lower_types;
pub mod monomorph;
pub mod stable_hash;
pub mod walker;

pub use analysis::{
    collect_local_refs_expr, collect_local_refs_stmt, infer_expr_type, infer_refinable_expr_type,
    HirTypeEnv, HirTypeFacts,
};
pub use audit::{audit_module, AuditManifest, ModuleAudit};
pub use capability::{audit_module_capabilities, CapabilityPolicy, CapabilityViolation};
pub use class_accessors::ClassAccessorNames;
pub use deferral::{arm_deferral_sink, disarm_deferral_sink, try_defer_refusal, DeferredRefusal};
pub use dynamic_import::{
    collect_dynamic_import_local_candidate_literals, collect_dynamic_import_param_literals,
    collect_module_const_locals, detect_top_level_await, dynamic_import_glob_pattern,
    flatten_exports, for_each_dynamic_import, for_each_dynamic_import_mut, for_each_worker_new,
    for_each_worker_new_mut, resolve_import_path, resolve_import_path_with_consts,
    resolve_import_path_with_consts_and_params, resolve_import_path_with_context, FlatExport,
    Resolution, DYNAMIC_IMPORT_PATH_CAP,
};
pub use egress::{audit_module_egress, EgressRefusalReason, EgressViolation};
pub use enums::fix_imported_enums;
pub use eval_classifier::{
    check_unimplemented_api, classify as classify_eval_surface, has_deferred_dynamic_code_sites,
    location_string, record_deferred_aot_site, set_eval_strict_mode, set_unimplemented_strict_mode,
    take_deferred_eval_sites, DeferredEvalSite, EvalBucket, EvalClassification, EvalDecision,
    EvalSurface, UnimplementedDecision, UNIMPLEMENTED_API_KIND,
};
pub use ir::*;
pub use js_transform::{
    fix_cross_module_native_instances, fix_local_native_instances, transform_js_imports,
    ExportedNativeInstance,
};
pub use lockdown::{audit_module_lockdown, LockdownViolation};
pub use lower::{
    lower_module, lower_module_full, lower_module_with_class_id,
    lower_module_with_class_id_and_types, lower_module_with_class_id_types_and_seed,
    lower_module_with_class_id_types_seed_and_entry,
};
pub use monomorph::monomorphize_module;
