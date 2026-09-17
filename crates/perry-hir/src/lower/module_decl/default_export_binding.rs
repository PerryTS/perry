//! Binding identity for `export default <identifier>` of a declared function
//! (#10434).
//!
//! `function F() {}; F.prototype.m = …; export default F;` must hand importers
//! the very function object the module calls `F`. Importers resolve a renamed
//! declared-function export through its origin (local) name, so the export row
//! has to be `{ local: "F", exported: "default" }` — the shape
//! `export { F as default }` already produces — rather than a
//! `{ local: "default" }` row that materializes a second function value with
//! none of F's expandos or prototype methods and no argument padding.

use std::collections::HashSet;

use swc_ecma_ast as ast;

use crate::ir::{Export, Module};
use crate::lower::LoweringContext;
use crate::types::FuncId;

/// The module-scope function name an `export default <expr>` refers to, when
/// `<expr>` is (modulo parentheses and erased TypeScript wrappers) a bare
/// identifier that resolves to the function `func_id`. Any other expression
/// that lowered to a `FuncRef` (a function expression) has no local binding
/// to share, so it keeps the synthetic `default` export row.
pub(super) fn default_export_function_binding(
    ctx: &LoweringContext,
    expr: &ast::Expr,
    func_id: FuncId,
) -> Option<String> {
    let mut expr = expr;
    loop {
        expr = match expr {
            ast::Expr::Paren(inner) => &inner.expr,
            ast::Expr::TsAs(inner) => &inner.expr,
            ast::Expr::TsNonNull(inner) => &inner.expr,
            ast::Expr::TsSatisfies(inner) => &inner.expr,
            ast::Expr::TsTypeAssertion(inner) => &inner.expr,
            ast::Expr::TsConstAssertion(inner) => &inner.expr,
            ast::Expr::TsInstantiation(inner) => &inner.expr,
            ast::Expr::Ident(ident) => {
                let name = ident.sym.to_string();
                return (ctx.lookup_func(&name) == Some(func_id)).then_some(name);
            }
            _ => return None,
        };
    }
}

/// Mark a function body exported when an export row names it as its local
/// binding (`export { F }`, `export { F as default }`, `export default F`).
///
/// The export arms flip `is_exported` on the functions already lowered, but a
/// hoisted declaration that appears AFTER its export clause
/// (`export { F as default }; function F() {}` or `export default F; function
/// F() {}`) is not in `module.functions` yet at that point. The CLI driver
/// only records a renamed export's origin name — the thing that keeps the
/// importer's value identical to the local `F` — for exported function
/// bodies, so the flag is settled once the whole module is lowered. Value
/// aliases (`export const g = F`) name `g`, not `F`, and stay as they were.
pub(crate) fn mark_exported_function_bodies(module: &mut Module) {
    if module.exported_functions.is_empty() {
        return;
    }
    let exported_ids: HashSet<FuncId> = module
        .exported_functions
        .iter()
        .map(|(_, id)| *id)
        .collect();
    let export_locals: HashSet<&str> = module
        .exports
        .iter()
        .filter_map(|export| match export {
            Export::Named { local, .. } => Some(local.as_str()),
            _ => None,
        })
        .collect();
    for func in &mut module.functions {
        if exported_ids.contains(&func.id) && export_locals.contains(func.name.as_str()) {
            func.is_exported = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::ir::{Export, Module};
    use crate::lower_module;
    use perry_diagnostics::SourceCache;
    use perry_parser::parse_typescript_with_cache;

    fn lower_src(src: &str) -> Module {
        let src = src.to_string();
        std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024)
            .spawn(move || {
                let mut cache = SourceCache::new();
                let parsed = parse_typescript_with_cache(&src, "test.ts", &mut cache)
                    .expect("parse should succeed");
                lower_module(&parsed.module, "test", "test.ts").expect("lowering should succeed")
            })
            .expect("spawn")
            .join()
            .expect("lowering thread")
    }

    fn default_rows(module: &Module) -> Vec<String> {
        module
            .exports
            .iter()
            .filter_map(|export| match export {
                Export::Named { local, exported } if exported == "default" => Some(local.clone()),
                _ => None,
            })
            .collect()
    }

    /// The function named `name` is exported and is what `default` resolves to.
    fn assert_default_is_function(module: &Module, name: &str) {
        assert_eq!(
            default_rows(module),
            vec![name.to_string()],
            "{:?}",
            module.exports
        );
        let func = module
            .functions
            .iter()
            .find(|f| f.name == name)
            .unwrap_or_else(|| panic!("function {name} not lowered"));
        assert!(func.is_exported, "{name} must be flagged exported");
        assert!(
            module
                .exported_functions
                .iter()
                .any(|(exported, id)| exported == "default" && *id == func.id),
            "{:?}",
            module.exported_functions
        );
    }

    #[test]
    fn export_default_identifier_exports_the_function_binding() {
        let module = lower_src(
            "function F(this: any, a: any) { this.a = a; }\n\
             F.prototype.m = function () { return 1; };\n\
             export default F;\n",
        );
        assert_default_is_function(&module, "F");
    }

    #[test]
    fn export_default_ahead_of_hoisted_declaration_marks_it_exported() {
        let module = lower_src("export default F;\nfunction F() { return 1; }\n");
        assert_default_is_function(&module, "F");
    }

    #[test]
    fn export_alias_ahead_of_hoisted_declaration_marks_it_exported() {
        let module = lower_src("export { F as default };\nfunction F() { return 1; }\n");
        assert_default_is_function(&module, "F");
    }

    #[test]
    fn export_default_sees_through_parens_and_type_assertions() {
        let module = lower_src("function F() { return 1; }\nexport default ((F as any)!);\n");
        assert_default_is_function(&module, "F");
    }

    #[test]
    fn value_alias_does_not_mark_the_aliased_body_exported() {
        let module = lower_src("export const g = F;\nfunction F() { return 1; }\n");
        let func = module.functions.iter().find(|f| f.name == "F").unwrap();
        assert!(
            module
                .exported_functions
                .iter()
                .any(|(name, id)| name == "g" && *id == func.id),
            "{:?}",
            module.exported_functions
        );
        assert!(
            !func.is_exported,
            "`export const g = F` exports `g`, not `F`"
        );
    }
}
