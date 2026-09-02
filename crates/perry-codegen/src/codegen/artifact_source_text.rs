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
    module_prefix: &str,
    llmod: &LlModule,
    user_fn_source: &mut Vec<(String, String)>,
) {
    // An HIR registry entry is not proof that this module emitted the body: a
    // cross-module or typed-only accessor can remain present without a local
    // definition. Referencing such a symbol from module initialization makes
    // LLVM reject the module, so `has_function` is the final authority.
    let mut seen: HashSet<String> = user_fn_source
        .iter()
        .map(|(symbol, _)| symbol.clone())
        .collect();
    let mut push_defined = |func_id: FuncId, symbol: String| {
        let Some(source) = hir.closure_source_text.get(&func_id) else {
            return;
        };
        if symbol.is_empty() || !llmod.has_function(&symbol) || !seen.insert(symbol.clone()) {
            return;
        }
        user_fn_source.push((symbol, source.clone()));
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
