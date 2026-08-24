//! Borrowed inputs for the module artifact-emission phase.

use std::collections::{HashMap, HashSet};

use perry_hir::Module as HirModule;

use crate::module::LlModule;
use crate::strings::StringPool;

use super::opts::CrossModuleCtx;

/// Read-only view of the `CompileOptions` fields that artifact emission still
/// references after the pipeline has moved other fields into `CrossModuleCtx`.
pub(super) struct OptsView<'a> {
    pub(super) import_function_prefixes: &'a HashMap<String, String>,
    pub(super) imported_classes: &'a [super::opts::ImportedClass],
    pub(super) is_entry_module: bool,
    pub(super) non_entry_module_prefixes: &'a [String],
    pub(super) output_type: &'a str,
}

/// Data computed by the `compile_module` prelude and borrowed by the artifact
/// tail. Keeping it together avoids a second oversized compiler entry module.
pub(super) struct ModuleArtifactsCtx<'a> {
    pub progress: &'a super::CompileProgress,
    pub llmod: &'a mut LlModule,
    pub target_triple: &'a str,
    pub strings: &'a mut StringPool,
    pub hir: &'a HirModule,
    pub import_function_prefixes: &'a HashMap<String, String>,
    pub imported_classes: &'a [super::opts::ImportedClass],
    pub is_entry_module: bool,
    pub non_entry_module_prefixes: &'a [String],
    pub output_type: &'a str,
    pub module_prefix: &'a String,
    pub class_table: &'a HashMap<String, &'a perry_hir::Class>,
    pub class_ids: &'a HashMap<String, u32>,
    pub enum_table: &'a HashMap<(String, String), perry_hir::EnumValue>,
    pub module_globals: &'a HashMap<u32, String>,
    pub module_global_types: &'a HashMap<u32, perry_hir::types::Type>,
    pub static_field_globals: &'a HashMap<(String, String), String>,
    pub method_names: &'a HashMap<(String, String), String>,
    pub func_names: &'a HashMap<u32, String>,
    pub func_signatures: &'a HashMap<u32, (usize, bool, bool, bool)>,
    pub func_synthetic_arguments: &'a HashSet<u32>,
    pub module_boxed_vars: &'a HashSet<u32>,
    /// Typed-ABI capture oracle: module-wide local types minus boxed ids.
    pub module_local_types: &'a HashMap<u32, perry_hir::types::Type>,
    /// Source-type metadata for closure receivers; not a representation proof.
    pub module_receiver_types: &'a HashMap<u32, perry_hir::types::Type>,
    pub closure_rest_params: &'a HashMap<u32, usize>,
    pub closure_synthetic_arguments: &'a HashSet<u32>,
    pub closure_rest_and_arguments: &'a HashSet<u32>,
    pub closure_arities: &'a HashMap<u32, u32>,
    pub closure_lengths: &'a HashMap<u32, u32>,
    pub closure_arrow_functions: &'a HashSet<u32>,
    pub trusted_box_closures: &'a HashMap<u32, super::closure_collect::TrustedBoxClosure>,
    pub closures: &'a [(perry_hir::types::FuncId, perry_hir::Expr)],
    pub class_keys_init_data: &'a [(String, String, u32, Vec<u64>, Vec<u64>)],
    /// Keys global to `(class id, packed GcHeader word)` for inline `new`.
    pub class_header_image_inits: &'a HashMap<String, (u32, u64)>,
    pub imported_class_stubs: &'a [perry_hir::Class],
    pub cross_module: &'a CrossModuleCtx,
}
