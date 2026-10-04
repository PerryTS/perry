//! An unrebound parameter is the receiver of `a[k] op= v` itself; anything
//! that can give the parameter another value keeps the `__cmpd_base` snapshot.
//!
//! VERDICT tests: both desugars print the same numbers when the parameter is
//! never rebound, so only the presence of the temp tells them apart. The
//! behaviour where the snapshot matters is covered against node by
//! `test-files/test_gap_compound_param_base*.{ts,cts}`.

#![cfg(test)]

use crate::ir::Expr;
use crate::{Module, Stmt};
use perry_diagnostics::SourceCache;

fn lower(src: &str, file: &'static str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = perry_parser::parse_typescript_with_cache(&src, file, &mut cache)
                .expect("parse should succeed");
            crate::lower_module(&parsed.module, "test", file).expect("lower should succeed")
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

/// Every statement of `stmts`, descending into blocks, loops, ifs and
/// closure bodies.
fn all_stmts(stmts: &[Stmt]) -> Vec<Stmt> {
    let mut out = Vec::new();
    fn walk_expr(e: &Expr, out: &mut Vec<Stmt>) {
        if let Expr::Closure { body, .. } = e {
            walk(body, out);
        }
        crate::walker::walk_expr_children(e, &mut |c| walk_expr(c, out));
    }
    fn walk(stmts: &[Stmt], out: &mut Vec<Stmt>) {
        for stmt in stmts {
            out.push(stmt.clone());
            match stmt {
                Stmt::For { init, body, .. } => {
                    if let Some(init) = init {
                        walk(std::slice::from_ref(init.as_ref()), out);
                    }
                    walk(body, out);
                }
                Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => walk(body, out),
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    walk(then_branch, out);
                    if let Some(else_branch) = else_branch {
                        walk(else_branch, out);
                    }
                }
                Stmt::Let { init: Some(e), .. } | Stmt::Expr(e) => walk_expr(e, out),
                _ => {}
            }
        }
    }
    walk(stmts, &mut out);
    out
}

fn function<'m>(module: &'m Module, name: &str) -> &'m crate::Function {
    module
        .functions
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("no function {name}"))
}

/// Names of the `__cmpd_<tag>_*` temps in `stmts`.
fn temps(stmts: &[Stmt], tag: &str) -> Vec<String> {
    all_stmts(stmts)
        .into_iter()
        .filter_map(|s| match s {
            Stmt::Let { name, .. } if name.starts_with(&format!("__cmpd_{tag}_")) => Some(name),
            _ => None,
        })
        .collect()
}

/// The receivers of every member write in `stmts`.
fn written_objects(stmts: &[Stmt]) -> Vec<Expr> {
    all_stmts(stmts)
        .into_iter()
        .filter_map(|s| match s {
            Stmt::Expr(Expr::PropertySet { object, .. })
            | Stmt::Expr(Expr::IndexSet { object, .. }) => Some(*object),
            _ => None,
        })
        .collect()
}

/// `f`'s first parameter is the receiver of its compound write, unspilled.
fn writes_through_param(module: &Module, name: &str) -> bool {
    let f = function(module, name);
    let a = f.params[0].id;
    temps(&f.body, "base").is_empty()
        && written_objects(&f.body)
            .iter()
            .any(|o| matches!(o, Expr::LocalGet(id) if *id == a))
}

/// `f`'s compound write still goes through a `__cmpd_base` snapshot.
fn keeps_snapshot(module: &Module, name: &str) -> bool {
    let f = function(module, name);
    let a = f.params[0].id;
    !temps(&f.body, "base").is_empty()
        && !written_objects(&f.body)
            .iter()
            .any(|o| matches!(o, Expr::LocalGet(id) if *id == a))
}

#[test]
fn an_unrebound_param_is_the_receiver_even_with_a_spilled_key() {
    // The #11810 kernel shape: a computed key that has to be spilled.
    let m = lower(
        "export function f(a: Float64Array, i: number) {\n\
           for (let j = 0; j < 3; j++) { a[i * 7 + j] -= 2; a[i + j] *= 3; }\n\
         }\n",
        "unrebound_params.ts",
    );
    assert!(
        writes_through_param(&m, "f"),
        "{:#?}",
        function(&m, "f").body
    );
    assert!(
        !temps(&function(&m, "f").body, "key").is_empty(),
        "the computed key itself still needs its temp"
    );
}

#[test]
fn an_unrebound_param_key_needs_no_temp() {
    let m = lower(
        "export function f(a: number[], i: number) { a[i] += 1; }\n",
        "unrebound_params.ts",
    );
    let f = function(&m, "f");
    assert!(temps(&f.body, "base").is_empty() && temps(&f.body, "key").is_empty());
}

#[test]
fn arrow_object_and_class_method_params_are_the_receiver() {
    let m = lower(
        "export const g = (a: { x: number }) => { a.x += 1; };\n\
         export const o = { m(a: { x: number }) { a.x += 1; } };\n\
         export class C { m(a: { x: number }) { a.x += 1; } }\n",
        "unrebound_params.ts",
    );
    let mut all = Vec::new();
    for f in &m.functions {
        all.extend(f.body.clone());
    }
    for c in &m.classes {
        for f in &c.methods {
            all.extend(f.body.clone());
        }
    }
    all.extend(m.init.clone());
    assert!(temps(&all, "base").is_empty(), "{:?}", temps(&all, "base"));
}

#[test]
fn a_param_assigned_anywhere_keeps_its_snapshot() {
    let m = lower(
        "export function later(a: any, b: any) { a.x += 1; a = b; }\n\
         export function inRhs(a: any, b: any) { a.x += ((a = b), 1); }\n\
         export function inClosure(a: any, b: any) { const g = () => { a = b; }; a.x += (g(), 1); }\n\
         export function updated(a: any) { a.x += 1; a++; }\n\
         export function destructured(a: any, b: any) { a.x += 1; [a] = [b]; }\n\
         export function wrapped(a: any, b: any) { a.x += 1; (a as any) = b; }\n\
         export function inDefault(a: any, g = () => { a = 0; }) { a.x += (g(), 1); }\n\
         export function redeclared(a: any, b: any) { a.x += 1; { var a = b; } }\n\
         export function fnDecl(a: any) { a.x += 1; { function a() {} } }\n\
         export function forHead(a: any, xs: any[]) { a.x += 1; for (a of xs) {} }\n",
        "unrebound_params.ts",
    );
    for name in [
        "later",
        "inRhs",
        "inClosure",
        "updated",
        "destructured",
        "wrapped",
        "inDefault",
        "redeclared",
        "fnDecl",
        "forHead",
    ] {
        assert!(keeps_snapshot(&m, name), "{name} must keep its snapshot");
    }
}

#[test]
fn arguments_or_eval_keeps_every_snapshot() {
    // A sloppy function with a simple parameter list maps `arguments[0]` onto
    // `a`; `eval` can assign `a` directly.
    let m = lower(
        "function mapped(a, b) { a.x += ((arguments[0] = b), 1); }\n\
         function escapes(a, b) { const args = arguments; a.x += 1; }\n\
         function inArrow(a, b) { const g = () => { arguments[0] = b; }; a.x += (g(), 1); }\n\
         function evals(a, b) { a.x += (eval(\"a = b\"), 1); }\n\
         module.exports = { mapped, escapes, inArrow, evals };\n",
        "unrebound_params.cts",
    );
    for name in ["mapped", "escapes", "inArrow", "evals"] {
        assert!(keeps_snapshot(&m, name), "{name} must keep its snapshot");
    }
}

#[test]
fn shadowing_or_unrelated_names_do_not_matter() {
    // Assignments to other names, member writes through the parameter and a
    // type annotation naming it leave it unrebound.
    let m = lower(
        "export function f(a: any, b: any, cb: (a: number) => void) {\n\
           b = 2; a.y = b; a[0]++; a.x += 1;\n\
         }\n",
        "unrebound_params.ts",
    );
    assert!(
        writes_through_param(&m, "f"),
        "{:#?}",
        function(&m, "f").body
    );
}
