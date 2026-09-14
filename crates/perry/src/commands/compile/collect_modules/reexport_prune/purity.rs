//! Deliberately small proof for modules without a package sideEffects contract.
//! Only expressions whose evaluation cannot call user code, read a binding, or
//! throw are accepted. Dependencies are checked separately by the scanner.

use swc_ecma_ast as ast;

pub(super) fn module_is_pure(module: &ast::Module) -> bool {
    module.body.iter().all(|item| match item {
        ast::ModuleItem::Stmt(ast::Stmt::Empty(_)) => true,
        ast::ModuleItem::Stmt(ast::Stmt::Expr(stmt)) => expr_is_pure(&stmt.expr),
        ast::ModuleItem::Stmt(ast::Stmt::Decl(decl)) => decl_is_pure(decl),
        ast::ModuleItem::ModuleDecl(decl) => match decl {
            ast::ModuleDecl::Import(import) => {
                import.type_only
                    || (import.phase == ast::ImportPhase::Evaluation && import.with.is_none())
            }
            ast::ModuleDecl::ExportNamed(export) => export.with.is_none(),
            ast::ModuleDecl::ExportAll(export) => export.with.is_none(),
            ast::ModuleDecl::ExportDecl(export) => decl_is_pure(&export.decl),
            ast::ModuleDecl::ExportDefaultDecl(export) => match &export.decl {
                ast::DefaultDecl::Fn(function) => function_is_pure(&function.function),
                ast::DefaultDecl::TsInterfaceDecl(_) => true,
                _ => false,
            },
            ast::ModuleDecl::ExportDefaultExpr(export) => expr_is_pure(&export.expr),
            _ => false,
        },
        _ => false,
    })
}

fn decl_is_pure(decl: &ast::Decl) -> bool {
    match decl {
        ast::Decl::Fn(function) => function_is_pure(&function.function),
        ast::Decl::Var(var) => {
            var.declare
                || var.decls.iter().all(|decl| {
                    // Destructuring can invoke iterators/getters even when the
                    // initializer is a freshly created object or array.
                    matches!(decl.name, ast::Pat::Ident(_))
                        && decl.init.as_deref().is_none_or(expr_is_pure)
                })
        }
        ast::Decl::TsInterface(_) | ast::Decl::TsTypeAlias(_) => true,
        _ => false,
    }
}

fn function_is_pure(function: &ast::Function) -> bool {
    // Parameters and the body execute on invocation; decorators execute when
    // the enclosing module initializes. Do not inspect deferred function code.
    function.decorators.is_empty()
        && function
            .params
            .iter()
            .all(|param| param.decorators.is_empty())
}

fn expr_is_pure(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::Lit(lit) => !matches!(lit, ast::Lit::JSXText(_)),
        ast::Expr::Fn(function) => function_is_pure(&function.function),
        ast::Expr::Arrow(_) => true,
        ast::Expr::Paren(paren) => expr_is_pure(&paren.expr),
        ast::Expr::TsAs(expr) => expr_is_pure(&expr.expr),
        ast::Expr::TsSatisfies(expr) => expr_is_pure(&expr.expr),
        ast::Expr::TsTypeAssertion(expr) => expr_is_pure(&expr.expr),
        ast::Expr::TsConstAssertion(expr) => expr_is_pure(&expr.expr),
        ast::Expr::TsNonNull(expr) => expr_is_pure(&expr.expr),
        ast::Expr::Array(array) => array
            .elems
            .iter()
            .flatten()
            .all(|element| element.spread.is_none() && expr_is_pure(&element.expr)),
        ast::Expr::Object(object) => object.props.iter().all(|property| {
            let ast::PropOrSpread::Prop(property) = property else {
                return false;
            };
            match property.as_ref() {
                ast::Prop::KeyValue(property) => {
                    !matches!(property.key, ast::PropName::Computed(_))
                        && expr_is_pure(&property.value)
                }
                _ => false,
            }
        }),
        // Even an identifier read can throw (TDZ/unbound), property reads can
        // invoke getters, and arithmetic can coerce objects or mix BigInts.
        // Classes can evaluate extends, computed keys, decorators and statics.
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_inert_initialization_and_deferred_function_bodies() {
        for source in [
            "export { answer } from './answer.js'; export * from './unused.js';",
            "import './dependency.js'; export const unused = 99;",
            "'use strict'; export const values = [1, null, { ok: true }];",
            "export function unused(x = effect()) { return effect(x); }",
            "export const unused = async () => await import('./lazy.js');",
            "export default function () { throw new Error('called'); }",
            "export const unused = ({ value: 42 } as const);",
            "export interface Shape { x: number }; export type Alias = string;",
        ] {
            let module = perry_parser::parse_typescript(source, "fixture.ts").unwrap();
            assert!(module_is_pure(&module), "{source}");
        }
    }

    #[test]
    fn preserves_observable_or_unknown_initialization() {
        for source in [
            "console.log('effect'); export const unused = 99;",
            "export const unused = effect();",
            "export const unused = missing;",
            "export const unused = object.getter;",
            "export const unused = 1n + 1;",
            "export const unused = { [key]: 1 };",
            "export const unused = { ...object };",
            "export const unused = [...array];",
            "export const { unused } = { get unused() { effect(); } };",
            "export class Unused { static value = effect(); }",
            "export default class extends effect() {}",
            "export const unused = import('./lazy.js');",
            "await effect(); export const unused = 99;",
            "export enum Unused { Value = effect() }",
            "using unused = resource();",
            "import value from './asset.js' with { type: 'file' };",
        ] {
            let module = perry_parser::parse_typescript(source, "fixture.ts").unwrap();
            assert!(!module_is_pure(&module), "{source}");
        }
    }
}
