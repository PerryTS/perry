//! IR Transformations for Perry
//!
//! This crate contains transformation passes that run on the HIR:
//! - Closure conversion
//! - Async/await lowering
//! - Optimization passes (function inlining)
//! - i18n string localization

pub mod async_to_generator;
pub mod closure;
pub mod generator;
pub mod i18n;
pub mod inline;
pub mod state_desugar;
pub mod unroll;

// Re-export main transformation functions
pub use async_to_generator::transform_async_to_generator;
pub use closure::convert_closures;
pub use generator::transform_generators;
pub use i18n::{apply_i18n, I18nDiagnostic, I18nStringTable};
pub use inline::{
    gather_cross_module_anon_classes, gather_cross_module_methods,
    gather_cross_module_methods_with_extern_imports, inline_functions, MethodCandidate,
};
pub use unroll::unroll_static_loops;
