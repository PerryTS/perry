//! The HIR shape of a Temporal Dead Zone check on a module-level lexical
//! binding (#11826).
//!
//! A module-level `let`/`const` lives in a module global. A read that can run
//! before the declarator has initialized the binding carries a check in the
//! HIR itself, so every later pass and the backend see it in the tree:
//!
//! - `Call(ExternFuncRef(TDZ_CHECK), [LocalGet(id), String(name)])` throws
//!   `ReferenceError: Cannot access '<name>' before initialization` while the
//!   binding still holds the `TAG_TDZ` sentinel and evaluates to `undefined`
//!   otherwise. A guarded read is `Sequence([check, <read of id>])`.
//! - `Call(ExternFuncRef(TDZ_CHECK_THEN), [LocalGet(id), String(name), value])`
//!   evaluates `value` first, then performs the same check, and evaluates to
//!   `value`. A guarded write is `LocalSet(id, check_then)`: the right-hand
//!   side runs before PutValue throws, as the spec orders it.
//!
//! The `LocalGet(id)` argument names the binding whose slot is tested; it is
//! never a value read. A pass that rewrites reads (constant folding, copy
//! propagation) must leave it alone, because it must observe the slot
//! itself. A backend reads the slot's raw bits; the module global of every id
//! named by a check is seeded with `TAG_TDZ` instead of `undefined`.
//!
//! Reads that provably run after initialization carry no check. Which reads
//! need one is decided by `lower::module_tdz`.

use std::collections::HashSet;

use crate::ir::{Class, Expr, Function, Module, Stmt};
use crate::types::{LocalId, Type};
use crate::walker::walk_expr_children;

/// Check the binding, evaluate to `undefined`.
pub const TDZ_CHECK: &str = "js_tdz_check_binding";
/// Evaluate the value, check the binding, evaluate to the value.
pub const TDZ_CHECK_THEN: &str = "js_tdz_check_binding_then";
/// The unconditional throw a read that always runs in the dead zone becomes.
pub const TDZ_THROW: &str = "js_throw_reference_error_tdz";

fn extern_call(name: &str, args: Vec<Expr>) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::ExternFuncRef {
            name: name.to_string(),
            param_types: Vec::new(),
            return_type: Type::Any,
        }),
        args,
        type_args: Vec::new(),
        byte_offset: 0,
    }
}

/// `check(id)`: throws while `id` is uninitialized.
pub fn check(id: LocalId, name: &str) -> Expr {
    extern_call(
        TDZ_CHECK,
        vec![Expr::LocalGet(id), Expr::String(name.to_string())],
    )
}

/// `check_then(id, value)`: evaluates `value`, then throws while `id` is
/// uninitialized, else yields `value`.
pub fn check_then(id: LocalId, name: &str, value: Expr) -> Expr {
    extern_call(
        TDZ_CHECK_THEN,
        vec![Expr::LocalGet(id), Expr::String(name.to_string()), value],
    )
}

/// The ReferenceError a dead-zone access throws.
pub fn throw(name: &str) -> Expr {
    extern_call(TDZ_THROW, vec![Expr::String(name.to_string())])
}

/// The binding a check expression tests, when `expr` is one.
pub fn checked_binding(expr: &Expr) -> Option<LocalId> {
    let Expr::Call { callee, args, .. } = expr else {
        return None;
    };
    let Expr::ExternFuncRef { name, .. } = callee.as_ref() else {
        return None;
    };
    if name != TDZ_CHECK && name != TDZ_CHECK_THEN {
        return None;
    }
    match args.first() {
        Some(Expr::LocalGet(id)) => Some(*id),
        _ => None,
    }
}

/// The access a guarded read performs: `access` when `expr` is
/// `Sequence([check(id), access])`, `expr` itself otherwise (repeated, so a
/// chain of lifted checks is seen through). A check adds only a throw, never a
/// value, so an analysis that asks what an expression evaluates to (a static
/// path, a call target) looks through it; an analysis about effects or about
/// the binding's slot must not.
pub fn without_checks(mut expr: &Expr) -> &Expr {
    while let Expr::Sequence(parts) = expr {
        match parts.as_slice() {
            [check, access] if is_read_check(check) => expr = access,
            _ => break,
        }
    }
    expr
}

/// True when `expr` is a read check `check(id)` (not `check_then`).
pub fn is_read_check(expr: &Expr) -> bool {
    matches!(expr, Expr::Call { callee, args, .. }
        if args.len() == 2
            && matches!(callee.as_ref(), Expr::ExternFuncRef { name, .. } if name == TDZ_CHECK)
            && matches!(args.first(), Some(Expr::LocalGet(_))))
}

/// Visit the value operand of a `check_then` (the only operand that is an
/// ordinary expression). Returns false when `expr` is not a check.
pub fn for_each_check_value_mut(expr: &mut Expr, f: &mut dyn FnMut(&mut Expr)) -> bool {
    if checked_binding(expr).is_none() {
        return false;
    }
    if let Expr::Call { args, .. } = expr {
        for arg in args.iter_mut().skip(2) {
            f(arg);
        }
    }
    true
}

/// Every binding some check in `module` names: the module globals a backend
/// seeds with the TDZ sentinel.
pub fn checked_ids(module: &Module) -> HashSet<LocalId> {
    let mut ids = HashSet::new();
    for_each_module_expr(module, &mut |expr| {
        if let Some(id) = checked_binding(expr) {
            ids.insert(id);
        }
    });
    ids
}

/// Visit every expression of `module` (module init, functions, class
/// members, closure bodies), parents before children.
pub fn for_each_module_expr(module: &Module, f: &mut dyn FnMut(&Expr)) {
    fn expr(e: &Expr, f: &mut dyn FnMut(&Expr)) {
        f(e);
        if let Expr::Closure { body, .. } = e {
            stmts(body, f);
        }
        walk_expr_children(e, &mut |child| expr(child, f));
    }
    fn stmts(list: &[Stmt], f: &mut dyn FnMut(&Expr)) {
        for stmt in list {
            for_each_stmt_part(stmt, &mut |part| match part {
                Part::Expr(e) => expr(e, f),
                Part::Stmts(inner) => stmts(inner, f),
            });
        }
    }
    fn function(func: &Function, f: &mut dyn FnMut(&Expr)) {
        for param in &func.params {
            if let Some(default) = &param.default {
                expr(default, f);
            }
        }
        stmts(&func.body, f);
    }
    fn class(c: &Class, f: &mut dyn FnMut(&Expr)) {
        for decorator in &c.decorators {
            for arg in &decorator.args {
                expr(arg, f);
            }
        }
        if let Some(extends) = &c.extends_expr {
            expr(extends, f);
        }
        if let Some(ctor) = &c.constructor {
            function(ctor, f);
        }
        for m in c.methods.iter().chain(&c.static_methods) {
            function(m, f);
        }
        for (_, m) in c.getters.iter().chain(&c.setters) {
            function(m, f);
        }
        for member in &c.computed_members {
            expr(&member.key_expr, f);
            function(&member.function, f);
        }
        for field in c.fields.iter().chain(&c.static_fields) {
            if let Some(key) = &field.key_expr {
                expr(key, f);
            }
            if let Some(init) = &field.init {
                expr(init, f);
            }
        }
    }
    stmts(&module.init, f);
    for func in &module.functions {
        function(func, f);
    }
    for c in &module.classes {
        class(c, f);
    }
}

pub(crate) enum Part<'a> {
    Expr(&'a Expr),
    Stmts(&'a [Stmt]),
}

/// Visit a statement's expressions and nested statement lists, in order.
pub(crate) fn for_each_stmt_part<'a>(stmt: &'a Stmt, f: &mut dyn FnMut(Part<'a>)) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(init) = init {
                f(Part::Expr(init));
            }
        }
        Stmt::Expr(expr) | Stmt::Throw(expr) => f(Part::Expr(expr)),
        Stmt::Return(expr) => {
            if let Some(expr) = expr {
                f(Part::Expr(expr));
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            f(Part::Expr(condition));
            f(Part::Stmts(then_branch));
            if let Some(else_branch) = else_branch {
                f(Part::Stmts(else_branch));
            }
        }
        Stmt::While { condition, body } => {
            f(Part::Expr(condition));
            f(Part::Stmts(body));
        }
        Stmt::DoWhile { body, condition } => {
            f(Part::Stmts(body));
            f(Part::Expr(condition));
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init) = init {
                for_each_stmt_part(init, f);
            }
            if let Some(condition) = condition {
                f(Part::Expr(condition));
            }
            f(Part::Stmts(body));
            if let Some(update) = update {
                f(Part::Expr(update));
            }
        }
        Stmt::Labeled { body, .. } => for_each_stmt_part(body, f),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            f(Part::Stmts(body));
            if let Some(catch) = catch {
                f(Part::Stmts(&catch.body));
            }
            if let Some(finally) = finally {
                f(Part::Stmts(finally));
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            f(Part::Expr(discriminant));
            for case in cases {
                if let Some(test) = &case.test {
                    f(Part::Expr(test));
                }
                f(Part::Stmts(&case.body));
            }
        }
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_)
        | Stmt::ReleaseBoxes(_) => {}
    }
}
