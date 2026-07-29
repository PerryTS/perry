//! Post-lowering static-type widening (#3576/#3575 family).
//!
//! A `var x = 2` infers `Type::Number` from its initializer, but a later
//! assignment — often from inside a closure, e.g. a sloppy accessor body
//! doing `x = this` (test262 10.4.3-1-56gs…61gs) — can store a value that
//! is certainly NOT a number. Codegen trusts the declared type and lowers
//! `x === o` as a float compare; NaN-boxed pointers are NaNs, so identity
//! comparisons go permanently false. Widen the declared type to `Any` for
//! every local that is assigned a certainly-non-numeric value anywhere in
//! the module, INCLUDING inside nested closure bodies (the expr walker
//! does not descend into `Expr::Closure` bodies, so this pass recurses
//! manually).
//!
//! Deliberately conservative: only RHS shapes that are statically known to
//! be non-numeric trigger widening, so number-typed fast paths for actual
//! numeric code are untouched (zero-regression requirement).

use crate::analysis::{infer_expr_type, HirTypeEnv};
use crate::ir::*;
use crate::types::{LocalId, Type};
use std::collections::HashSet;

fn type_is_certainly_object_like(ty: &Type) -> bool {
    ty.is_reference_like()
        || matches!(ty, Type::Void | Type::Null)
        || matches!(ty, Type::Union(variants) if variants.iter().all(type_is_certainly_object_like))
}

/// RHS shapes that can never evaluate to a JS number.
fn rhs_certainly_object_like(expr: &Expr, env: &HirTypeEnv) -> bool {
    matches!(
        expr,
        Expr::This
            | Expr::Object(_)
            | Expr::ObjectSpread { .. }
            | Expr::ObjectAssign { .. }
            | Expr::Array(_)
            | Expr::ArraySpread(_)
            | Expr::Closure { .. }
            | Expr::New { .. }
            | Expr::Null
            | Expr::Undefined
    ) || type_is_certainly_object_like(&infer_expr_type(expr, env))
}

/// RHS shapes that are primitives but not JS numbers (still wrong for a
/// `Type::Number`/`Type::Int32`-declared slot).
fn rhs_certainly_non_number_primitive(expr: &Expr, env: &HirTypeEnv) -> bool {
    let ty = infer_expr_type(expr, env);
    !matches!(ty, Type::Never)
        && ty.is_definitely_non_number_like()
        && !type_is_certainly_object_like(&ty)
}

#[derive(Default)]
struct WidenSets {
    /// Assigned an object-like value → widen any primitive declared type.
    object_like: HashSet<LocalId>,
    /// Assigned a primitive that is not a JS number → widen a numeric declared type.
    non_number_primitive: HashSet<LocalId>,
}

fn visit_expr(expr: &Expr, out: &mut WidenSets, env: &HirTypeEnv) {
    if let Expr::LocalSet(id, rhs) = expr {
        if rhs_certainly_object_like(rhs, env) {
            out.object_like.insert(*id);
        } else if rhs_certainly_non_number_primitive(rhs, env) {
            out.non_number_primitive.insert(*id);
        }
    }
    if let Expr::Closure { params, body, .. } = expr {
        for p in params {
            if let Some(d) = &p.default {
                visit_expr(d, out, env);
            }
        }
        for s in body {
            visit_stmt(s, out, env);
        }
        return;
    }
    crate::walker::walk_expr_children(expr, &mut |child| visit_expr(child, out, env));
}

fn visit_stmt(stmt: &Stmt, out: &mut WidenSets, env: &HirTypeEnv) {
    match stmt {
        Stmt::Let { init: Some(e), .. }
        | Stmt::Expr(e)
        | Stmt::Return(Some(e))
        | Stmt::Throw(e) => visit_expr(e, out, env),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            visit_expr(condition, out, env);
            for s in then_branch {
                visit_stmt(s, out, env);
            }
            if let Some(b) = else_branch {
                for s in b {
                    visit_stmt(s, out, env);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            visit_expr(condition, out, env);
            for s in body {
                visit_stmt(s, out, env);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(s) = init {
                visit_stmt(s, out, env);
            }
            if let Some(e) = condition {
                visit_expr(e, out, env);
            }
            if let Some(e) = update {
                visit_expr(e, out, env);
            }
            for s in body {
                visit_stmt(s, out, env);
            }
        }
        Stmt::Labeled { body, .. } => visit_stmt(body, out, env),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                visit_stmt(s, out, env);
            }
            if let Some(c) = catch {
                for s in &c.body {
                    visit_stmt(s, out, env);
                }
            }
            if let Some(f) = finally {
                for s in f {
                    visit_stmt(s, out, env);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            visit_expr(discriminant, out, env);
            for c in cases {
                if let Some(t) = &c.test {
                    visit_expr(t, out, env);
                }
                for s in &c.body {
                    visit_stmt(s, out, env);
                }
            }
        }
        _ => {}
    }
}

fn widen_lets_stmt(stmt: &mut Stmt, sets: &WidenSets) {
    match stmt {
        Stmt::Let { id, ty, .. } => {
            let widen = match ty {
                Type::Number | Type::Int32 => {
                    sets.object_like.contains(id) || sets.non_number_primitive.contains(id)
                }
                Type::String | Type::Boolean => sets.object_like.contains(id),
                _ => false,
            };
            if widen {
                *ty = Type::Any;
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            for s in then_branch {
                widen_lets_stmt(s, sets);
            }
            if let Some(b) = else_branch {
                for s in b {
                    widen_lets_stmt(s, sets);
                }
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            for s in body {
                widen_lets_stmt(s, sets);
            }
        }
        Stmt::For { init, body, .. } => {
            if let Some(s) = init {
                widen_lets_stmt(s, sets);
            }
            for s in body {
                widen_lets_stmt(s, sets);
            }
        }
        Stmt::Labeled { body, .. } => widen_lets_stmt(body, sets),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                widen_lets_stmt(s, sets);
            }
            if let Some(c) = catch {
                for s in &mut c.body {
                    widen_lets_stmt(s, sets);
                }
            }
            if let Some(f) = finally {
                for s in f {
                    widen_lets_stmt(s, sets);
                }
            }
        }
        Stmt::Switch { cases, .. } => {
            for c in cases {
                for s in &mut c.body {
                    widen_lets_stmt(s, sets);
                }
            }
        }
        _ => {}
    }
}

/// Module-scoped widening state.
///
/// Callers collect assignments from every body first, then apply the final
/// sets back to every body. HIR `LocalId`s are module-unique, so an assignment
/// inside a nested closure can safely widen the matching `Stmt::Let` even when
/// that declaration lives in another body.
pub(crate) struct TypeWidening {
    env: HirTypeEnv,
    sets: WidenSets,
}

impl TypeWidening {
    pub(crate) fn from_module(module: &Module) -> Self {
        Self {
            env: HirTypeEnv::from_module(module),
            sets: WidenSets::default(),
        }
    }

    pub(crate) fn collect(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            visit_stmt(s, &mut self.sets, &self.env);
        }
    }

    /// Like [`collect`], but with `this`/`super` bound to `class_name` so member
    /// reads (`this.name`, `super.x`) inside class method bodies infer real
    /// types instead of `Any` — otherwise a numeric local assigned from a
    /// known-string member would silently keep its `Number` type.
    pub(crate) fn collect_in_class(&mut self, class_name: &str, stmts: &[Stmt]) {
        let prev = self.env.set_current_class(Some(class_name.to_string()));
        for s in stmts {
            visit_stmt(s, &mut self.sets, &self.env);
        }
        self.env.set_current_class(prev);
    }

    pub(crate) fn apply(&self, stmts: &mut [Stmt]) {
        if self.sets.object_like.is_empty() && self.sets.non_number_primitive.is_empty() {
            return;
        }
        for s in stmts {
            widen_lets_stmt(s, &self.sets);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn let_ty(stmts: &[Stmt], id: LocalId) -> &Type {
        stmts
            .iter()
            .find_map(|stmt| match stmt {
                Stmt::Let {
                    id: stmt_id, ty, ..
                } if *stmt_id == id => Some(ty),
                _ => None,
            })
            .expect("missing let statement")
    }

    #[test]
    fn widens_numeric_local_assigned_string_typed_local() {
        let mut module = Module::new("type-widening-test");
        module.init = vec![
            Stmt::Let {
                id: 1,
                name: "n".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Number(1.0)),
            },
            Stmt::Let {
                id: 2,
                name: "s".to_string(),
                ty: Type::String,
                mutable: false,
                init: Some(Expr::String("value".to_string())),
            },
            Stmt::Expr(Expr::LocalSet(1, Box::new(Expr::LocalGet(2)))),
        ];

        let mut widening = TypeWidening::from_module(&module);
        widening.collect(&module.init);
        widening.apply(&mut module.init);

        assert_eq!(let_ty(&module.init, 1), &Type::Any);
        assert_eq!(let_ty(&module.init, 2), &Type::String);
    }

    #[test]
    fn preserves_numeric_local_assigned_number_typed_local() {
        let mut module = Module::new("type-widening-test");
        module.init = vec![
            Stmt::Let {
                id: 1,
                name: "target".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Number(1.0)),
            },
            Stmt::Let {
                id: 2,
                name: "source".to_string(),
                ty: Type::Number,
                mutable: false,
                init: Some(Expr::Number(2.0)),
            },
            Stmt::Expr(Expr::LocalSet(1, Box::new(Expr::LocalGet(2)))),
        ];

        let mut widening = TypeWidening::from_module(&module);
        widening.collect(&module.init);
        widening.apply(&mut module.init);

        assert_eq!(let_ty(&module.init, 1), &Type::Number);
        assert_eq!(let_ty(&module.init, 2), &Type::Number);
    }

    #[test]
    fn widens_numeric_local_assigned_named_object_typed_local() {
        let mut module = Module::new("type-widening-test");
        module.init = vec![
            Stmt::Let {
                id: 1,
                name: "n".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Number(1.0)),
            },
            Stmt::Let {
                id: 2,
                name: "d".to_string(),
                ty: Type::Named("Date".to_string()),
                mutable: false,
                init: Some(Expr::DateNew(vec![])),
            },
            Stmt::Expr(Expr::LocalSet(1, Box::new(Expr::LocalGet(2)))),
        ];

        let mut widening = TypeWidening::from_module(&module);
        widening.collect(&module.init);
        widening.apply(&mut module.init);

        assert_eq!(let_ty(&module.init, 1), &Type::Any);
    }

    #[test]
    fn widens_numeric_local_assigned_bigint_typed_local() {
        let mut module = Module::new("type-widening-test");
        module.init = vec![
            Stmt::Let {
                id: 1,
                name: "n".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Number(1.0)),
            },
            Stmt::Let {
                id: 2,
                name: "b".to_string(),
                ty: Type::BigInt,
                mutable: false,
                init: Some(Expr::BigInt("1".to_string())),
            },
            Stmt::Expr(Expr::LocalSet(1, Box::new(Expr::LocalGet(2)))),
        ];

        let mut widening = TypeWidening::from_module(&module);
        widening.collect(&module.init);
        widening.apply(&mut module.init);

        assert_eq!(let_ty(&module.init, 1), &Type::Any);
    }

    #[test]
    fn widens_numeric_local_assigned_bigint_expression() {
        let mut module = Module::new("type-widening-test");
        module.init = vec![
            Stmt::Let {
                id: 1,
                name: "n".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Number(1.0)),
            },
            Stmt::Expr(Expr::LocalSet(1, Box::new(Expr::ProcessHrtimeBigint))),
        ];

        let mut widening = TypeWidening::from_module(&module);
        widening.collect(&module.init);
        widening.apply(&mut module.init);

        assert_eq!(let_ty(&module.init, 1), &Type::Any);
    }

    #[test]
    fn widens_numeric_local_assigned_optional_string_array_element() {
        let mut module = Module::new("type-widening-test");
        module.init = vec![
            Stmt::Let {
                id: 1,
                name: "n".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Number(1.0)),
            },
            Stmt::Let {
                id: 2,
                name: "items".to_string(),
                ty: Type::Array(Box::new(Type::String)),
                mutable: true,
                init: Some(Expr::Array(vec![])),
            },
            Stmt::Expr(Expr::LocalSet(1, Box::new(Expr::ArrayPop(2)))),
        ];

        let mut widening = TypeWidening::from_module(&module);
        widening.collect(&module.init);
        widening.apply(&mut module.init);

        assert_eq!(let_ty(&module.init, 1), &Type::Any);
    }

    #[test]
    fn widens_primitive_local_assigned_object_null_union() {
        let mut module = Module::new("type-widening-test");
        module.init = vec![
            Stmt::Let {
                id: 1,
                name: "s".to_string(),
                ty: Type::String,
                mutable: true,
                init: Some(Expr::String("value".to_string())),
            },
            Stmt::Expr(Expr::LocalSet(
                1,
                Box::new(Expr::ObjectGetPrototypeOf(Box::new(Expr::Object(vec![])))),
            )),
        ];

        let mut widening = TypeWidening::from_module(&module);
        widening.collect(&module.init);
        widening.apply(&mut module.init);

        assert_eq!(let_ty(&module.init, 1), &Type::Any);
    }

    #[test]
    fn preserves_numeric_local_assigned_never_expression() {
        let mut module = Module::new("type-widening-test");
        module.init = vec![
            Stmt::Let {
                id: 1,
                name: "n".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Number(1.0)),
            },
            Stmt::Expr(Expr::LocalSet(1, Box::new(Expr::ProcessExit(None)))),
        ];

        let mut widening = TypeWidening::from_module(&module);
        widening.collect(&module.init);
        widening.apply(&mut module.init);

        assert_eq!(let_ty(&module.init, 1), &Type::Number);
    }
}
