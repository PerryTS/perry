//! Redundant property-read elimination over **diverging guard chains**.
//!
//! # The shape
//!
//! The discriminated-union dispatch idiom is the single most common shape in
//! idiomatic TypeScript, and it reads the discriminant once per arm:
//!
//! ```ignore
//! function evalNode(n: Node, env: Env): Value {
//!   if (n.kind === "num") return { kind: "num", num: n.num };
//!   if (n.kind === "str") return { kind: "str", str: n.str };
//!   if (n.kind === "var") return lookup(env, n.name);
//!   …
//! }
//! ```
//!
//! Every one of those `n.kind` reads compiles to a **full generic
//! property-get diamond** — `pget.recv_sso` / `pget.check_class_ref` /
//! `pget.recv_ok` / the monomorphic-IC guard ladder / `pic.hit` / `pic.miss` /
//! `pget.recv_merge` — about fifty instructions and six branches per site
//! (`expr/property_get/generic_dispatch.rs`). `gc-handoff/apps/interp.ts`'s
//! `evalNode` has **53 such sites**, of which `n.kind` accounts for seven and
//! `n.op` for seven more.
//!
//! LLVM `-O3` cannot remove them. GVN dedupes *instructions*, not congruent
//! control-flow regions, and every arm of the diamond that is not the inline
//! fast load ends in an opaque call (`js_object_get_field_ic_miss`,
//! `js_object_get_field_by_name_f64`) whose memory effects license nothing. So
//! the redundancy has to be removed before it becomes a diamond — here.
//!
//! # The rewrite
//!
//! Inside a run of consecutive `if (COND) { … }` statements that have no
//! `else` and whose bodies **always diverge** (`return` / `throw` / `break` /
//! `continue`), the fall-through path from one guard to the next runs *no user
//! code at all* — only the guard expressions themselves. So a
//! `local.property` read appearing in two or more guards of the run is
//! redundant, and one hoisted `const` in front of the run replaces all of
//! them.
//!
//! Divergence is what makes the run analyzable: a body that falls through
//! could call anything, and then the next guard's read is a genuinely fresh
//! observation.
//!
//! # Why this is sound, and the one thing that gates it
//!
//! Everything the guards may contain is on a closed allowlist
//! ([`guard_expr_is_analyzable`]): literals, `LocalGet`, `local.prop` reads,
//! **strict** `===`/`!==` compares, `&&`/`||`/`??`, `!`, and `typeof`. Loose
//! compares and relational operators are excluded because they can invoke
//! `valueOf`/`toString`; `Binary` is excluded wholesale for the same reason;
//! calls, `new`, assignments, `await` and index reads are excluded outright.
//! A run containing an assignment to any local is skipped, so the receiver
//! cannot change between guards.
//!
//! That leaves exactly one way for user code to run between two reads of
//! `n.kind`: **`kind` could be an accessor**, and a stateful getter that
//! returns a different value on its second invocation would observe the
//! difference. The guard is therefore a whole-module veto
//! ([`module_defines_accessors`]) — if the module defines a class getter or
//! setter, defines an object-literal accessor, calls
//! `Object.defineProperty`/`defineProperties`/`__defineGetter__`/
//! `__defineSetter__`, or constructs a `Proxy`, the pass does nothing at all.
//! This mirrors `perry-codegen`'s `Ptr<Shape>` promotion, which is vetoed
//! module-wide by the same family of constructs (`collectors/ptr_shape.rs`)
//! before it emits an *unguarded* field load — a strictly stronger assumption
//! than this one.
//!
//! `PERRY_PROP_CSE=0` disables the pass for bisection.

use std::collections::HashMap;

use perry_hir::types::{LocalId, Type};
use perry_hir::{CompareOp, Expr, Function, Module, Stmt, UnaryOp};

/// A hoistable read: `LocalGet(id).property`.
type ReadKey = (LocalId, String);

/// `PERRY_PROP_CSE=0` / `off` / `false` disables the pass.
fn enabled() -> bool {
    match std::env::var("PERRY_PROP_CSE") {
        Ok(v) => !matches!(v.as_str(), "0" | "off" | "false"),
        Err(_) => true,
    }
}

/// Entry point. Runs after the inliner and the loop unroller, so inlined
/// callee bodies get the same treatment.
pub fn run(module: &mut Module) {
    if !enabled() || module_defines_accessors(module) {
        return;
    }
    let mut next_local_id = crate::generator::compute_max_local_id(module).saturating_add(1);

    // `module.init` is deliberately skipped: a module-level `Stmt::Let` is not
    // an ordinary local (module variables live in `@perry_global_<mod>__<id>`
    // and are registered as GC roots before init), so minting one here would
    // need registration this pass does not do. It is also cold code.
    for f in &mut module.functions {
        run_function(f, &mut next_local_id);
    }
    for c in &mut module.classes {
        if let Some(ctor) = &mut c.constructor {
            run_function(ctor, &mut next_local_id);
        }
        for m in &mut c.methods {
            run_function(m, &mut next_local_id);
        }
        // Accessor bodies cannot exist here — `module_defines_accessors`
        // already returned early if the module had any.
    }
}

fn run_function(f: &mut Function, next_local_id: &mut LocalId) {
    // The async/generator transforms run AFTER this pass and box every body
    // local into a shared mutable `Any` cell. A hoisted `const` would become
    // one more boxed cell rather than a register, which is a pessimization,
    // and the boxing path is a known-weak area (per-iteration bindings
    // collapse there). Leave those bodies alone.
    if f.is_async || f.is_generator {
        return;
    }
    cse_stmts(&mut f.body, next_local_id);
}

/// Walk a statement list, rewriting maximal runs of diverging guards.
/// Recurses into every nested statement list first so inner runs are handled
/// in their own right.
fn cse_stmts(stmts: &mut Vec<Stmt>, next_local_id: &mut LocalId) {
    for s in stmts.iter_mut() {
        for inner in nested_stmt_lists(s) {
            cse_stmts(inner, next_local_id);
        }
    }

    let mut i = 0usize;
    while i < stmts.len() {
        let mut j = i;
        while j < stmts.len() && diverging_guard(&stmts[j]).is_some() {
            j += 1;
        }
        if j - i >= 2 {
            // `rewrite_run` reports each hoist as (offset-within-run, Let).
            // Inserting from the back keeps the earlier offsets valid.
            let mut hoists = rewrite_run(&mut stmts[i..j], next_local_id);
            let n = hoists.len();
            hoists.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
            for (at, stmt) in hoists {
                stmts.insert(i + at, stmt);
            }
            i = j + n;
        } else {
            i = if j > i { j } else { i + 1 };
        }
    }
}

/// The guard condition of an `if (COND) { … }` whose body always diverges and
/// which has no `else`, provided the condition is on the analyzable allowlist.
fn diverging_guard(stmt: &Stmt) -> Option<&Expr> {
    match stmt {
        Stmt::If {
            condition,
            then_branch,
            else_branch: None,
        } if always_diverges(then_branch) && guard_expr_is_analyzable(condition) => Some(condition),
        _ => None,
    }
}

/// Every path through `stmts` leaves the enclosing statement list.
fn always_diverges(stmts: &[Stmt]) -> bool {
    match stmts.last() {
        Some(Stmt::Return(_))
        | Some(Stmt::Throw(_))
        | Some(Stmt::Break)
        | Some(Stmt::Continue)
        | Some(Stmt::LabeledBreak(_))
        | Some(Stmt::LabeledContinue(_)) => true,
        Some(Stmt::If {
            then_branch,
            else_branch: Some(else_branch),
            ..
        }) => always_diverges(then_branch) && always_diverges(else_branch),
        _ => false,
    }
}

/// Closed allowlist of guard expressions: nothing here can run user code
/// except a property read, which is precisely what the module-wide accessor
/// veto rules out.
fn guard_expr_is_analyzable(e: &Expr) -> bool {
    match e {
        Expr::Undefined
        | Expr::Null
        | Expr::Bool(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::BigInt(_)
        | Expr::String(_)
        | Expr::WtfString(_)
        | Expr::This
        | Expr::LocalGet(_)
        | Expr::FuncRef(_)
        | Expr::ClassRef(_) => true,
        // Only a direct local receiver. A nested `a.b.c` would need the inner
        // read hoisted too, and the outer key would then be keyed on a
        // temporary — not worth the extra bookkeeping for the idiom this
        // targets.
        Expr::PropertyGet { object, .. } => matches!(object.as_ref(), Expr::LocalGet(_)),
        // STRICT compares only. `==`/`!=`/`<`/`<=`/`>`/`>=` all coerce, and
        // coercion calls `valueOf`/`toString`.
        Expr::Compare { op, left, right } => {
            matches!(op, CompareOp::Eq | CompareOp::Ne)
                && guard_expr_is_analyzable(left)
                && guard_expr_is_analyzable(right)
        }
        Expr::Logical { left, right, .. } => {
            guard_expr_is_analyzable(left) && guard_expr_is_analyzable(right)
        }
        Expr::Unary { op, operand } => {
            matches!(op, UnaryOp::Not) && guard_expr_is_analyzable(operand)
        }
        Expr::TypeOf(operand) => guard_expr_is_analyzable(operand),
        _ => false,
    }
}

/// Hoist repeated reads. Returns `(offset within the run, the `Stmt::Let`)`
/// pairs; the caller inserts them.
///
/// # Where a hoist may go, and why it is not simply "the top of the run"
///
/// The hoisted read is evaluated where the `Let` lands, so it must land
/// somewhere the original program already evaluated it **unconditionally** —
/// otherwise `if (a.x === 1) return; if (b.y === 2) return;` would start
/// dereferencing `b` on the path where `a.x === 1` returns, and a null `b`
/// would throw where it previously did not.
///
/// So a key is a candidate only at the guard where it is that guard's **first
/// unconditionally-evaluated read** — first, so hoisting it in front of the
/// guard cannot reorder it past a sibling read that could throw first; and
/// unconditional, so it is not under the right-hand side of `&&`/`||`/`??`,
/// which the original may skip. The `Let` goes immediately before that guard,
/// and every occurrence from there to the end of the run is replaced.
fn rewrite_run(run: &mut [Stmt], next_local_id: &mut LocalId) -> Vec<(usize, Stmt)> {
    // Candidate key per guard: its first unconditional read, if any.
    let candidates: Vec<Option<ReadKey>> = run
        .iter()
        .map(|s| diverging_guard(s).and_then(first_unconditional_read))
        .collect();

    let mut hoists: Vec<(usize, Stmt)> = Vec::new();
    let mut claimed: Vec<ReadKey> = Vec::new();
    for (at, cand) in candidates.iter().enumerate() {
        let Some(key) = cand else { continue };
        if claimed.contains(key) {
            continue;
        }
        // Occurrences from this guard to the end of the run — the region the
        // hoisted value dominates.
        let mut uses = 0usize;
        for stmt in run[at..].iter() {
            if let Some(cond) = diverging_guard(stmt) {
                uses += count_reads_of(cond, key);
            }
        }
        if uses < 2 {
            continue;
        }
        let id = *next_local_id;
        *next_local_id += 1;
        hoists.push((
            at,
            Stmt::Let {
                id,
                name: format!("__perry_cse_{}_{}", key.1, id),
                // `Any` — the hoist must not claim knowledge the read did not
                // have. Codegen's string-literal `===` inlining (#7767) keys
                // on the literal side, so it still fires through an `Any`.
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(key.0)),
                    property: key.1.clone(),
                    byte_offset: 0,
                }),
            },
        ));
        let subst: HashMap<ReadKey, LocalId> = std::iter::once((key.clone(), id)).collect();
        for stmt in run[at..].iter_mut() {
            if let Stmt::If { condition, .. } = stmt {
                substitute_reads(condition, &subst);
            }
        }
        claimed.push(key.clone());
    }
    hoists
}

/// The first `local.prop` read this expression evaluates on every path,
/// walking left to right and never entering the right-hand side of a
/// short-circuiting operator.
fn first_unconditional_read(e: &Expr) -> Option<ReadKey> {
    match e {
        Expr::PropertyGet {
            object, property, ..
        } => match object.as_ref() {
            Expr::LocalGet(id) => Some((*id, property.clone())),
            _ => None,
        },
        Expr::Compare { left, right, .. } => {
            first_unconditional_read(left).or_else(|| first_unconditional_read(right))
        }
        // Only the LEFT operand of `&&`/`||`/`??` is unconditional.
        Expr::Logical { left, .. } => first_unconditional_read(left),
        Expr::Unary { operand, .. } | Expr::TypeOf(operand) => first_unconditional_read(operand),
        _ => None,
    }
}

fn count_reads_of(e: &Expr, key: &ReadKey) -> usize {
    match e {
        Expr::PropertyGet {
            object, property, ..
        } => match object.as_ref() {
            Expr::LocalGet(id) if *id == key.0 && *property == key.1 => 1,
            _ => 0,
        },
        Expr::Compare { left, right, .. } | Expr::Logical { left, right, .. } => {
            count_reads_of(left, key) + count_reads_of(right, key)
        }
        Expr::Unary { operand, .. } | Expr::TypeOf(operand) => count_reads_of(operand, key),
        _ => 0,
    }
}

fn substitute_reads(e: &mut Expr, subst: &HashMap<ReadKey, LocalId>) {
    match e {
        Expr::PropertyGet {
            object, property, ..
        } => {
            if let Expr::LocalGet(id) = object.as_ref() {
                if let Some(tmp) = subst.get(&(*id, property.clone())) {
                    *e = Expr::LocalGet(*tmp);
                }
            }
        }
        Expr::Compare { left, right, .. } | Expr::Logical { left, right, .. } => {
            substitute_reads(left, subst);
            substitute_reads(right, subst);
        }
        Expr::Unary { operand, .. } | Expr::TypeOf(operand) => substitute_reads(operand, subst),
        _ => {}
    }
}

/// Statement lists nested directly inside `s`.
fn nested_stmt_lists(s: &mut Stmt) -> Vec<&mut Vec<Stmt>> {
    match s {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => match else_branch {
            Some(e) => vec![then_branch, e],
            None => vec![then_branch],
        },
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => vec![body],
        Stmt::For { body, .. } => vec![body],
        Stmt::Labeled { body, .. } => nested_stmt_lists(body),
        Stmt::Switch { cases, .. } => cases.iter_mut().map(|c| &mut c.body).collect(),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            let mut v = vec![body];
            if let Some(c) = catch {
                v.push(&mut c.body);
            }
            if let Some(f) = finally {
                v.push(f);
            }
            v
        }
        _ => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Module-wide accessor veto
// ---------------------------------------------------------------------------

/// Property names that install an accessor. None of them is a plausible
/// user-defined method name, so matching on the name alone (rather than on
/// `Object.` as the receiver) costs no realistic optimization opportunity and
/// keeps the scan a flat expression walk.
const ACCESSOR_INSTALLERS: &[&str] = &[
    "defineProperty",
    "defineProperties",
    "__defineGetter__",
    "__defineSetter__",
];

/// Extern helpers the object-literal lowering emits for `{ get x() {} }`
/// (`SpreadOp::DefineAccessor`, `lower/expr_object.rs`).
const ACCESSOR_EXTERNS: &[&str] = &[
    "js_object_define_accessor",
    "js_object_define_property",
    "js_class_define_accessor",
];

/// Two residual holes, both deliberate and both closed by `PERRY_PROP_CSE=0`:
///
/// * `Object.create(proto, descriptors)` can install a getter through a
///   descriptor object without naming `defineProperty`. Vetoing every
///   `.create(` would disable the pass for most real modules to close a
///   pattern that essentially does not occur.
/// * An object built in **another** module by a getter-defining module and
///   passed in. The veto is per-module because the transform pipeline is.
fn module_defines_accessors(module: &Module) -> bool {
    for c in &module.classes {
        if !c.getters.is_empty() || !c.setters.is_empty() {
            return true;
        }
    }
    let mut found = false;
    let scan = |stmts: &Vec<Stmt>, found: &mut bool| {
        for s in stmts {
            scan_stmt(s, found);
        }
    };
    scan(&module.init, &mut found);
    for f in &module.functions {
        scan(&f.body, &mut found);
    }
    for c in &module.classes {
        if let Some(ctor) = &c.constructor {
            scan(&ctor.body, &mut found);
        }
        for m in &c.methods {
            scan(&m.body, &mut found);
        }
    }
    found
}

fn scan_stmt(s: &Stmt, found: &mut bool) {
    if *found {
        return;
    }
    for e in stmt_exprs(s) {
        scan_expr(e, found);
    }
    // `nested_stmt_lists` needs `&mut`; this immutable scan mirrors it.
    match s {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            for st in then_branch {
                scan_stmt(st, found);
            }
            if let Some(e) = else_branch {
                for st in e {
                    scan_stmt(st, found);
                }
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            for st in body {
                scan_stmt(st, found);
            }
        }
        // `For`'s init is a `Stmt`, not an expression — `stmt_exprs` cannot
        // reach it, so the veto would miss an accessor installed there.
        Stmt::For { init, body, .. } => {
            if let Some(init) = init {
                scan_stmt(init, found);
            }
            for st in body {
                scan_stmt(st, found);
            }
        }
        Stmt::Labeled { body, .. } => scan_stmt(body, found),
        Stmt::Switch { cases, .. } => {
            for c in cases {
                for st in &c.body {
                    scan_stmt(st, found);
                }
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for st in body {
                scan_stmt(st, found);
            }
            if let Some(c) = catch {
                for st in &c.body {
                    scan_stmt(st, found);
                }
            }
            if let Some(f) = finally {
                for st in f {
                    scan_stmt(st, found);
                }
            }
        }
        _ => {}
    }
}

fn stmt_exprs(s: &Stmt) -> Vec<&Expr> {
    match s {
        Stmt::Let { init, .. } => init.iter().collect(),
        Stmt::Expr(e) | Stmt::Throw(e) => vec![e],
        Stmt::Return(e) => e.iter().collect(),
        Stmt::If { condition, .. }
        | Stmt::While { condition, .. }
        | Stmt::DoWhile { condition, .. } => vec![condition],
        Stmt::For {
            condition, update, ..
        } => condition.iter().chain(update.iter()).collect(),
        Stmt::Switch {
            discriminant,
            cases,
        } => std::iter::once(discriminant)
            .chain(cases.iter().filter_map(|c| c.test.as_ref()))
            .collect(),
        _ => Vec::new(),
    }
}

fn scan_expr(e: &Expr, found: &mut bool) {
    if *found {
        return;
    }
    if expr_installs_accessor(e) {
        *found = true;
        return;
    }
    // A closure body is a `Vec<Stmt>` the expression walker does not descend
    // into; a getter installed from inside one still counts.
    if let Expr::Closure { body, .. } = e {
        for st in body {
            scan_stmt(st, found);
        }
    }
    perry_hir::walker::walk_expr_children(e, &mut |child: &Expr| scan_expr(child, found));
}

fn expr_installs_accessor(e: &Expr) -> bool {
    match e {
        Expr::ExternFuncRef { name, .. } => ACCESSOR_EXTERNS.contains(&name.as_str()),
        Expr::PropertyGet { property, .. } => ACCESSOR_INSTALLERS.contains(&property.as_str()),
        Expr::New { class_name, .. } => class_name == "Proxy",
        Expr::ClassRef(name) => name == "Proxy",
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const N: LocalId = 1;

    fn read(id: LocalId, prop: &str) -> Expr {
        Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(id)),
            property: prop.to_string(),
            byte_offset: 0,
        }
    }

    fn guard(prop: &str, lit: &str, body: Vec<Stmt>) -> Stmt {
        Stmt::If {
            condition: Expr::Compare {
                op: CompareOp::Eq,
                left: Box::new(read(N, prop)),
                right: Box::new(Expr::String(lit.to_string())),
            },
            then_branch: body,
            else_branch: None,
        }
    }

    fn diverging(prop: &str, lit: &str) -> Stmt {
        guard(prop, lit, vec![Stmt::Return(Some(Expr::Integer(0)))])
    }

    /// Every `local.prop` read still present anywhere in `stmts`.
    fn count_reads(stmts: &[Stmt], prop: &str) -> usize {
        fn in_expr(e: &Expr, prop: &str, n: &mut usize) {
            if let Expr::PropertyGet { property, .. } = e {
                if property == prop {
                    *n += 1;
                }
            }
            perry_hir::walker::walk_expr_children(e, &mut |c: &Expr| in_expr(c, prop, n));
        }
        let mut n = 0;
        for s in stmts {
            for e in stmt_exprs(s) {
                in_expr(e, prop, &mut n);
            }
            if let Stmt::If {
                then_branch,
                else_branch,
                ..
            } = s
            {
                n += count_reads(then_branch, prop);
                if let Some(e) = else_branch {
                    n += count_reads(e, prop);
                }
            }
        }
        n
    }

    #[test]
    fn hoists_a_diverging_guard_chain_to_one_read() {
        let mut body = vec![
            diverging("kind", "num"),
            diverging("kind", "str"),
            diverging("kind", "var"),
            diverging("kind", "fun"),
        ];
        assert_eq!(count_reads(&body, "kind"), 4);
        let mut next = 100;
        cse_stmts(&mut body, &mut next);
        assert_eq!(
            count_reads(&body, "kind"),
            1,
            "four guards must collapse to the single hoisted read"
        );
        assert_eq!(body.len(), 5, "one Let inserted in front of the run");
        match &body[0] {
            Stmt::Let { id, init, .. } => {
                assert_eq!(*id, 100);
                assert!(matches!(init, Some(Expr::PropertyGet { .. })));
            }
            other => panic!("expected the hoisted Let, got {:?}", other),
        }
        // The guards now compare the temporary.
        for s in &body[1..] {
            match s {
                Stmt::If {
                    condition: Expr::Compare { left, .. },
                    ..
                } => assert!(matches!(left.as_ref(), Expr::LocalGet(100))),
                other => panic!("guard was rewritten wrong: {:?}", other),
            }
        }
    }

    /// The divergence requirement is the whole safety argument: a body that
    /// falls through can call anything, and the next guard's read is then a
    /// fresh observation.
    #[test]
    fn a_falling_through_body_blocks_the_hoist() {
        let mut body = vec![
            guard("kind", "num", vec![Stmt::Expr(Expr::Integer(0))]),
            guard("kind", "str", vec![Stmt::Expr(Expr::Integer(0))]),
            guard("kind", "var", vec![Stmt::Expr(Expr::Integer(0))]),
        ];
        let mut next = 100;
        cse_stmts(&mut body, &mut next);
        assert_eq!(count_reads(&body, "kind"), 3);
        assert_eq!(next, 100, "no temporary was minted");
    }

    /// One call in the middle splits the run in two, and neither half repeats.
    #[test]
    fn a_call_between_guards_splits_the_run() {
        let mut body = vec![
            diverging("kind", "num"),
            Stmt::Expr(Expr::Call {
                callee: Box::new(Expr::FuncRef(7)),
                args: vec![],
                type_args: vec![],
                byte_offset: 0,
            }),
            diverging("kind", "str"),
        ];
        let mut next = 100;
        cse_stmts(&mut body, &mut next);
        assert_eq!(count_reads(&body, "kind"), 2);
        assert_eq!(next, 100);
    }

    /// `==` coerces, and coercion calls `valueOf`/`toString`.
    #[test]
    fn loose_equality_is_not_analyzable() {
        let mk = |lit: &str| Stmt::If {
            condition: Expr::Compare {
                op: CompareOp::LooseEq,
                left: Box::new(read(N, "kind")),
                right: Box::new(Expr::String(lit.to_string())),
            },
            then_branch: vec![Stmt::Return(None)],
            else_branch: None,
        };
        let mut body = vec![mk("num"), mk("str"), mk("var")];
        let mut next = 100;
        cse_stmts(&mut body, &mut next);
        assert_eq!(count_reads(&body, "kind"), 3);
        assert_eq!(next, 100);
    }

    /// Relational compares coerce too.
    #[test]
    fn relational_compare_is_not_analyzable() {
        assert!(!guard_expr_is_analyzable(&Expr::Compare {
            op: CompareOp::Lt,
            left: Box::new(read(N, "kind")),
            right: Box::new(Expr::Integer(1)),
        }));
    }

    #[test]
    fn a_single_occurrence_is_not_hoisted() {
        let mut body = vec![diverging("kind", "num"), diverging("op", "+")];
        let mut next = 100;
        cse_stmts(&mut body, &mut next);
        assert_eq!(count_reads(&body, "kind"), 1);
        assert_eq!(count_reads(&body, "op"), 1);
        assert_eq!(next, 100);
    }

    /// Two distinct repeated keys interleaved in one run: each `Let` lands
    /// immediately before the guard that first read it unconditionally, never
    /// at the top of the run.
    #[test]
    fn two_repeated_keys_hoist_at_their_own_first_guard() {
        let mut body = vec![
            diverging("op", "+"),
            diverging("kind", "num"),
            diverging("op", "-"),
            diverging("kind", "str"),
        ];
        let mut next = 100;
        cse_stmts(&mut body, &mut next);
        assert_eq!(count_reads(&body, "op"), 1);
        assert_eq!(count_reads(&body, "kind"), 1);
        let props: Vec<Option<&str>> = body
            .iter()
            .map(|s| match s {
                Stmt::Let {
                    init: Some(Expr::PropertyGet { property, .. }),
                    ..
                } => Some(property.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            props,
            vec![Some("op"), None, Some("kind"), None, None, None],
            "each Let sits in front of its own first guard"
        );
    }

    /// The evaluation-order rule: a key first read by a LATER guard must not
    /// be hoisted to the top of the run, or a receiver that is only safe to
    /// dereference once the earlier guards have failed would throw early.
    #[test]
    fn a_key_first_read_later_is_not_hoisted_above_the_earlier_guards() {
        let other: LocalId = 2;
        let later = |lit: &str| Stmt::If {
            condition: Expr::Compare {
                op: CompareOp::Eq,
                left: Box::new(read(other, "y")),
                right: Box::new(Expr::String(lit.to_string())),
            },
            then_branch: vec![Stmt::Return(None)],
            else_branch: None,
        };
        let mut body = vec![diverging("kind", "num"), later("a"), later("b")];
        let mut next = 100;
        cse_stmts(&mut body, &mut next);
        // `y` is hoisted, but only in front of the guard at index 1.
        assert_eq!(count_reads(&body, "y"), 1);
        assert!(
            matches!(&body[0], Stmt::If { .. }),
            "the run still starts with the original first guard, not a Let"
        );
        assert!(matches!(&body[1], Stmt::Let { .. }));
    }

    /// A read under the right-hand side of `&&` is conditional: the original
    /// may never evaluate it, so it is not a hoist candidate at that guard.
    #[test]
    fn a_short_circuited_read_is_not_a_candidate() {
        let other: LocalId = 2;
        let g = |lit: &str| Stmt::If {
            condition: Expr::Logical {
                op: perry_hir::LogicalOp::And,
                left: Box::new(Expr::Compare {
                    op: CompareOp::Eq,
                    left: Box::new(read(N, "kind")),
                    right: Box::new(Expr::String("bin".to_string())),
                }),
                right: Box::new(Expr::Compare {
                    op: CompareOp::Eq,
                    left: Box::new(read(other, "y")),
                    right: Box::new(Expr::String(lit.to_string())),
                }),
            },
            then_branch: vec![Stmt::Return(None)],
            else_branch: None,
        };
        let mut body = vec![g("a"), g("b")];
        let mut next = 100;
        cse_stmts(&mut body, &mut next);
        assert_eq!(
            count_reads(&body, "y"),
            2,
            "the short-circuited read must survive"
        );
        assert_eq!(count_reads(&body, "kind"), 1, "the left read still hoists");
    }

    /// Nested runs inside a diverging body are handled in their own right —
    /// this is the `if (n.kind === "bin") { … if (n.op === "+") … }` shape.
    #[test]
    fn a_nested_run_inside_a_diverging_body_is_rewritten() {
        let inner = vec![
            diverging("op", "+"),
            diverging("op", "-"),
            diverging("op", "*"),
            Stmt::Return(None),
        ];
        let mut body = vec![guard("kind", "bin", inner)];
        let mut next = 100;
        cse_stmts(&mut body, &mut next);
        assert_eq!(count_reads(&body, "op"), 1);
        assert_eq!(count_reads(&body, "kind"), 1);
    }

    fn accessor_module(f: Function) -> Module {
        let mut m = Module::new("t");
        m.functions.push(f);
        m
    }

    fn fn_with(body: Vec<Stmt>) -> Function {
        Function {
            id: 1,
            name: "f".to_string(),
            type_params: vec![],
            params: vec![],
            return_type: Type::Any,
            body,
            is_async: false,
            is_generator: false,
            is_strict: false,
            is_exported: false,
            captures: vec![],
            decorators: vec![],
            was_plain_async: false,
            was_unrolled: false,
        }
    }

    /// The whole-module veto: a `defineProperty` anywhere turns the pass off,
    /// because a property read is then no longer provably free of user code.
    #[test]
    fn define_property_anywhere_vetoes_the_module() {
        let body = vec![
            Stmt::Expr(Expr::Call {
                callee: Box::new(read(9, "defineProperty")),
                args: vec![],
                type_args: vec![],
                byte_offset: 0,
            }),
            diverging("kind", "num"),
            diverging("kind", "str"),
        ];
        let mut m = accessor_module(fn_with(body));
        assert!(module_defines_accessors(&m));
        run(&mut m);
        assert_eq!(count_reads(&m.functions[0].body, "kind"), 2);
    }

    #[test]
    fn an_object_literal_accessor_vetoes_the_module() {
        let body = vec![Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::ExternFuncRef {
                name: "js_object_define_accessor".to_string(),
                param_types: vec![],
                return_type: Type::Void,
            }),
            args: vec![],
            type_args: vec![],
            byte_offset: 0,
        })];
        let m = accessor_module(fn_with(body));
        assert!(module_defines_accessors(&m));
    }

    /// `For`'s init is a `Stmt`, not an expression, so the flat expression
    /// walk cannot reach it — the veto has to recurse into it explicitly.
    #[test]
    fn an_accessor_installed_in_a_for_init_vetoes_the_module() {
        let body = vec![Stmt::For {
            init: Some(Box::new(Stmt::Expr(Expr::Call {
                callee: Box::new(read(9, "defineProperty")),
                args: vec![],
                type_args: vec![],
                byte_offset: 0,
            }))),
            condition: None,
            update: None,
            body: vec![],
        }];
        let m = accessor_module(fn_with(body));
        assert!(module_defines_accessors(&m));
    }

    #[test]
    fn a_clean_module_is_not_vetoed_and_is_rewritten() {
        let mut m = accessor_module(fn_with(vec![
            diverging("kind", "num"),
            diverging("kind", "str"),
        ]));
        assert!(!module_defines_accessors(&m));
        run(&mut m);
        assert_eq!(count_reads(&m.functions[0].body, "kind"), 1);
    }

    /// Async and generator bodies are skipped: the async→generator transform
    /// runs after this pass and boxes every body local.
    #[test]
    fn async_bodies_are_skipped() {
        let mut f = fn_with(vec![diverging("kind", "num"), diverging("kind", "str")]);
        f.is_async = true;
        let mut next = 100;
        run_function(&mut f, &mut next);
        assert_eq!(count_reads(&f.body, "kind"), 2);
        assert_eq!(next, 100);
    }
}
