//! #2309 Stage 2: build-time `process.env` folding + dead-branch elimination.
//!
//! Runs (under tree-shaking) on each freshly-lowered module BEFORE its dynamic
//! `import()` edges are registered, so a dead `import()` inside a statically
//! false branch is removed before it enters the module graph. This is how
//! `ink`'s `if (process.env['DEV'] === 'true') { await import('./devtools.js') }`
//! drops the entire `ws`/`react-devtools` subtree.
//!
//! It evaluates the condition of an `if` statement against build-time-known
//! `process.env.<NAME>` values — explicit `perry.define` entries, plus an
//! implicit `NODE_ENV → "production"` default applied only to `node_modules`
//! code. When the condition const-folds to a definite truthiness, the dead
//! branch (and everything in it) is spliced out. Anything not provably
//! constant is left untouched — never folds an un-configured runtime env read.
//!
//! Scope (PR): statement-level `if` in module init + function bodies and the
//! nested blocks they contain (`if`/`while`/`for`/`do`/`try`/`switch`/
//! labeled). Ternary `import()` gating and closures nested inside expressions
//! are out of scope for now (the ink gate is a statement-level `if`).

use std::collections::HashMap;

use perry_hir::{CompareOp, Expr, LogicalOp, Module, Stmt, UnaryOp};

use super::DefineValue;

/// Fold build-time env branches in a module. No-op when there is nothing to
/// resolve (no defines and not node_modules code).
pub(super) fn fold_env_branches(
    module: &mut Module,
    define: &HashMap<String, DefineValue>,
    is_node_modules: bool,
) {
    if define.is_empty() && !is_node_modules {
        return;
    }
    let env = Env {
        define,
        is_node_modules,
    };
    fold_stmts(&mut module.init, &env);
    for f in &mut module.functions {
        fold_stmts(&mut f.body, &env);
    }
}

struct Env<'a> {
    define: &'a HashMap<String, DefineValue>,
    is_node_modules: bool,
}

impl Env<'_> {
    /// Resolve `process.env.<key>` to a build-time constant, if known.
    fn lookup(&self, key: &str) -> Option<Const> {
        if let Some(dv) = self.define.get(&format!("process.env.{key}")) {
            return Some(match dv {
                DefineValue::Str(s) => Const::Str(s.clone()),
                DefineValue::Bool(b) => Const::Bool(*b),
                DefineValue::Number(n) => Const::Num(*n),
                DefineValue::Null => Const::Null,
            });
        }
        // Implicit production default for dependency code only (#2309 decision).
        if self.is_node_modules && key == "NODE_ENV" {
            return Some(Const::Str("production".to_string()));
        }
        None
    }
}

#[derive(Clone)]
enum Const {
    Str(String),
    Bool(bool),
    Num(f64),
    Null,
    Undef,
}

impl Const {
    fn truthy(&self) -> bool {
        match self {
            Const::Str(s) => !s.is_empty(),
            Const::Bool(b) => *b,
            Const::Num(n) => *n != 0.0 && !n.is_nan(),
            Const::Null | Const::Undef => false,
        }
    }
}

/// Splice out statically-dead `if` branches in a statement list, recursing
/// into nested blocks.
fn fold_stmts(stmts: &mut Vec<Stmt>, env: &Env) {
    let mut out: Vec<Stmt> = Vec::with_capacity(stmts.len());
    for mut stmt in std::mem::take(stmts) {
        recurse_stmt(&mut stmt, env);
        if let Stmt::If {
            condition,
            then_branch,
            else_branch,
        } = &mut stmt
        {
            if let Some(c) = try_const(condition, env) {
                // Condition is build-time-constant: keep only the live branch.
                let mut taken = if c.truthy() {
                    std::mem::take(then_branch)
                } else {
                    else_branch.take().unwrap_or_default()
                };
                fold_stmts(&mut taken, env);
                out.extend(taken);
                continue;
            }
        }
        out.push(stmt);
    }
    *stmts = out;
}

/// Recurse into the nested statement lists a statement owns (so a dead branch
/// deeper in the tree is also eliminated).
fn recurse_stmt(stmt: &mut Stmt, env: &Env) {
    match stmt {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            fold_stmts(then_branch, env);
            if let Some(eb) = else_branch {
                fold_stmts(eb, env);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
            fold_stmts(body, env);
        }
        Stmt::Labeled { body, .. } => recurse_stmt(body, env),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            fold_stmts(body, env);
            if let Some(c) = catch {
                fold_stmts(&mut c.body, env);
            }
            if let Some(f) = finally {
                fold_stmts(f, env);
            }
        }
        Stmt::Switch { cases, .. } => {
            for case in cases {
                fold_stmts(&mut case.body, env);
            }
        }
        _ => {}
    }
}

/// Try to evaluate an expression to a build-time constant. Returns `None` for
/// anything not provably constant — the safe default (no fold).
fn try_const(e: &Expr, env: &Env) -> Option<Const> {
    match e {
        Expr::EnvGet(key) => env.lookup(key),
        Expr::String(s) => Some(Const::Str(s.clone())),
        Expr::Bool(b) => Some(Const::Bool(*b)),
        Expr::Number(n) => Some(Const::Num(*n)),
        Expr::Null => Some(Const::Null),
        Expr::Undefined => Some(Const::Undef),
        Expr::Unary {
            op: UnaryOp::Not,
            operand,
        } => try_const(operand, env).map(|c| Const::Bool(!c.truthy())),
        Expr::Logical { op, left, right } => {
            let l = try_const(left, env)?;
            match op {
                LogicalOp::And => {
                    if !l.truthy() {
                        Some(l)
                    } else {
                        try_const(right, env)
                    }
                }
                LogicalOp::Or => {
                    if l.truthy() {
                        Some(l)
                    } else {
                        try_const(right, env)
                    }
                }
                LogicalOp::Coalesce => match l {
                    Const::Null | Const::Undef => try_const(right, env),
                    other => Some(other),
                },
            }
        }
        Expr::Compare { op, left, right } => {
            let l = try_const(left, env)?;
            let r = try_const(right, env)?;
            eval_compare(*op, &l, &r)
        }
        _ => None,
    }
}

/// Evaluate a comparison of two constants. Strict eq/ne are total (cross-type
/// ⇒ unequal); loose eq/ne and ordering only fold within matching types to
/// avoid replicating full JS coercion semantics. `None` ⇒ don't fold.
fn eval_compare(op: CompareOp, l: &Const, r: &Const) -> Option<Const> {
    let strict_equal = match (l, r) {
        (Const::Str(a), Const::Str(b)) => Some(a == b),
        (Const::Bool(a), Const::Bool(b)) => Some(a == b),
        (Const::Num(a), Const::Num(b)) => Some(a == b),
        (Const::Null, Const::Null) | (Const::Undef, Const::Undef) => Some(true),
        // Different constant kinds are never `===`.
        _ => Some(false),
    };
    match op {
        CompareOp::Eq => strict_equal.map(Const::Bool),
        CompareOp::Ne => strict_equal.map(|b| Const::Bool(!b)),
        CompareOp::LooseEq | CompareOp::LooseNe => {
            // Only fold loose (in)equality between same-typed constants; the
            // null/undefined cross-case and numeric coercion are left to
            // runtime to stay correct.
            let same_type_eq = match (l, r) {
                (Const::Str(a), Const::Str(b)) => Some(a == b),
                (Const::Bool(a), Const::Bool(b)) => Some(a == b),
                (Const::Num(a), Const::Num(b)) => Some(a == b),
                (Const::Null | Const::Undef, Const::Null | Const::Undef) => Some(true),
                _ => None,
            }?;
            Some(Const::Bool(if matches!(op, CompareOp::LooseEq) {
                same_type_eq
            } else {
                !same_type_eq
            }))
        }
        CompareOp::Lt | CompareOp::Le | CompareOp::Gt | CompareOp::Ge => match (l, r) {
            (Const::Num(a), Const::Num(b)) => Some(Const::Bool(match op {
                CompareOp::Lt => a < b,
                CompareOp::Le => a <= b,
                CompareOp::Gt => a > b,
                CompareOp::Ge => a >= b,
                _ => unreachable!(),
            })),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_with(pairs: &[(&str, DefineValue)], nm: bool) -> HashMap<String, DefineValue> {
        let _ = nm;
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    fn if_stmt(cond: Expr, then_n: usize, else_n: usize) -> Stmt {
        Stmt::If {
            condition: cond,
            then_branch: (0..then_n).map(|_| Stmt::Break).collect(),
            else_branch: if else_n == 0 {
                None
            } else {
                Some((0..else_n).map(|_| Stmt::Continue).collect())
            },
        }
    }

    fn count(stmts: &[Stmt]) -> (usize, usize) {
        let breaks = stmts.iter().filter(|s| matches!(s, Stmt::Break)).count();
        let conts = stmts.iter().filter(|s| matches!(s, Stmt::Continue)).count();
        (breaks, conts)
    }

    #[test]
    fn dead_branch_eliminated_via_define() {
        // if (process.env.DEV === "true") { B } else { C } with DEV="false"
        let define = env_with(
            &[("process.env.DEV", DefineValue::Str("false".into()))],
            false,
        );
        let cond = Expr::Compare {
            op: CompareOp::Eq,
            left: Box::new(Expr::EnvGet("DEV".into())),
            right: Box::new(Expr::String("true".into())),
        };
        let mut stmts = vec![if_stmt(cond, 2, 1)];
        let env = Env {
            define: &define,
            is_node_modules: false,
        };
        fold_stmts(&mut stmts, &env);
        // condition false ⇒ then(2 breaks) dropped, else(1 continue) kept.
        assert_eq!(count(&stmts), (0, 1));
    }

    #[test]
    fn node_env_production_default_keeps_then() {
        // if (process.env.NODE_ENV === "production") { B } in node_modules.
        let define = HashMap::new();
        let cond = Expr::Compare {
            op: CompareOp::Eq,
            left: Box::new(Expr::EnvGet("NODE_ENV".into())),
            right: Box::new(Expr::String("production".into())),
        };
        let mut stmts = vec![if_stmt(cond, 2, 0)];
        let env = Env {
            define: &define,
            is_node_modules: true,
        };
        fold_stmts(&mut stmts, &env);
        assert_eq!(count(&stmts), (2, 0), "production-true branch kept");
    }

    #[test]
    fn unconfigured_env_not_folded() {
        // No define, not node_modules ⇒ condition stays runtime, if untouched.
        let define = HashMap::new();
        let cond = Expr::Compare {
            op: CompareOp::Eq,
            left: Box::new(Expr::EnvGet("DEBUG".into())),
            right: Box::new(Expr::String("1".into())),
        };
        let mut stmts = vec![if_stmt(cond, 2, 1)];
        let env = Env {
            define: &define,
            is_node_modules: false,
        };
        fold_stmts(&mut stmts, &env);
        // If preserved (1 If stmt), branches intact.
        assert_eq!(stmts.len(), 1);
        assert!(matches!(stmts[0], Stmt::If { .. }));
    }
}
