//! Source-text registration for raw class method and accessor symbols.
//!
//! Class members are not ordinary closure wrappers, so their retained source
//! must be paired with the LLVM body symbol codegen actually emitted. Kept out
//! of `artifacts.rs` so that file remains below the repository's 2,000-line
//! limit.

use std::collections::HashSet;

use perry_hir::types::FuncId;
use perry_hir::Module as HirModule;

use crate::module::LlModule;

use super::helpers::{scoped_method_name, scoped_static_method_name};

pub(super) fn extend_class_method_source_text(
    hir: &HirModule,
    closures: &super::function_source_header::ClosureHeaders<'_>,
    module_prefix: &str,
    llmod: &LlModule,
    user_fn_source: &mut Vec<(String, String, bool)>,
) {
    // An HIR registry entry is not proof that this module emitted the body: a
    // cross-module or typed-only accessor can remain present without a local
    // definition. Referencing such a symbol from module initialization makes
    // LLVM reject the module, so `has_function` is the final authority.
    let mut seen: HashSet<String> = user_fn_source
        .iter()
        .map(|(symbol, _, _)| symbol.clone())
        .collect();
    let mut push_defined = |func_id: FuncId, symbol: String| {
        let Some(source) = hir.closure_source_text.get(&func_id) else {
            return;
        };
        if symbol.is_empty() || !llmod.has_function(&symbol) || !seen.insert(symbol.clone()) {
            return;
        }
        user_fn_source.push((
            symbol,
            super::function_source_header::retained_function_text(
                hir,
                closures,
                func_id,
                &source.text,
            ),
            source.is_non_strict_ordinary,
        ));
    };

    for class in &hir.classes {
        if class.id == 0 {
            continue;
        }
        for method in &class.methods {
            push_defined(
                method.id,
                scoped_method_name(module_prefix, &class.name, &method.name),
            );
        }
        for member in class
            .computed_members
            .iter()
            .filter(|member| !member.is_static)
        {
            push_defined(
                member.function.id,
                scoped_method_name(module_prefix, &class.name, &member.function.name),
            );
        }
        for (prop, getter) in &class.getters {
            let symbol = if class.static_accessor_fn_ids.contains(&getter.id) {
                scoped_static_method_name(
                    module_prefix,
                    class.id,
                    &class.name,
                    &format!("__get_{prop}"),
                )
            } else {
                scoped_method_name(
                    module_prefix,
                    &class.name,
                    &format!("__get_{}", getter.name),
                )
            };
            push_defined(getter.id, symbol);
        }
        for (prop, setter) in &class.setters {
            let symbol = if class.static_accessor_fn_ids.contains(&setter.id) {
                scoped_static_method_name(
                    module_prefix,
                    class.id,
                    &class.name,
                    &format!("__set_{prop}"),
                )
            } else {
                scoped_method_name(
                    module_prefix,
                    &class.name,
                    &format!("__set_{}", setter.name),
                )
            };
            push_defined(setter.id, symbol);
        }
        for method in &class.static_methods {
            push_defined(
                method.id,
                scoped_static_method_name(module_prefix, class.id, &class.name, &method.name),
            );
        }
        for member in class
            .computed_members
            .iter()
            .filter(|member| member.is_static)
        {
            push_defined(
                member.function.id,
                scoped_static_method_name(
                    module_prefix,
                    class.id,
                    &class.name,
                    &member.function.name,
                ),
            );
        }
    }
}

/// Collect retained `Function.prototype.toString` source text for every user
/// function this module emits, keyed by the same wrapper/closure symbol the
/// name registration uses.
///
/// Split out of `artifacts.rs::emit_module_artifacts` for the 2000-line cap
/// (#10579 took that file to 2006). Body verbatim; it returns the vector the
/// caller used to build in place.
pub(super) fn collect_user_fn_source(
    hir: &HirModule,
    func_names: &std::collections::HashMap<u32, String>,
    closures: &[(FuncId, perry_hir::Expr)],
    registered_fn_ids: &HashSet<FuncId>,
    materialized_closure_ids: &HashSet<FuncId>,
    module_prefix: &str,
    llmod: &LlModule,
) -> Vec<(String, String, bool)> {
    // #4101: collect retained function source text, keyed by the same
    // wrapper/closure symbol the name registration uses. Top-level functions
    // always have a `__perry_wrap_<name>` global (emitted unconditionally
    // above); inline closures only have a `perry_closure_*` global when
    // materialized, so gate those on `materialized_closure_ids` to avoid
    // referencing an undefined global (the #318/#343 clang-failure class).
    // #10574: resolve closure params/kind for functions that are not
    // `hir.functions` entries, so header mode keeps their names and parameters.
    let closure_headers = super::function_source_header::ClosureHeaders::new(closures);
    let mut user_fn_source: Vec<(String, String, bool)> = Vec::new();
    for f in &hir.functions {
        if let Some(src) = hir.closure_source_text.get(&f.id) {
            if let Some(sym) = func_names.get(&f.id) {
                user_fn_source.push((
                    format!("__perry_wrap_{}", sym),
                    super::function_source_header::retained_function_text(
                        hir,
                        &closure_headers,
                        f.id,
                        &src.text,
                    ),
                    src.is_non_strict_ordinary,
                ));
            }
        }
    }
    // Sorted, NOT raw `HashMap` iteration (#7038). The loop above walks
    // `hir.functions` (a `Vec`) and is already deterministic; this one keyed off
    // the map's iteration order, so the `@.str.N` numbering of the emitted
    // string constants was a per-process permutation. Same input, different
    // `.ll` on every run — which silently invalidates any A/B that compares raw
    // IR, a technique several representation and GC investigations relied on.
    // Emission order is the only thing that changes; sorting by `FuncId` makes
    // it stable without altering what is emitted.
    let mut materialized_closure_sources: Vec<(
        &perry_hir::types::FuncId,
        &perry_hir::FunctionSourceMetadata,
    )> = hir
        .closure_source_text
        .iter()
        .filter(|(func_id, _)| {
            !registered_fn_ids.contains(*func_id) && materialized_closure_ids.contains(*func_id)
        })
        .collect();
    materialized_closure_sources.sort_by_key(|(func_id, _)| **func_id);
    for (func_id, src) in materialized_closure_sources {
        let sym = format!("perry_closure_{}__{}", module_prefix, func_id);
        user_fn_source.push((
            sym,
            super::function_source_header::retained_function_text(
                hir,
                &closure_headers,
                *func_id,
                &src.text,
            ),
            src.is_non_strict_ordinary,
        ));
    }

    // #9468: method/accessor bodies are raw symbols rather than closure
    // wrappers. Pair retained MethodDefinition text only with symbols this
    // module actually emitted; the helper also preserves the file-size gate.
    extend_class_method_source_text(
        hir,
        &closure_headers,
        module_prefix,
        llmod,
        &mut user_fn_source,
    );
    user_fn_source
}
