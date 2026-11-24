//! RULE RP: a function whose every completion returns one plain
//! parameter's binding returns, at each call, the argument THAT call
//! passes there (`FuncInfo::ret_param`, used by `Model::bind_args_bound`).
//!
//! ECMA-262 10.2.1.3 OrdinaryCallEvaluateBody / 10.2.11
//! FunctionDeclarationInstantiation: a simple parameter list binds each
//! parameter to the call's argument at its position (`undefined` past the
//! end) in a fresh environment of that call. A binding nothing reassigns
//! keeps that value for the whole run: no `=` / `++` / compound assignment
//! of it in the body or any closure of it (`LocalSet`, `Update`), no
//! redeclaration (`Let` of its id), no `with` fallback write (`WithSet`),
//! and no `arguments` object (a sloppy mapped one, 10.4.4.7, aliases the
//! parameters: a write `arguments[i] = v` changes the binding; a function
//! with any `arguments` object is out). A defaulted, rest or decorated
//! parameter is out (its binding is not the plain argument).
//!
//! Completion (14.10 ReturnStatement, 14.15.3 try/finally): every
//! `return` of the body (closures not entered) is `return p` with the same
//! `p`, and the last statement of the body is one, so the body cannot fall
//! off its end with `undefined`. A `finally` that returns something else
//! is a `return` of another value, so it is out; one that throws gives no
//! result. A generator or async function returns its generator / promise,
//! not the value: out.
use perry_hir::tdz_check::{for_each_stmt_deep, for_each_stmt_shallow, for_each_stmts_expr};
use perry_hir::{Expr, Param, Stmt, WithSetFallback};

/// The position of the parameter every return of `body` yields (RULE RP).
pub(crate) fn ret_param(params: &[Param], body: &[Stmt], suspends: bool) -> Option<u32> {
    if suspends || params.iter().any(|p| p.arguments_object.is_some()) {
        return None;
    }
    let Some(Stmt::Return(Some(Expr::LocalGet(id)), _)) = body.last() else {
        return None;
    };
    let id = *id;
    let i = params.iter().position(|p| p.id == id)?;
    let p = &params[i];
    if p.default.is_some() || p.is_rest || !p.decorators.is_empty() {
        return None;
    }
    let mut ok = true;
    for_each_stmt_shallow(body, &mut |s| {
        if let Stmt::Return(r, _) = s {
            if !matches!(r, Some(Expr::LocalGet(x)) if *x == id) {
                ok = false;
            }
        }
    });
    for_each_stmt_deep(body, &mut |s| {
        if matches!(s, Stmt::Let { id: x, .. } if *x == id) {
            ok = false;
        }
    });
    for_each_stmts_expr(body, &mut |e| match e {
        Expr::LocalSet(x, _) | Expr::Update { id: x, .. } if *x == id => ok = false,
        Expr::WithSet { fallback, .. } if matches!(fallback, WithSetFallback::Local(x) | WithSetFallback::SloppyImplicit(x) if *x == id) => {
            ok = false
        }
        _ => {}
    });
    ok.then_some(i as u32)
}
