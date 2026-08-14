//! User-function name/signature registry for `compile_module`.
//!
//! Extracted verbatim from the `compile_module` body (pure code move, no
//! behavior change). Resolves every user function's mangled LLVM symbol up
//! front so body lowering can emit forward/recursive calls without worrying
//! about emission order, and records each function's ABI signature
//! `(param_count, has_rest, returns_number, synthetic_is_rest)`.

use std::collections::HashMap;

use perry_hir::{Export, Module as HirModule};

// Collector and boxing-analysis walkers live in dedicated modules.

// Name-mangling helper from the trunk (also reachable via `super::*`).
use super::helpers::scoped_fn_name;

/// Result bundle of the user-function name/signature registry pass.
pub(crate) struct FuncRegistry {
    pub func_names: HashMap<u32, String>,
    pub func_signatures: HashMap<u32, (usize, bool, bool, bool)>,
    pub func_synthetic_arguments: std::collections::HashSet<u32>,
}

/// Resolve user function names + signatures up front. Names are scoped by
/// module prefix; distinct functions that mangle to the same symbol get a
/// numeric `$dupN` suffix (exported functions reserve their canonical name
/// first and never get suffixed). The `$` separator keeps the uniquifier in
/// the reserved generated-suffix namespace (issue #6927): `sanitize` output
/// is `[A-Za-z0-9_]`-only, so a user function literally named `A__dup1`
/// cannot collide with the disambiguated symbol of a duplicate `A`.
pub(crate) fn build_func_registry(hir: &HirModule, module_prefix: &str) -> FuncRegistry {
    let mut func_names: HashMap<u32, String> = HashMap::new();
    let mut func_signatures: HashMap<u32, (usize, bool, bool, bool)> = HashMap::new();
    let mut func_synthetic_arguments: std::collections::HashSet<u32> =
        std::collections::HashSet::new();
    // Distinct functions can mangle to the same symbol: minified code reuses
    // short names (`function A`) across scopes, and perry lambda-lifts nested
    // functions to module level, so two module functions can share a name — clang
    // then rejects the duplicate `define perry_fn_<mod>__A`. Disambiguate with
    // a numeric suffix, keyed by the mangled symbol. Exported functions are
    // referenced cross-module by their canonical `scoped_fn_name` and are unique
    // per module, so they reserve that name first and never get suffixed.
    let mut used_fn_symbols: HashMap<String, u32> = HashMap::new();
    // Every public named export reserves its ABI symbol, including exported
    // runtime values/closures that are served by a module-global getter.
    // Otherwise an unrelated local function with the public name can claim
    // the symbol before either a forwarding alias or value getter is emitted:
    //
    //   function optionalKey(ast) { ... }       // private AST helper
    //   const optionalKey2 = lambda(...);        // public Schema wrapper
    //   export { optionalKey2 as optionalKey };
    //
    // Reserving only `exported_functions` covered direct FuncRef aliases but
    // missed the closure-valued second shape (OpenCode/Effect Schema).
    for export in &hir.exports {
        if let Export::Named { exported, .. } = export {
            used_fn_symbols
                .entry(scoped_fn_name(module_prefix, exported))
                .or_insert(1);
        }
    }
    for f in &hir.functions {
        let base = scoped_fn_name(module_prefix, &f.name);
        // A function owns its canonical local-name symbol only when that exact
        // public name maps to this exact FuncId. Name-only matching is wrong
        // for renamed exports and made a different local function overwrite
        // the export target (OpenCode/Effect: `resolveAt2 as resolveAt`).
        let owns_canonical_export = hir
            .exported_functions
            .iter()
            .any(|(exp, func_id)| exp == &f.name && *func_id == f.id);
        let sym = if owns_canonical_export {
            base
        } else {
            let n = used_fn_symbols.entry(base.clone()).or_insert(0);
            let s = if *n == 0 {
                base.clone()
            } else {
                format!("{base}$dup{n}")
            };
            *n += 1;
            s
        };
        func_names.insert(f.id, sym);
        let has_rest = f.params.iter().any(|p| p.is_rest);
        let synthetic_is_rest = f
            .params
            .last()
            .map(|p| p.arguments_object.is_some() && p.is_rest)
            .unwrap_or(false);
        if f.params
            .last()
            .map(|p| p.arguments_object.is_some())
            .unwrap_or(false)
        {
            func_synthetic_arguments.insert(f.id);
        }
        let returns_number = matches!(
            f.return_type,
            perry_hir::types::Type::Number | perry_hir::types::Type::Int32
        );
        func_signatures.insert(
            f.id,
            (f.params.len(), has_rest, returns_number, synthetic_is_rest),
        );
    }

    FuncRegistry {
        func_names,
        func_signatures,
        func_synthetic_arguments,
    }
}
