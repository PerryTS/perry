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
//!
//! MATHC5: in the same walk, a call through a value `f(args…)` whose callee
//! the facts prove to be exactly the original builtin `Math.<m>` (a native
//! declared `this = ignored` at its definition: its algorithm never reads
//! its `this` value) becomes `(f, <intrinsic of Math.<m>(args…)>)`. ECMA-262
//! 13.3.6.1: the call evaluates the callee, then the arguments in order, then
//! calls the value with `this` undefined; the sequence keeps the callee's
//! evaluation first and the intrinsic is the HIR the direct call
//! `Math.<m>(args…)` lowers to (ToNumber of each argument it reads, in
//! order). A call whose arguments the intrinsic does not read exactly (a
//! spread, fewer or more than its operands) keeps the generic call.

use std::collections::{BTreeMap, BTreeSet};

use crate::ir::{Expr, Module, Stmt};

/// What picks a pair's arm: the proven generic-call offsets of the module,
/// the function ids of its bodies the program never runs, and whether the
/// walk is inside such a body.
#[derive(Clone, Copy)]
struct Arms<'a> {
    proven: &'a BTreeSet<u32>,
    inactive: &'a BTreeSet<u32>,
    dead: bool,
    /// Counts the pairs that keep their generic arm.
    kept: &'a std::cell::Cell<usize>,
    /// MATHC5: the value calls proven to call `Math.<m>`: call byte offset
    /// to the member `m`.
    aliases: &'a BTreeMap<u32, String>,
}

impl Arms<'_> {
    /// The arms of the body with function id `id`: dead when it is inactive
    /// (a body nested in a dead one stays dead: only running its parent
    /// creates it).
    fn enter(self, id: u32) -> Self {
        Arms {
            dead: self.dead || self.inactive.contains(&id),
            ..self
        }
    }
}

/// The callee of the pair's generic `globalThis.<ns>.<member>` call.
pub fn pair_callee<'a>(ns: &str, generic: &'a Expr) -> Option<&'a Expr> {
    let (Expr::Call { callee, .. } | Expr::CallSpread { callee, .. }) = generic else {
        return None;
    };
    let Expr::PropertyGet { object, .. } = callee.as_ref() else {
        return None;
    };
    match object.as_ref() {
        Expr::PropertyGet {
            object: g,
            property,
            ..
        } if matches!(g.as_ref(), Expr::GlobalThisExpr) && property == ns => Some(callee),
        _ => None,
    }
}

/// The generic call's named member offset, shared by the picker and facts.
pub fn pair_offset(ns: &str, generic: &Expr) -> Option<u32> {
    match pair_callee(ns, generic)? {
        Expr::PropertyGet { byte_offset, .. } => Some(*byte_offset),
        _ => unreachable!("pair_callee only admits a named member"),
    }
}

/// The live pair takes its intrinsic exactly when its named site is proven.
pub fn intrinsic_selected(offset: u32, proven: &BTreeSet<u32>) -> bool {
    offset != 0 && proven.contains(&offset)
}

/// MATHC5: `callee(args…)` as `(callee, <intrinsic of Math.<member>(args…)>)`
/// when the intrinsic reads exactly the call's arguments (one operand per
/// argument), else `None` and the call is unchanged.
fn alias_call(member: &str, callee: &Expr, args: &[Expr]) -> Option<Expr> {
    let intrinsic = crate::lower::math_intrinsic(member, args.to_vec(), false).ok()?;
    let mut operands = 0usize;
    crate::walker::walk_expr_children(&intrinsic, &mut |_| operands += 1);
    (operands == args.len()).then(|| Expr::Sequence(vec![callee.clone(), intrinsic]))
}

fn pick(e: &mut Expr, proven: Arms<'_>) {
    if let Expr::Call {
        callee,
        args,
        byte_offset,
        ..
    } = e
    {
        if *byte_offset != 0 {
            if let Some(member) = proven.aliases.get(byte_offset) {
                if let Some(rewritten) = alias_call(member, callee, args) {
                    *e = rewritten;
                }
            }
        }
    }
    if let Expr::Conditional {
        condition,
        then_expr,
        else_expr,
    } = e
    {
        if let Expr::NamespacePatched(ns) = condition.as_ref() {
            if let Some(offset) = pair_offset(ns, then_expr) {
                let arm = if proven.dead || intrinsic_selected(offset, proven.proven) {
                    std::mem::replace(else_expr.as_mut(), Expr::Undefined)
                } else {
                    proven.kept.set(proven.kept.get() + 1);
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
            Stmt::Let { init: Some(e), .. }
            | Stmt::Expr(e)
            | Stmt::Throw(e)
            | Stmt::Return(Some(e), _) => pick(e, proven),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
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
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
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
            Stmt::Try {
                body,
                catch,
                finally,
                ..
            } => {
                stmts(body, proven);
                if let Some(c) = catch {
                    stmts(&mut c.body, proven);
                }
                if let Some(b) = finally {
                    stmts(b, proven);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
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
///
/// Returns the number of pairs that keep the generic call: the only places
/// where the picked module holds code a build without the pairs does not
/// (MATHC4: the optional-feature scan runs again for such a module).
///
/// `aliases`: the value calls of the module the facts prove to call the
/// original `Math.<m>` (call byte offset to `m`); each becomes the intrinsic
/// (MATHC5, module comment).
pub fn pick_namespace_pair_arms(
    module: &mut Module,
    proven: &BTreeSet<u32>,
    inactive: &BTreeSet<u32>,
    aliases: &BTreeMap<u32, String>,
) -> usize {
    let kept = std::cell::Cell::new(0);
    module_arms(
        module,
        Arms {
            proven,
            inactive,
            dead: false,
            kept: &kept,
            aliases,
        },
    );
    kept.get()
}

/// Replace every namespace pair of `module` by its intrinsic arm: the HIR the
/// module has without the pairs. The optional-feature scan reads this view
/// before the facts run (MATHC4), so a pair's generic arm selects no runtime
/// feature unless the pick keeps it.
pub fn take_intrinsic_arms(module: &mut Module) {
    let none = BTreeSet::new();
    let kept = std::cell::Cell::new(0);
    let aliases = BTreeMap::new();
    module_arms(
        module,
        Arms {
            proven: &none,
            inactive: &none,
            dead: true,
            kept: &kept,
            aliases: &aliases,
        },
    );
}

fn module_arms(module: &mut Module, proven: Arms<'_>) {
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
        let source =
            "function run(x: number) { return x; } function never(x: number) { return x; }";
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
                construct_only: false,
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
        pick_namespace_pair_arms(
            &mut m,
            &BTreeSet::new(),
            &BTreeSet::from([never]),
            &BTreeMap::new(),
        );
        // run: generic; never: closure intrinsic, return intrinsic.
        assert_eq!(arms(&m), ["generic", "intrinsic", "intrinsic"]);
    }

    /// MATHC3 KEEP: with no inactive body every unproven pair stays the
    /// generic call, and a proven one is the intrinsic wherever it is.
    #[test]
    fn mathc3_no_inactive_body_keeps_the_verdict_pick() {
        let mut m = module();
        pick_namespace_pair_arms(
            &mut m,
            &BTreeSet::from([10]),
            &BTreeSet::new(),
            &BTreeMap::new(),
        );
        assert_eq!(arms(&m), ["intrinsic", "generic", "generic"]);
    }

    /// MATHC5: a proven value call `c(1)` (offset 40) becomes `(c, MathCos(1))`;
    /// `c()` and `c(1, 2)` (offsets 41, 42), whose arguments the intrinsic
    /// does not read exactly, and an unproven one (43) keep the call.
    #[test]
    fn mathc5_proven_alias_call_takes_the_intrinsic_when_it_reads_every_argument() {
        let call = |offset: u32, args: Vec<Expr>| Expr::Call {
            callee: Box::new(Expr::Undefined),
            args,
            type_args: Vec::new(),
            byte_offset: offset,
            byte_end: 0,
        };
        let mut m = module();
        m.functions[0].body = vec![
            Stmt::Expr(call(40, vec![Expr::Number(1.0)])),
            Stmt::Expr(call(41, vec![])),
            Stmt::Expr(call(42, vec![Expr::Number(1.0), Expr::Number(2.0)])),
            Stmt::Expr(call(43, vec![Expr::Number(1.0)])),
        ];
        let aliases: BTreeMap<u32, String> = [40, 41, 42].map(|o| (o, "cos".to_string())).into();
        pick_namespace_pair_arms(&mut m, &BTreeSet::new(), &BTreeSet::new(), &aliases);
        let shapes: Vec<&str> = m.functions[0]
            .body
            .iter()
            .map(|s| match s {
                Stmt::Expr(Expr::Sequence(v))
                    if matches!(v.as_slice(), [Expr::Undefined, Expr::MathCos(_)]) =>
                {
                    "intrinsic"
                }
                Stmt::Expr(Expr::Call { .. }) => "call",
                _ => "other",
            })
            .collect();
        assert_eq!(shapes, ["intrinsic", "call", "call", "call"]);
    }
}
