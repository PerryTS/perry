//! The parameter-fate rule on the fixture and counterexample shapes of the
//! LIFE D design (`target/life/d/fix.ts`, `target/life/d/cx/`), built by hand.

use std::collections::{HashMap, HashSet};

use super::*;
use perry_hir::types::{FuncId, Type};
use perry_hir::{BinaryOp, CompareOp, Expr, Function, Param, Stmt};

/// Reads proven by `(function, key)`; a key in `bets` is proven on a guard.
#[derive(Default)]
struct TestProof {
    reads: HashSet<(FuncId, &'static str)>,
    bets: HashMap<(FuncId, &'static str), u64>,
}

impl TestProof {
    fn reading(pairs: &[(FuncId, &'static str)]) -> Self {
        Self {
            reads: pairs.iter().copied().collect(),
            ..Self::default()
        }
    }
}

impl FateProof for TestProof {
    fn read(&self, func: FuncId, _: &Expr, property: &str, _: u32) -> Option<Proof> {
        let key = self
            .reads
            .iter()
            .find(|(f, k)| *f == func && *k == property);
        if key.is_some() {
            return Some(Proof::Proven);
        }
        self.bets
            .iter()
            .find(|((f, k), _)| *f == func && *k == property)
            .map(|(_, site)| Proof::Bet(*site))
    }
    fn store(&self, _: FuncId, _: &str) -> Option<Proof> {
        None
    }
    fn targets(&self, callee: &Expr) -> Option<(Vec<FuncId>, Proof)> {
        direct_target(callee)
    }
}

fn param(id: u32) -> Param {
    Param {
        id,
        name: format!("p{id}"),
        ty: Type::Any,
        default: None,
        decorators: vec![],
        is_rest: false,
        arguments_object: None,
    }
}

fn func(id: FuncId, params: &[u32], body: Vec<Stmt>) -> Function {
    Function {
        id,
        name: format!("f{id}"),
        type_params: vec![],
        params: params.iter().map(|p| param(*p)).collect(),
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    }
}

fn lg(id: u32) -> Expr {
    Expr::LocalGet(id)
}

fn get(object: Expr, key: &str) -> Expr {
    Expr::PropertyGet {
        object: Box::new(object),
        property: key.into(),
        byte_offset: 0,
        byte_end: 0,
    }
}

fn set(object: Expr, key: &str, value: Expr) -> Expr {
    Expr::PropertySet {
        object: Box::new(object),
        property: key.into(),
        value: Box::new(value),
        byte_offset: 0,
        byte_end: 0,
    }
}

fn call(callee: Expr, args: Vec<Expr>) -> Expr {
    Expr::Call {
        callee: Box::new(callee),
        args,
        type_args: vec![],
        byte_offset: 0,
        byte_end: 0,
    }
}

fn add(l: Expr, r: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(l),
        right: Box::new(r),
    }
}

fn eq(l: Expr, r: Expr) -> Expr {
    Expr::Compare {
        op: CompareOp::Eq,
        left: Box::new(l),
        right: Box::new(r),
    }
}

fn let_(id: u32, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: format!("l{id}"),
        ty: Type::Any,
        mutable: true,
        init: Some(init),
    }
}

fn ret(e: Expr) -> Stmt {
    Stmt::Return(Some(e))
}

fn if_(c: Expr, then: Vec<Stmt>) -> Stmt {
    Stmt::If {
        condition: c,
        then_branch: then,
        else_branch: None,
    }
}

/// `kept.push(q)` on a module array (local 900): a method call on an
/// unknown target.
fn push(q: Expr) -> Stmt {
    Stmt::Expr(call(get(lg(900), "push"), vec![q]))
}

fn fates(fns: &[Function], proof: &TestProof) -> Fates {
    let refs: Vec<&Function> = fns.iter().collect();
    Fates::compute(&refs, &[], proof)
}

fn fate(f: &Fates, id: FuncId, i: usize) -> Fate {
    f.of_fn(id).expect("function has fates").params[i].fate
}

/// The fixture's `score(p)`: reads `p.x`, `p.y`, then arithmetic on locals.
fn score() -> Function {
    func(
        1,
        &[10],
        vec![
            let_(11, get(lg(10), "x")),
            let_(12, get(lg(10), "y")),
            let_(13, add(lg(11), lg(12))),
            ret(lg(13)),
        ],
    )
}

#[test]
fn fixture_score_drops_its_parameter() {
    let f = fates(&[score()], &TestProof::reading(&[(1, "x"), (1, "y")]));
    assert!(f.of_fn(1).unwrap().params[0].proven_dropped());
}

#[test]
fn a_read_the_proof_rejects_keeps() {
    // c6 (`p.leak` is a getter) and c9b (`(p as any).z` from a prototype
    // getter) look the same to the rule: the proof does not prove the read.
    let f = fates(&[score()], &TestProof::reading(&[(1, "x")]));
    assert_eq!(fate(&f, 1, 0), Fate::Kept);
}

#[test]
fn a_guarded_read_is_a_bet() {
    let mut proof = TestProof::reading(&[(1, "x")]);
    proof.bets.insert((1, "y"), 7);
    let f = fates(&[score()], &proof);
    let p = f.of_fn(1).unwrap().params[0];
    assert_eq!(p.fate, Fate::Dropped);
    assert_eq!(p.proof, Proof::Bet(7));
    assert!(!p.proven_dropped());
}

#[test]
fn c1_store_into_module_variable_keeps() {
    let f1 = func(
        1,
        &[10],
        vec![Stmt::Expr(Expr::LocalSet(901, Box::new(lg(10))))],
    );
    assert_eq!(fate(&fates(&[f1], &TestProof::default()), 1, 0), Fate::Kept);
}

#[test]
fn c2_return_is_returned() {
    let f1 = func(1, &[10], vec![let_(11, get(lg(10), "x")), ret(lg(10))]);
    let f = fates(&[f1], &TestProof::reading(&[(1, "x")]));
    assert_eq!(fate(&f, 1, 0), Fate::Returned);
}

#[test]
fn c3_store_into_other_parameter_keeps_both() {
    let f1 = func(1, &[10, 20], vec![Stmt::Expr(set(lg(20), "v", lg(10)))]);
    let f = fates(&[f1], &TestProof::default());
    assert_eq!(fate(&f, 1, 0), Fate::Kept);
    assert_eq!(fate(&f, 1, 1), Fate::Kept);
}

#[test]
fn c4_capture_keeps() {
    let closure = Expr::Closure {
        func_id: 2,
        params: vec![],
        return_type: Type::Any,
        body: vec![ret(get(lg(10), "x"))],
        captures: vec![10],
        mutable_captures: vec![],
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: true,
    };
    let f1 = func(1, &[10], vec![ret(closure)]);
    let f = fates(&[f1], &TestProof::reading(&[(1, "x"), (2, "x")]));
    assert_eq!(fate(&f, 1, 0), Fate::Kept);
}

#[test]
fn c5_pass_on_to_a_keeper_keeps() {
    let g = func(2, &[20], vec![push(lg(20)), ret(get(lg(20), "x"))]);
    let f1 = func(1, &[10], vec![ret(call(Expr::FuncRef(2), vec![lg(10)]))]);
    let f = fates(&[f1, g], &TestProof::reading(&[(2, "x")]));
    assert_eq!(fate(&f, 2, 0), Fate::Kept);
    assert_eq!(fate(&f, 1, 0), Fate::Kept);
}

#[test]
fn c7_throw_keeps() {
    let f1 = func(1, &[10], vec![Stmt::Throw(lg(10))]);
    assert_eq!(fate(&fates(&[f1], &TestProof::default()), 1, 0), Fate::Kept);
}

/// `walk(p, n)`: at depth 0 `keep` keeps p, else recurse.
fn walker(keep: bool) -> Function {
    let base = if keep {
        vec![push(lg(10)), ret(get(lg(10), "x"))]
    } else {
        vec![ret(get(lg(10), "x"))]
    };
    func(
        1,
        &[10, 11],
        vec![
            if_(eq(lg(11), Expr::Number(0.0)), base),
            ret(add(
                get(lg(10), "x"),
                call(Expr::FuncRef(1), vec![lg(10), lg(11)]),
            )),
        ],
    )
}

#[test]
fn c8_recursion_that_keeps_at_depth_zero_keeps() {
    let f = fates(&[walker(true)], &TestProof::reading(&[(1, "x")]));
    assert_eq!(fate(&f, 1, 0), Fate::Kept);
}

#[test]
fn recursion_that_only_passes_on_drops() {
    let f = fates(&[walker(false)], &TestProof::reading(&[(1, "x")]));
    assert!(f.of_fn(1).unwrap().params[0].proven_dropped());
    // `n` is compared and passed on: dropped too.
    assert_eq!(fate(&f, 1, 1), Fate::Dropped);
}

#[test]
fn c10_callee_drops_the_alias_is_the_callers() {
    // `f(p)` reads only: dropped. The caller's `prev = p` is a keep in the
    // caller, seen when the caller is the body read.
    let f1 = func(1, &[10], vec![ret(get(lg(10), "x"))]);
    let caller = func(
        2,
        &[20],
        vec![
            Stmt::Expr(call(Expr::FuncRef(1), vec![lg(20)])),
            Stmt::Expr(Expr::LocalSet(902, Box::new(lg(20)))),
        ],
    );
    let f = fates(&[f1, caller], &TestProof::reading(&[(1, "x")]));
    assert_eq!(fate(&f, 1, 0), Fate::Dropped);
    assert_eq!(fate(&f, 2, 0), Fate::Kept);
}

#[test]
fn c11_identity_key_of_a_builtin_call_keeps() {
    // `ws.set(p, s)`: a method of an unknown target.
    let f1 = func(
        1,
        &[10],
        vec![Stmt::Expr(call(
            get(lg(903), "set"),
            vec![lg(10), Expr::Number(1.0)],
        ))],
    );
    assert_eq!(fate(&fates(&[f1], &TestProof::default()), 1, 0), Fate::Kept);
}

/// `a(p, n)` calls `b(p, n)` calls `a`; `b` keeps p when `keep`.
fn mutual(keep: bool) -> Vec<Function> {
    let a = func(
        1,
        &[10, 11],
        vec![
            let_(12, get(lg(10), "x")),
            ret(call(Expr::FuncRef(2), vec![lg(10), lg(11)])),
        ],
    );
    let mut b_body = vec![];
    if keep {
        b_body.push(push(lg(20)));
    }
    b_body.push(ret(call(Expr::FuncRef(1), vec![lg(20), lg(21)])));
    vec![a, func(2, &[20, 21], b_body)]
}

#[test]
fn c12_keep_inside_a_mutual_recursion_keeps_the_cycle() {
    let f = fates(&mutual(true), &TestProof::reading(&[(1, "x")]));
    assert_eq!(fate(&f, 1, 0), Fate::Kept);
    assert_eq!(fate(&f, 2, 0), Fate::Kept);
}

#[test]
fn mutual_recursion_that_only_reads_drops() {
    let f = fates(&mutual(false), &TestProof::reading(&[(1, "x")]));
    assert!(f.of_fn(1).unwrap().params[0].proven_dropped());
    assert!(f.of_fn(2).unwrap().params[0].proven_dropped());
}

#[test]
fn an_unknown_callee_keeps() {
    // `g(p)` where `g` is a local: not a direct reference.
    let f1 = func(1, &[10], vec![Stmt::Expr(call(lg(904), vec![lg(10)]))]);
    assert_eq!(fate(&fates(&[f1], &TestProof::default()), 1, 0), Fate::Kept);
    // A direct reference to a body outside the table keeps too.
    let f2 = func(
        1,
        &[10],
        vec![Stmt::Expr(call(Expr::FuncRef(77), vec![lg(10)]))],
    );
    assert_eq!(fate(&fates(&[f2], &TestProof::default()), 1, 0), Fate::Kept);
}

#[test]
fn compares_drop_operators_keep() {
    let cmp = func(1, &[10, 11], vec![ret(eq(lg(10), lg(11)))]);
    let f = fates(&[cmp], &TestProof::default());
    assert_eq!(fate(&f, 1, 0), Fate::Dropped);
    // `p + 1` may run `valueOf` with p as `this`.
    let plus = func(1, &[10], vec![ret(add(lg(10), Expr::Number(1.0)))]);
    assert_eq!(
        fate(&fates(&[plus], &TestProof::default()), 1, 0),
        Fate::Kept
    );
}

#[test]
fn not_ordinary_keeps_everything() {
    let mut f1 = func(1, &[10], vec![ret(get(lg(10), "x"))]);
    f1.is_async = true;
    assert_eq!(
        fate(&fates(&[f1], &TestProof::reading(&[(1, "x")])), 1, 0),
        Fate::Kept
    );
}

#[test]
fn this_of_a_reading_method_drops() {
    let m = func(1, &[], vec![ret(get(Expr::This, "x"))]);
    let f = fates(&[m], &TestProof::reading(&[(1, "x")]));
    assert!(f.of_fn(1).unwrap().this.proven_dropped());
}
