//! IR Transformations for Perry
//!
//! This crate contains transformation passes that run on the HIR:
//! - Closure conversion
//! - Async/await lowering
//! - Optimization passes (function inlining)
//! - i18n string localization

pub mod async_to_generator;
pub mod closure;
pub mod deforest;
pub mod finally_inline;
pub mod generator;
pub mod i18n;
pub mod inline;
pub mod prop_cse;
pub mod state_desugar;
pub mod unroll;

// Re-export main transformation functions
pub use async_to_generator::transform_async_to_generator;
pub use closure::convert_closures;
pub use finally_inline::inline_finally_into_returns;
pub use generator::transform_generators;
pub use i18n::{apply_i18n, I18nDiagnostic, I18nStringTable};
pub use inline::{
    gather_cross_module_anon_classes, gather_cross_module_methods,
    gather_cross_module_methods_with_extern_imports, inline_functions, MethodCandidate,
};
pub use unroll::unroll_static_loops;

/// Post-inline HIR cleanups, in the one order that works for both.
///
/// [`unroll_static_loops`] and [`prop_cse::run`] share their ordering
/// constraint, so the driver runs them through a single entry point rather
/// than repeating the constraint at each call site:
///
/// * **After the inliner** — both want the inlined callee bodies. The unroller
///   gets their loops; `prop_cse` gets the guard chains an inlined body
///   contributes, and the copies the unroller then makes.
/// * **Before the async/generator transforms** — those rewrite control flow
///   into state-machine shapes the unroll match no longer recognizes, and they
///   box every body local into a shared mutable `Any` cell, which would turn a
///   hoisted `const` into one more boxed cell instead of a register.
///   (`prop_cse` skips async/generator bodies for exactly that reason; keeping
///   the order here stops the two facts from drifting apart.)
pub fn post_inline_cleanups(module: &mut perry_hir::Module) {
    unroll_static_loops(module);
    prop_cse::run(module);
}
