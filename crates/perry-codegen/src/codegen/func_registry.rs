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
use super::helpers::{sanitize, scoped_fn_name};

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
    //
    // Every public export name owns `perry_fn_<mod>__<name>` too: importers
    // call that symbol, and the export getters and forwarding wrappers define
    // it. A function is entitled to it only when it IS that export. Any other
    // function spelled the same (a local `function inner` beside
    // `export { inner2 as inner }`, as prettier's `formatWithCursor` beside
    // `export { formatWithCursor2 as formatWithCursor }`) takes a suffixed
    // symbol, or the export's getter replaces its body.
    let mut used_fn_symbols: HashMap<String, u32> = HashMap::new();
    for name in public_export_names(hir) {
        for sym in export_symbol_spellings(module_prefix, name) {
            used_fn_symbols.entry(sym).or_insert(1);
        }
    }
    let function_ids: std::collections::HashSet<u32> = hir.functions.iter().map(|f| f.id).collect();
    for f in &hir.functions {
        let base = scoped_fn_name(module_prefix, &f.name);
        let is_exported = is_exported_function(hir, &function_ids, f);
        let sym = if is_exported {
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

/// The names this module publishes to importers.
fn public_export_names(hir: &HirModule) -> impl Iterator<Item = &str> {
    hir.exports.iter().filter_map(|export| match export {
        perry_hir::Export::Named { exported, .. }
        | perry_hir::Export::ReExport { exported, .. } => Some(exported.as_str()),
        perry_hir::Export::NamespaceReExport { name, .. } => Some(name.as_str()),
        perry_hir::Export::ExportAll { .. } => None,
    })
}

/// Every spelling under which an export's symbol is defined: the getter and
/// wrapper paths use `sanitize`, the raw-spelling aliases keep the name
/// verbatim, and a function body uses `scoped_fn_name`.
fn export_symbol_spellings(module_prefix: &str, name: &str) -> [String; 3] {
    [
        scoped_fn_name(module_prefix, name),
        format!("perry_fn_{}__{}", module_prefix, sanitize(name)),
        format!("perry_fn_{}__{}", module_prefix, name),
    ]
}

/// `f` is the function an export publishes under its own name. The export
/// records the function's id; a function that merely shares the name (a
/// lambda-lifted nested function, or a local beside `export { a as f }`) is
/// not it. A recorded id that no longer names a function falls back to the
/// name.
fn is_exported_function(
    hir: &HirModule,
    function_ids: &std::collections::HashSet<u32>,
    f: &perry_hir::Function,
) -> bool {
    hir.exported_functions
        .iter()
        .any(|(exp, id)| exp == &f.name && (*id == f.id || !function_ids.contains(id)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::{Export, Function};

    fn function(id: u32, name: &str) -> Function {
        Function {
            id,
            name: name.to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: perry_hir::types::Type::Any,
            body: Vec::new(),
            is_async: false,
            is_generator: false,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
            is_strict: false,
        }
    }

    fn renamed(local: &str, exported: &str) -> Export {
        Export::Named {
            local: local.to_string(),
            exported: exported.to_string(),
        }
    }

    /// prettier's index.mjs: `async function formatWithCursor` beside
    /// `var formatWithCursor2 = withPlugins(formatWithCursor)` and
    /// `export { formatWithCursor2 as formatWithCursor }`. The export's getter
    /// is `perry_fn_m__formatWithCursor`; the local function must not be.
    #[test]
    fn a_local_function_never_takes_another_bindings_export_symbol() {
        let mut hir = HirModule::new("m");
        hir.functions.push(function(1, "formatWithCursor"));
        hir.exports
            .push(renamed("formatWithCursor2", "formatWithCursor"));
        let names = build_func_registry(&hir, "m").func_names;
        assert_ne!(names[&1], "perry_fn_m__formatWithCursor");
    }

    /// `export { a as b }` beside a local `function b`: the forwarding
    /// wrapper owns `perry_fn_m__b` and calls `a`.
    #[test]
    fn a_function_export_alias_owns_its_public_symbol() {
        let mut hir = HirModule::new("m");
        hir.functions.push(function(1, "a"));
        hir.functions.push(function(2, "b"));
        hir.exports.push(renamed("a", "b"));
        hir.exported_functions.push(("b".to_string(), 1));
        let names = build_func_registry(&hir, "m").func_names;
        assert_eq!(names[&1], "perry_fn_m__a");
        assert_ne!(names[&2], "perry_fn_m__b");
    }

    /// The export itself keeps the canonical symbol, ahead of a lambda-lifted
    /// nested function of the same name.
    #[test]
    fn an_exported_function_keeps_its_symbol() {
        let mut hir = HirModule::new("m");
        hir.functions.push(function(1, "helper"));
        hir.functions.push(function(2, "helper"));
        hir.exports.push(renamed("helper", "helper"));
        hir.exported_functions.push(("helper".to_string(), 2));
        let names = build_func_registry(&hir, "m").func_names;
        assert_eq!(names[&2], "perry_fn_m__helper");
        assert_ne!(names[&1], "perry_fn_m__helper");
    }
}
