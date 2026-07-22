//! User-function name/signature registry for `compile_module`.
//!
//! Extracted verbatim from the `compile_module` body (pure code move, no
//! behavior change). Resolves every user function's mangled LLVM symbol up
//! front so body lowering can emit forward/recursive calls without worrying
//! about emission order, and records each function's ABI signature
//! `(param_count, has_rest, returns_number, synthetic_is_rest)`.

use std::collections::HashMap;

use perry_hir::Module as HirModule;

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
/// numeric `__dupN` suffix (exported functions reserve their canonical name
/// first and never get suffixed).
pub(crate) fn build_func_registry(hir: &HirModule, module_prefix: &str) -> FuncRegistry {
    let mut func_names: HashMap<u32, String> = HashMap::new();
    let mut func_signatures: HashMap<u32, (usize, bool, bool, bool)> = HashMap::new();
    let mut func_synthetic_arguments: std::collections::HashSet<u32> =
        std::collections::HashSet::new();
    // Distinct functions can mangle to the same symbol: minified code reuses
    // short names (`function A`) across scopes, and perry lambda-lifts nested
    // functions to module level, so two module functions can share a name — clang
    // then rejects the duplicate `define perry_fn_<mod>__A`. Disambiguate with a
    // numeric suffix, keyed by the mangled symbol. Exported functions are
    // referenced cross-module by their canonical `scoped_fn_name` and are unique
    // per module, so they reserve that name first and never get suffixed.
    let mut used_fn_symbols: HashMap<String, u32> = HashMap::new();
    for f in &hir.functions {
        if hir.exported_functions.iter().any(|(exp, _)| exp == &f.name) {
            used_fn_symbols
                .entry(scoped_fn_name(module_prefix, &f.name))
                .or_insert(1);
        }
    }
    for f in &hir.functions {
        let base = scoped_fn_name(module_prefix, &f.name);
        let is_exported = hir.exported_functions.iter().any(|(exp, _)| exp == &f.name);
        let sym = if is_exported {
            base
        } else {
            let n = used_fn_symbols.entry(base.clone()).or_insert(0);
            let s = if *n == 0 {
                base.clone()
            } else {
                format!("{base}__dup{n}")
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
