//! Normalize export edges while borrowing declarations from the original HIR.

use std::collections::HashSet;

use perry_hir::{Export, ExportBindings, Import, Module, Stmt};

pub(super) struct ExportAnalysis<'a> {
    pub(super) module: &'a Module,
    pub(super) exports: Vec<Export>,
    pub(super) imports: Vec<Import>,
    entry_bindings: HashSet<&'a str>,
}

impl<'a> ExportAnalysis<'a> {
    pub(super) fn new(module: &'a Module) -> Self {
        let entry_bindings = perry_codegen::codegen::entry_outline::logical_entry_stmts(module)
            .into_iter()
            .filter_map(|stmt| match stmt {
                Stmt::Let { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        Self {
            module,
            exports: module.exports.clone(),
            imports: module.imports.clone(),
            entry_bindings,
        }
    }
}

impl ExportBindings for ExportAnalysis<'_> {
    fn exports(&self) -> &[Export] {
        &self.exports
    }

    fn imports(&self) -> &[Import] {
        &self.imports
    }

    fn defines_local_binding(&self, name: &str) -> bool {
        self.module.defines_local_binding(name) || self.entry_bindings.contains(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lower(source: &str, name: &str) -> Module {
        let filename = format!("/virtual/{name}.ts");
        let ast = perry_parser::parse_typescript(source, &filename).unwrap();
        perry_hir::lower_module(&ast, name, &filename).unwrap()
    }

    #[test]
    fn normalized_edges_resolve_imported_export_aliases() {
        let leaf = lower("export const value = 7;", "leaf");
        let bridge = lower(
            "import { value as local } from './leaf'; export { local as result };",
            "bridge",
        );
        let leaf_view = ExportAnalysis::new(&leaf);
        let mut bridge_view = ExportAnalysis::new(&bridge);
        bridge_view.imports[0].source = "leaf".into();
        let lookup = |name: &str| match name {
            "leaf" => Some(&leaf_view),
            "bridge" => Some(&bridge_view),
            _ => None,
        };
        let exports = perry_hir::flatten_exports("bridge", &lookup);
        assert_eq!(exports.len(), 1);
        assert_eq!(exports[0].name, "result");
        assert_eq!(exports[0].source_module, "leaf");
        assert_eq!(exports[0].source_local, "value");
    }

    #[test]
    fn outlined_entry_bindings_are_visible_but_unrelated_locals_are_not() {
        let module = lower(
            "function __perry_entry_chunk_0() { let value = 7; } \
             function unrelated() { let privateName = 9; } \
             __perry_entry_chunk_0(); export { value };",
            "outlined",
        );
        let view = ExportAnalysis::new(&module);
        assert!(view.defines_local_binding("value"));
        assert!(!view.defines_local_binding("privateName"));
    }
}
