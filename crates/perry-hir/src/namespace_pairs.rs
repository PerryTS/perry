//! MATHC1: the namespace pairs of the facts lowering, each replaced by the ONE
//! arm its call site's verdict picks.
//!
//! With the whole-program facts on, a direct builtin namespace call is lowered
//! as `NamespacePatched(ns) ? globalThis.<ns>.<m>(args) : <intrinsic>`
//! (`lower/expr_call/patched_builtin_call.rs`), so the solver sees its member
//! read. Once the facts are solved, the pair becomes the intrinsic when the
//! generic call is proven to read the original builtin (ECMA-262 13.3.6.1:
//! the call evaluates the member `N.m` and calls that value), else the generic
//! call alone. No pair is left for a shadow-bit test, and the intrinsic is the
//! same HIR the call lowers to without the pair.
//!
//! MATHC3: a pair inside a body the program never runs (the activation's
//! inactive predicate, `perry_wpfacts::BodyReach::inactive`, the one the
//! dead-body removal reads) takes the intrinsic too: the solve holds no site
//! for it, no execution observes its lowering, and the intrinsic is the HIR a
//! build without the pair has there (the plain member call would only add
//! generic-call code to a body that never runs).

use std::collections::BTreeSet;

use crate::ir::{Expr, Module, Stmt};

/// What picks a pair's arm: the proven generic-call offsets of the module,
/// the function ids of its bodies the program never runs, and whether the
/// walk is inside such a body.
#[derive(Clone, Copy)]
struct Arms<'a> {
    proven: &'a BTreeSet<u32>,
    inactive: &'a BTreeSet<u32>,
    dead: bool,
}

impl Arms<'_> {
    /// The arms of the body with function id `id`: dead when it is inactive
    /// (a body nested in a dead one stays dead: only running its parent
    /// creates it).
    fn enter(self, id: u32) -> Self {
        Arms { dead: self.dead || self.inactive.contains(&id), ..self }
    }
}

/// The generic call's member offset when `generic` is a pair's generic arm
/// `globalThis.<ns>.<m>(args…)`.
fn pair_offset(ns: &str, generic: &Expr) -> Option<u32> {
    let (Expr::Call { callee, .. } | Expr::CallSpread { callee, .. }) = generic else {
        return None;
    };
    let Expr::PropertyGet { object, byte_offset, .. } = callee.as_ref() else {
        return None;
    };
    match object.as_ref() {
        Expr::PropertyGet { object: g, property, .. }
            if matches!(g.as_ref(), Expr::GlobalThisExpr) && property == ns =>
        {
            Some(*byte_offset)
        }
        _ => None,
    }
}

fn pick(e: &mut Expr, proven: Arms<'_>) {
    if let Expr::Conditional { condition, then_expr, else_expr } = e {
        if let Expr::NamespacePatched(ns) = condition.as_ref() {
            if let Some(offset) = pair_offset(ns, then_expr) {
                let arm = if proven.dead || (offset != 0 && proven.proven.contains(&offset)) {
                    std::mem::replace(else_expr.as_mut(), Expr::Undefined)
                } else {
                    std::mem::replace(then_expr.as_mut(), Expr::Undefined)
                };
                *e = arm;
                return pick(e, proven);
            }
        }
    }
    if let Expr::Closure { func_id, body, .. } = e {
        stmts(body, proven.enter(*func_id));
    }
    crate::walker::walk_expr_children_mut(e, &mut |c| pick(c, proven));
}

fn stmts(body: &mut [Stmt], proven: Arms<'_>) {
    for stmt in body {
        match stmt {
            Stmt::Let { init: Some(e), .. } | Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => {
                pick(e, proven)
            }
            Stmt::If { condition, then_branch, else_branch } => {
                pick(condition, proven);
                stmts(then_branch, proven);
                if let Some(b) = else_branch {
                    stmts(b, proven);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                pick(condition, proven);
                stmts(body, proven);
            }
            Stmt::For { init, condition, update, body } => {
                if let Some(init) = init {
                    stmts(std::slice::from_mut(init), proven);
                }
                if let Some(e) = condition {
                    pick(e, proven);
                }
                if let Some(e) = update {
                    pick(e, proven);
                }
                stmts(body, proven);
            }
            Stmt::Labeled { body, .. } => stmts(std::slice::from_mut(body), proven),
            Stmt::Try { body, catch, finally, .. } => {
                stmts(body, proven);
                if let Some(c) = catch {
                    stmts(&mut c.body, proven);
                }
                if let Some(b) = finally {
                    stmts(b, proven);
                }
            }
            Stmt::Switch { discriminant, cases } => {
                pick(discriminant, proven);
                for case in cases {
                    if let Some(e) = &mut case.test {
                        pick(e, proven);
                    }
                    stmts(&mut case.body, proven);
                }
            }
            _ => {}
        }
    }
}

fn function(f: &mut crate::ir::Function, proven: Arms<'_>) {
    let proven = proven.enter(f.id);
    for p in &mut f.params {
        if let Some(d) = &mut p.default {
            pick(d, proven);
        }
    }
    stmts(&mut f.body, proven);
}

/// Replace every namespace pair of `module` by its one arm: the intrinsic
/// where `proven` holds the generic call's member offset, else the generic
/// call. A pair codegen meets elsewhere is picked there by the same rule.
///
/// `inactive`: the function ids of the module's bodies the program never
/// runs (empty when reach is incomplete); every pair in one takes the
/// intrinsic.
pub fn pick_namespace_pair_arms(
    module: &mut Module,
    proven: &BTreeSet<u32>,
    inactive: &BTreeSet<u32>,
) {
    let proven = Arms { proven, inactive, dead: false };
    stmts(&mut module.init, proven);
    for global in &mut module.globals {
        if let Some(e) = &mut global.init {
            pick(e, proven);
        }
    }
    for f in &mut module.functions {
        function(f, proven);
    }
    for class in &mut module.classes {
        if let Some(e) = &mut class.extends_expr {
            pick(e, proven);
        }
        for field in class.fields.iter_mut().chain(&mut class.static_fields) {
            if let Some(e) = &mut field.key_expr {
                pick(e, proven);
            }
            if let Some(e) = &mut field.init {
                pick(e, proven);
            }
        }
        for f in class
            .constructor
            .iter_mut()
            .chain(&mut class.methods)
            .chain(&mut class.static_methods)
            .chain(class.getters.iter_mut().map(|(_, f)| f))
            .chain(class.setters.iter_mut().map(|(_, f)| f))
        {
            function(f, proven);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `NamespacePatched(Math) ? globalThis.Math.cos(1) : MathCos(1)`, the
    /// generic call's member at `offset`.
    fn pair(offset: u32) -> Expr {
        let member = Expr::PropertyGet {
            object: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::GlobalThisExpr),
                property: "Math".into(),
                byte_offset: 0,
                byte_end: 0,
            }),
            property: "cos".into(),
            byte_offset: offset,
            byte_end: 0,
        };
        Expr::Conditional {
            condition: Box::new(Expr::NamespacePatched("Math".into())),
            then_expr: Box::new(Expr::Call {
                callee: Box::new(member),
                args: vec![Expr::Number(1.0)],
                type_args: Vec::new(),
                byte_offset: offset,
                byte_end: 0,
            }),
            else_expr: Box::new(Expr::MathCos(Box::new(Expr::Number(1.0)))),
        }
    }

    /// Two functions, `run` and `never`, each returning one unproven pair
    /// (offsets 10 and 20); `never` also holds a closure with one (30).
    fn module() -> Module {
        let source = "function run(x: number) { return x; } function never(x: number) { return x; }";
        let ast = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
        let mut m = crate::lower_module(&ast, "t", "t.ts").expect("source lowers");
        for (f, offset) in m.functions.iter_mut().zip([10, 20]) {
            f.body = vec![Stmt::Return(Some(pair(offset)))];
        }
        let closure = m
            .functions
            .iter()
            .map(|f| f.id)
            .max()
            .expect("two functions")
            + 1;
        m.functions[1].body.insert(
            0,
            Stmt::Expr(Expr::Closure {
                func_id: closure,
                params: Vec::new(),
                return_type: crate::types::Type::Any,
                body: vec![Stmt::Return(Some(pair(30)))],
                captures: Vec::new(),
                mutable_captures: Vec::new(),
                captures_this: false,
                captures_new_target: false,
                enclosing_class: None,
                is_arrow: true,
                is_async: false,
                is_generator: false,
                is_strict: true,
            }),
        );
        m
    }

    fn arms(m: &Module) -> Vec<&'static str> {
        fn of(e: &Expr) -> &'static str {
            match e {
                Expr::MathCos(_) => "intrinsic",
                Expr::Call { .. } => "generic",
                _ => "pair",
            }
        }
        let mut out = Vec::new();
        for f in &m.functions {
            for st in &f.body {
                match st {
                    Stmt::Return(Some(e)) => out.push(of(e)),
                    Stmt::Expr(Expr::Closure { body, .. }) => {
                        if let [Stmt::Return(Some(e))] = &body[..] {
                            out.push(of(e));
                        }
                    }
                    _ => {}
                }
            }
        }
        out
    }

    /// MATHC3: an unproven pair is the generic call in a body the program
    /// runs, the intrinsic in one it never runs and in a closure nested in it.
    #[test]
    fn mathc3_pair_in_an_inactive_body_takes_the_intrinsic() {
        let mut m = module();
        let never = m.functions[1].id;
        pick_namespace_pair_arms(&mut m, &BTreeSet::new(), &BTreeSet::from([never]));
        // run: generic; never: closure intrinsic, return intrinsic.
        assert_eq!(arms(&m), ["generic", "intrinsic", "intrinsic"]);
    }

    /// MATHC3 KEEP: with no inactive body every unproven pair stays the
    /// generic call, and a proven one is the intrinsic wherever it is.
    #[test]
    fn mathc3_no_inactive_body_keeps_the_verdict_pick() {
        let mut m = module();
        pick_namespace_pair_arms(&mut m, &BTreeSet::from([10]), &BTreeSet::new());
        assert_eq!(arms(&m), ["intrinsic", "generic", "generic"]);
    }
}
