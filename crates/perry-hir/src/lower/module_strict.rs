//! Runtime strictness follows the original source, including a CommonJS wrap.
use super::{is_raw_use_strict_directive, string_directive_stmt_lit};
use swc_ecma_ast as ast;

// Sloppy CommonJS uses this compiler-owned alias to opt out of ESM strictness.
// Strict CommonJS uses the equal-length __perry_cjs_strict_require alias and
// retains module strictness, without adding a directive that shifts spans.
fn is_sloppy_commonjs_wrap(module: &ast::Module) -> bool {
    module.body.iter().any(|item| {
        let ast::ModuleItem::ModuleDecl(ast::ModuleDecl::Import(import)) = item else {
            return false;
        };
        let source = import.src.value.to_string_lossy();
        source.strip_prefix("node:").unwrap_or(&source) == "module"
            && import.specifiers.iter().any(|specifier| {
                matches!(specifier, ast::ImportSpecifier::Named(named)
                    if named.local.sym == *"__perry_cjs_create_require"
                        && matches!(&named.imported,
                            Some(ast::ModuleExportName::Ident(name)) if name.sym == *"createRequire"))
            })
    })
}

pub(crate) fn module_has_strict_mode(ast_module: &ast::Module, source_file_path: &str) -> bool {
    // Node treats ESM format/package context and any source-text module as
    // strict even without a directive prologue (#6542).
    if !is_sloppy_commonjs_wrap(ast_module)
        && (perry_parser::file_is_es_module_by_format(source_file_path)
            || ast_module
                .body
                .iter()
                .any(|item| matches!(item, ast::ModuleItem::ModuleDecl(_))))
    {
        return true;
    }
    for item in &ast_module.body {
        let ast::ModuleItem::Stmt(stmt) = item else {
            break;
        };
        let Some(directive) = string_directive_stmt_lit(stmt) else {
            break;
        };
        if is_raw_use_strict_directive(directive) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    #[test]
    fn both_wrapper_aliases_keep_commonjs_identity_and_source_strictness() {
        for (alias, strict) in [
            ("__perry_cjs_create_require", false),
            ("__perry_cjs_strict_require", true),
        ] {
            let source = format!("import {{ createRequire as {alias} }} from 'node:module'; function receiver() {{ return this; }}");
            let parsed = perry_parser::parse_typescript(&source, "wrapped.cjs").unwrap();
            let hir = super::super::lower_module(&parsed, "wrapped", "wrapped.cjs").unwrap();
            assert!(hir.is_commonjs_wrap());
            assert_eq!(hir.init_is_strict, strict);
            assert_eq!(
                hir.functions
                    .iter()
                    .find(|f| f.name == "receiver")
                    .unwrap()
                    .is_strict,
                strict
            );
        }
    }
}
