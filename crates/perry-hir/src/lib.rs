//! High-level Intermediate Representation (HIR) for Perry
//!
//! The HIR is a typed, simplified representation of TypeScript code
//! that is easier to analyze and transform than the raw AST.

pub(crate) mod analysis;
pub(crate) mod destructuring;
pub(crate) mod enums;
pub mod error;
pub mod ir;
pub mod js_transform;
pub(crate) mod jsx;
pub mod lower;
pub(crate) mod lower_decl;
pub(crate) mod lower_patterns;
pub(crate) mod lower_types;
pub mod monomorph;
pub mod walker;

pub use analysis::{collect_local_refs_expr, collect_local_refs_stmt};
pub use enums::fix_imported_enums;
pub use ir::*;
pub use js_transform::{
    fix_cross_module_native_instances, fix_local_native_instances, transform_js_imports,
    ExportedNativeInstance,
};
pub use lower::{lower_module, lower_module_with_class_id, lower_module_with_class_id_and_types};
pub use monomorph::monomorphize_module;
