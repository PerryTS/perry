//! Fold reads of module-level `const` literals into the literal — run by the
//! driver on every module AFTER the whole transform phase, immediately before
//! code generation.
//!
//! `export const COMPONENT_ID_MAX = 1023;` lowers to a `Stmt::Let { mutable:
//! false, init: Some(Integer(1023)) }` in `module.init`, and every function in
//! the module reads it as a plain `LocalGet` of that module-scope id. Such a
//! read is opaque to the typed-ABI clone rules: `isComponentId(id)`, whose
//! body is `id >= 1 && id <= COMPONENT_ID_MAX`, was refused its `i1` clone
//! (`ReturnExprNotTypedI1Safe`), so every call ran the boxed body — a module
//! global load and the full dynamic tag-coercion compare on both operands —
//! instead of a guard and two `fcmp`s. With the literal in place of the read,
//! the body is straight-line typed and the clone is admitted.
//!
//! Why this is NOT a pipeline pass: the cross-module inliner admits only
//! bodies whose locals are their own, and the folded predicates qualify. Run
//! inside the pipeline, the fold made them harvestable, they consumed the
//! caller's inline budget, and a hot `world.set` lost `resolveSetOperation`
//! (a 43% regression); with a larger budget the inlined bodies still did the
//! dynamic compare on the untyped call-site value, a wash. Run after every
//! module has been transformed (harvests already taken from the unfolded
//! bodies), the fold changes no inlining decision and only what codegen sees.
//!
//! Admission: a top-level `module.init` `let` that is immutable, whose
//! initializer is a number, integer, string or boolean literal, and that no
//! `LocalSet` / `Update` in the module writes (a `const` cannot be, but the
//! scan is cheap and keeps the pass honest against synthesized bindings).
//! Reads are folded in every function, method, accessor and constructor
//! body, in closure bodies (the id is then dropped from the closure's capture
//! lists — the value it would have captured is the literal), and in
//! `module.init` statements AFTER the declaration. Reads before the
//! declaration are left alone: they are in the temporal dead zone and must
//! keep throwing.
use std::collections::{HashMap, HashSet};

use perry_hir::types::LocalId;
use perry_hir::walker::walk_expr_children_mut;
use perry_hir::{Expr, Function, Module, Stmt};

use crate::closure_local_inline::{for_each_expr_in_stmt_mut, nested_stmt_lists};

pub fn run(module: &mut Module) {
    fold_module_consts(module);
    // Phase 2 runs unconditionally — see `rewrite_literal_index_gets`.
    rewrite_literal_index_gets(module);
}

fn fold_module_consts(module: &mut Module) {
    let mut consts: HashMap<LocalId, Expr> = HashMap::new();
    let mut decl_index: HashMap<LocalId, usize> = HashMap::new();
    for (index, stmt) in module.init.iter().enumerate() {
        if let Stmt::Let {
            id,
            mutable: false,
            init: Some(init),
            ..
        } = stmt
        {
            if is_foldable_literal(init) {
                consts.insert(*id, init.clone());
                decl_index.insert(*id, index);
            }
        }
    }
    if consts.is_empty() {
        return;
    }
    // Anything written anywhere in the module is not a constant.
    let mut written: HashSet<LocalId> = HashSet::new();
    for_each_function(module, &mut |f| {
        collect_written_in_stmts(&f.body, &mut written)
    });
    collect_written_in_stmts(&module.init, &mut written);
    for id in written {
        consts.remove(&id);
        decl_index.remove(&id);
    }
    if consts.is_empty() {
        return;
    }
    for_each_function(module, &mut |f| fold_stmts(&mut f.body, &consts));
    // `module.init`: only statements after each declaration.
    for (index, stmt) in module.init.iter_mut().enumerate() {
        let visible: HashMap<LocalId, Expr> = consts
            .iter()
            .filter(|(id, _)| decl_index.get(*id).is_some_and(|d| *d < index))
            .map(|(id, lit)| (*id, lit.clone()))
            .collect();
        if visible.is_empty() {
            continue;
        }
        fold_stmt(stmt, &visible);
    }
}

fn is_foldable_literal(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Integer(_) | Expr::Number(_) | Expr::String(_) | Expr::Bool(_)
    )
}

fn for_each_function(module: &mut Module, f: &mut dyn FnMut(&mut Function)) {
    for function in &mut module.functions {
        f(function);
    }
    for class in &mut module.classes {
        if let Some(ctor) = &mut class.constructor {
            f(ctor);
        }
        for m in class
            .methods
            .iter_mut()
            .chain(class.static_methods.iter_mut())
        {
            f(m);
        }
        for (_, g) in &mut class.getters {
            f(g);
        }
        for (_, s) in &mut class.setters {
            f(s);
        }
        for cm in &mut class.computed_members {
            f(&mut cm.function);
        }
    }
}

fn collect_written_in_stmts(stmts: &[Stmt], written: &mut HashSet<LocalId>) {
    fn visit(expr: &Expr, written: &mut HashSet<LocalId>) {
        match expr {
            Expr::LocalSet(id, _) | Expr::Update { id, .. } => {
                written.insert(*id);
            }
            _ => {}
        }
        perry_hir::walker::walk_expr_children(expr, &mut |child| visit(child, written));
    }
    for stmt in stmts {
        walk_stmt_exprs(stmt, &mut |expr| visit(expr, written));
    }
}

fn walk_stmt_exprs(stmt: &Stmt, f: &mut dyn FnMut(&Expr)) {
    match stmt {
        Stmt::Let { init: Some(e), .. }
        | Stmt::Expr(e)
        | Stmt::Throw(e)
        | Stmt::Return(Some(e)) => f(e),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            f(condition);
            for s in then_branch {
                walk_stmt_exprs(s, f);
            }
            if let Some(e) = else_branch {
                for s in e {
                    walk_stmt_exprs(s, f);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            f(condition);
            for s in body {
                walk_stmt_exprs(s, f);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(s) = init {
                walk_stmt_exprs(s, f);
            }
            if let Some(e) = condition {
                f(e);
            }
            if let Some(e) = update {
                f(e);
            }
            for s in body {
                walk_stmt_exprs(s, f);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            f(discriminant);
            for case in cases {
                if let Some(t) = &case.test {
                    f(t);
                }
                for s in &case.body {
                    walk_stmt_exprs(s, f);
                }
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                walk_stmt_exprs(s, f);
            }
            if let Some(c) = catch {
                for s in &c.body {
                    walk_stmt_exprs(s, f);
                }
            }
            if let Some(fin) = finally {
                for s in fin {
                    walk_stmt_exprs(s, f);
                }
            }
        }
        Stmt::Labeled { body, .. } => walk_stmt_exprs(body, f),
        _ => {}
    }
}

fn fold_stmts(stmts: &mut Vec<Stmt>, consts: &HashMap<LocalId, Expr>) {
    for stmt in stmts.iter_mut() {
        fold_stmt(stmt, consts);
    }
}

fn fold_stmt(stmt: &mut Stmt, consts: &HashMap<LocalId, Expr>) {
    for inner in nested_stmt_lists(stmt) {
        fold_stmts(inner, consts);
    }
    for_each_expr_in_stmt_mut(stmt, &mut |e| fold_expr(e, consts));
}

fn fold_expr(expr: &mut Expr, consts: &HashMap<LocalId, Expr>) {
    if let Expr::LocalGet(id) = expr {
        if let Some(lit) = consts.get(id) {
            *expr = lit.clone();
            return;
        }
    }
    if let Expr::Closure {
        body,
        captures,
        mutable_captures,
        ..
    } = expr
    {
        captures.retain(|id| !consts.contains_key(id));
        mutable_captures.retain(|id| !consts.contains_key(id));
        fold_stmts(body, consts);
    }
    walk_expr_children_mut(expr, &mut |child| fold_expr(child, consts));
}

/// Phase 2 (#10761) — rewrite `o[<string literal>]` into `o.<name>`.
///
/// This is the SAME rewrite the AST→HIR member lowering already applies to a
/// literal key written in source (`lower/expr_member/member_tail.rs`, the
/// issue #529 fold), re-applied here because phase 1 above — and the inliner
/// before it — *create* `IndexGet { _, String(_) }` nodes AFTER that matcher
/// has run, and nothing re-ran it.
///
/// The gap is worth 7.3x. A hoisted `const K = "a"` is folded to its literal
/// by phase 1, but the enclosing node stays an `IndexGet`, and codegen's
/// `IndexGet` arm for a static string key
/// (`expr/index_get.rs`, the `Expr::String(literal)` branch) calls
/// `js_typed_feedback_object_get_field_by_name_f64` — a full by-name runtime
/// resolution per read: UTF-8-validate the key, hash it for the accessor
/// Bloom summary, classify the receiver, then scan the shape's key array.
/// Measured on `O[K] + O[J]` over `{a:1,b:2,c:3}`: **618 instructions per
/// read**, against **24** for the identical read spelled `O.a`, which reaches
/// the per-site monomorphic inline cache in
/// `expr/property_get/generic_dispatch.rs`. `O["a"]` written in source is
/// already 24 — only the spelling that goes through a binding was stranded.
///
/// Numeric-index strings are excluded, exactly as the source-level fold
/// excludes them: `arr["0"]` keeps `IndexGet` semantics (string-coerced
/// element access on an array), and that is the disambiguator the spec itself
/// uses between indexed and named properties.
///
/// Everything else about the read is unchanged, because the produced node is
/// bit-identical to the one `o["name"]` produces in source: the same
/// `PropertyGet`, the same receiver expression, the same key string. There is
/// no new fast path here and no new guard — the rewrite moves a read onto a
/// lowering the whole test suite already exercises.
fn rewrite_literal_index_gets(module: &mut Module) {
    for_each_function(module, &mut |f| rewrite_stmts(&mut f.body));
    rewrite_stmts(&mut module.init);
}

/// A key that JavaScript resolves as an array index rather than a name.
///
/// Mirrors `member_tail.rs`'s test verbatim so the two folds admit exactly the
/// same key set; if they ever diverge, `o["0"]` and a `const Z = "0"` spelling
/// of it would compile to different lowerings.
fn is_numeric_index_string(key: &str) -> bool {
    !key.is_empty()
        && key.chars().all(|c| c.is_ascii_digit())
        && !(key.len() > 1 && key.starts_with('0'))
}

fn rewrite_stmts(stmts: &mut [Stmt]) {
    for stmt in stmts.iter_mut() {
        rewrite_stmt(stmt);
    }
}

fn rewrite_stmt(stmt: &mut Stmt) {
    for inner in nested_stmt_lists(stmt) {
        rewrite_stmts(inner);
    }
    for_each_expr_in_stmt_mut(stmt, &mut rewrite_expr);
}

fn rewrite_expr(expr: &mut Expr) {
    if let Expr::IndexGet { object, index } = expr {
        let property = match index.as_ref() {
            Expr::String(key) if !is_numeric_index_string(key) => Some(key.clone()),
            _ => None,
        };
        if let Some(property) = property {
            // The index is a literal, so there is no key expression to keep
            // alive and no evaluation-order obligation: `o[k]` evaluates `o`
            // then `k`, and a literal `k` is already a value.
            let object = std::mem::replace(object.as_mut(), Expr::Integer(0));
            *expr = Expr::PropertyGet {
                // Synthesized: the literal was not written at a source span
                // (phase 1 substituted it), so there is no member offset to
                // carry. `0` is the established "no debug location" value on
                // this node, and the `IndexGet` this replaces carried none
                // either.
                byte_offset: 0,
                object: Box::new(object),
                property,
            };
        }
    }
    // `walk_expr_children_mut` does not descend into a closure's STATEMENT
    // body; phase 1 has the same explicit arm for the same reason.
    if let Expr::Closure { body, .. } = expr {
        rewrite_stmts(body);
    }
    walk_expr_children_mut(expr, &mut rewrite_expr);
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::types::Type;
    use perry_hir::{CompareOp, Param};

    fn func(id: u32, body: Vec<Stmt>) -> Function {
        Function {
            id,
            name: format!("f{id}"),
            type_params: Vec::new(),
            params: vec![Param {
                id: 8,
                name: "x".to_string(),
                ty: Type::Number,
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            }],
            return_type: Type::Boolean,
            body,
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: true,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        }
    }

    fn le_const(const_id: u32) -> Stmt {
        Stmt::Return(Some(Expr::Compare {
            op: CompareOp::Le,
            left: Box::new(Expr::LocalGet(8)),
            right: Box::new(Expr::LocalGet(const_id)),
        }))
    }

    fn module_with_const(mutable: bool, init: Expr) -> Module {
        let mut m = Module::new("types.ts");
        m.init.push(Stmt::Let {
            id: 3,
            name: "COMPONENT_ID_MAX".to_string(),
            ty: Type::Number,
            mutable,
            init: Some(init),
        });
        m.functions.push(func(1, vec![le_const(3)]));
        m
    }

    #[test]
    fn an_immutable_literal_module_binding_folds_into_its_readers() {
        let mut m = module_with_const(false, Expr::Integer(1023));
        run(&mut m);
        assert!(
            matches!(
                &m.functions[0].body[0],
                Stmt::Return(Some(Expr::Compare { right, .. }))
                    if matches!(right.as_ref(), Expr::Integer(1023))
            ),
            "{:?}",
            m.functions[0].body[0]
        );
    }

    #[test]
    fn a_mutable_binding_or_a_non_literal_initializer_is_left_alone() {
        let mut m = module_with_const(true, Expr::Integer(1023));
        run(&mut m);
        assert!(matches!(
            &m.functions[0].body[0],
            Stmt::Return(Some(Expr::Compare { right, .. })) if matches!(right.as_ref(), Expr::LocalGet(3))
        ));
        let mut m = module_with_const(
            false,
            Expr::Binary {
                op: perry_hir::BinaryOp::Mul,
                left: Box::new(Expr::Integer(2)),
                right: Box::new(Expr::Integer(3)),
            },
        );
        run(&mut m);
        assert!(matches!(
            &m.functions[0].body[0],
            Stmt::Return(Some(Expr::Compare { right, .. })) if matches!(right.as_ref(), Expr::LocalGet(3))
        ));
    }

    #[test]
    fn a_read_before_the_declaration_in_init_keeps_its_tdz_and_a_later_one_folds() {
        let mut m = module_with_const(false, Expr::Integer(7));
        m.init.insert(0, Stmt::Expr(Expr::LocalGet(3)));
        m.init.push(Stmt::Expr(Expr::LocalGet(3)));
        run(&mut m);
        assert!(matches!(&m.init[0], Stmt::Expr(Expr::LocalGet(3))));
        assert!(matches!(&m.init[2], Stmt::Expr(Expr::Integer(7))));
    }

    #[test]
    fn a_closure_reading_the_constant_drops_it_from_its_captures() {
        let mut m = module_with_const(false, Expr::Integer(5));
        m.functions[0].body = vec![Stmt::Return(Some(Expr::Closure {
            func_id: 77,
            params: Vec::new(),
            return_type: Type::Boolean,
            body: vec![le_const(3)],
            captures: vec![3],
            mutable_captures: Vec::new(),
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: true,
            is_async: false,
            is_generator: false,
            is_strict: true,
        }))];
        run(&mut m);
        let Stmt::Return(Some(Expr::Closure { body, captures, .. })) = &m.functions[0].body[0]
        else {
            panic!("closure expected");
        };
        assert!(captures.is_empty(), "{captures:?}");
        assert!(matches!(
            &body[0],
            Stmt::Return(Some(Expr::Compare { right, .. })) if matches!(right.as_ref(), Expr::Integer(5))
        ));
    }

    // ---- phase 2 (#10761): the literal-index rewrite -------------------

    /// The module-level fold substitutes the literal, and phase 2 then moves
    /// the read onto the SAME node `o["a"]` produces in source. Without
    /// phase 2 this stays an `IndexGet` and codegen resolves it by name at
    /// runtime — 618 instructions per read against 24.
    #[test]
    fn a_const_string_key_read_becomes_a_property_get() {
        let mut m = Module::new("k.ts");
        m.init.push(Stmt::Let {
            id: 3,
            name: "K".to_string(),
            ty: Type::String,
            mutable: false,
            init: Some(Expr::String("a".to_string())),
        });
        m.functions.push(func(
            1,
            vec![Stmt::Return(Some(Expr::IndexGet {
                object: Box::new(Expr::LocalGet(8)),
                index: Box::new(Expr::LocalGet(3)),
            }))],
        ));
        run(&mut m);
        let Stmt::Return(Some(Expr::PropertyGet {
            object, property, ..
        })) = &m.functions[0].body[0]
        else {
            panic!("expected a PropertyGet, got {:?}", m.functions[0].body[0]);
        };
        assert_eq!(property, "a");
        assert!(matches!(object.as_ref(), Expr::LocalGet(8)));
    }

    /// GUARD WITNESS for `is_numeric_index_string`. An array index key must
    /// keep `IndexGet` semantics; `arr["0"]` is a string-coerced ELEMENT read,
    /// not a named one, and that is the disambiguator the spec itself uses.
    /// Delete the guard in `rewrite_expr` and this assertion fails.
    ///
    /// Note honestly what this test is and is not: at RUNTIME both spellings
    /// happen to resolve a numeric name on an Array, a TypedArray and a String
    /// through the same ladder, so the removal is behaviour-neutral on every
    /// receiver I could construct. What removing it does cost is measured —
    /// `Int32Array[K]` with `const K = "1"` goes from 969 to 1343 instructions
    /// per read (+38.5%), because the folded form leaves the element lane.
    #[test]
    fn an_array_index_key_is_not_folded() {
        for key in ["0", "1", "42", "4294967294"] {
            let mut m = Module::new("k.ts");
            m.functions.push(func(
                1,
                vec![Stmt::Return(Some(Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(8)),
                    index: Box::new(Expr::String(key.to_string())),
                }))],
            ));
            run(&mut m);
            assert!(
                matches!(
                    &m.functions[0].body[0],
                    Stmt::Return(Some(Expr::IndexGet { .. }))
                ),
                "key {key:?} must stay an IndexGet, got {:?}",
                m.functions[0].body[0]
            );
        }
    }

    /// The keys the guard does NOT claim: a leading zero, a fraction, a sign
    /// and the empty string are property NAMES, not indices, and must fold —
    /// exactly as `member_tail.rs` folds them when written in source.
    #[test]
    fn a_non_index_numeric_looking_key_is_folded() {
        for key in ["07", "1.5", "-1", "", "1e3", "NaN"] {
            let mut m = Module::new("k.ts");
            m.functions.push(func(
                1,
                vec![Stmt::Return(Some(Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(8)),
                    index: Box::new(Expr::String(key.to_string())),
                }))],
            ));
            run(&mut m);
            assert!(
                matches!(
                    &m.functions[0].body[0],
                    Stmt::Return(Some(Expr::PropertyGet { property, .. })) if property == key
                ),
                "key {key:?} must fold, got {:?}",
                m.functions[0].body[0]
            );
        }
    }

    /// Phase 2 runs even when phase 1 folded nothing: a literal index can be
    /// put there by the inliner, and `run` early-returns out of phase 1 when
    /// the module declares no foldable const.
    #[test]
    fn the_rewrite_runs_with_no_module_consts_at_all() {
        let mut m = Module::new("k.ts");
        m.init.push(Stmt::Expr(Expr::IndexGet {
            object: Box::new(Expr::LocalGet(8)),
            index: Box::new(Expr::String("name".to_string())),
        }));
        run(&mut m);
        assert!(matches!(
            &m.init[0],
            Stmt::Expr(Expr::PropertyGet { property, .. }) if property == "name"
        ));
    }

    /// The receiver subtree is moved, not dropped: a nested read rewrites at
    /// both levels and keeps its inner object.
    #[test]
    fn a_nested_literal_index_rewrites_at_every_level() {
        let mut m = Module::new("k.ts");
        m.init.push(Stmt::Expr(Expr::IndexGet {
            object: Box::new(Expr::IndexGet {
                object: Box::new(Expr::LocalGet(8)),
                index: Box::new(Expr::String("outer".to_string())),
            }),
            index: Box::new(Expr::String("inner".to_string())),
        }));
        run(&mut m);
        let Stmt::Expr(Expr::PropertyGet {
            object, property, ..
        }) = &m.init[0]
        else {
            panic!("expected outer PropertyGet, got {:?}", m.init[0]);
        };
        assert_eq!(property, "inner");
        assert!(matches!(
            object.as_ref(),
            Expr::PropertyGet { property, .. } if property == "outer"
        ));
    }
}
