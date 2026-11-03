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

/// #13558: does this module's *source* have the ES-module goal?
///
/// True for an ESM format/package context (`.mjs`/`.mts`, or a nearest
/// `package.json` with `"type": "module"`) and for any source-level
/// `import`/`export` module declaration. False for a CommonJS file (including
/// one the compiler already wrapped as CommonJS, whose synthetic import must
/// not count).
///
/// This is deliberately the SAME predicate that already decided strictness
/// (#6542), so a file's module goal and its strictness can never disagree.
/// Two Node module-syntax detection cases stay outside it, as they did for
/// strictness: a source whose only module syntax is `import.meta`, or only a
/// top-level `await` (both are `ast::Expr` forms, not module declarations).
/// Closing those is a detection change of its own, not a top-level-`this` one.
///
/// The distinction matters for module top-level `this`, which is `undefined`
/// in a Module Environment Record (ECMA-262 §16.2.1.6.4) and the CJS
/// `module.exports` stand-in otherwise — a *module goal* decision, not a
/// strictness one.
///
/// `is_sloppy_commonjs_wrap` is deliberately the only exclusion: a CJS source
/// with `"use strict"` is strict but still has CommonJS top-level `this`, and
/// a wrapped body already sits inside the wrapper IIFE, so this predicate is
/// only consulted for the module's own top level.
pub(crate) fn module_has_module_goal(ast_module: &ast::Module, source_file_path: &str) -> bool {
    !is_sloppy_commonjs_wrap(ast_module)
        && (perry_parser::file_is_es_module_by_format(source_file_path)
            || ast_module
                .body
                .iter()
                .any(|item| matches!(item, ast::ModuleItem::ModuleDecl(_))))
}

pub(crate) fn module_has_strict_mode(ast_module: &ast::Module, source_file_path: &str) -> bool {
    // Node treats ESM format/package context and any source-text module as
    // strict even without a directive prologue (#6542).
    if module_has_module_goal(ast_module, source_file_path) {
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
    use super::module_has_module_goal;

    /// #13558: the module goal (which decides a module's top-level `this`) comes
    /// from exactly two inputs — the file's format/package context and its own
    /// `import`/`export` syntax.
    #[test]
    fn module_goal_follows_format_and_source_syntax() {
        let parse = |src: &str| perry_parser::parse_typescript(src, "probe.ts").unwrap();
        let plain = parse("console.log(this);");
        let with_export = parse("export const x = 1;");
        let with_import = parse("import 'node:path';");

        // Format: an explicit ESM extension is a module with no module syntax.
        assert!(module_has_module_goal(&plain, "/no/such/dir/main.mjs"));
        assert!(module_has_module_goal(&plain, "/no/such/dir/main.mts"));
        // Format: an explicit CommonJS extension is never a module by itself.
        assert!(!module_has_module_goal(&plain, "/no/such/dir/main.cjs"));
        // An ambiguous extension with no declaring package.json is CommonJS.
        assert!(!module_has_module_goal(
            &plain,
            "/nonexistent-perry-13558-root/main.ts"
        ));
        // Source syntax: `import`/`export` makes the source a module whatever
        // its extension says.
        assert!(module_has_module_goal(
            &with_export,
            "/nonexistent-perry-13558-root/main.ts"
        ));
        assert!(module_has_module_goal(
            &with_import,
            "/nonexistent-perry-13558-root/main.ts"
        ));
    }

    /// The wrapped form a CommonJS file takes inside the compiler is *not* a
    /// module, so its body must keep CommonJS top-level `this`: the module goal
    /// decision has to survive the synthetic `import` the wrap adds.
    #[test]
    fn commonjs_wrap_keeps_commonjs_identity() {
        let wrapped = "import { createRequire as __perry_cjs_create_require } from 'node:module'; const t = this;";
        let parsed = perry_parser::parse_typescript(wrapped, "wrapped.cjs").unwrap();
        // The synthetic import alone would read as module syntax; the wrap alias
        // is what keeps the body CommonJS.
        assert!(!module_has_module_goal(&parsed, "wrapped.cjs"));
    }

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
