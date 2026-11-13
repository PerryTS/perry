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
    /// `<any>.<key>(..)` calls resolved to one body.
    methods: Vec<(&'static str, FuncId)>,
    /// Locals proven to hold only Numbers.
    numbers: HashSet<u32>,
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
        if let Expr::PropertyGet { property, .. } = callee {
            if let Some((_, f)) = self.methods.iter().find(|(k, _)| k == property) {
                return Some((vec![*f], Proof::Proven));
            }
        }
        direct_target(callee)
    }
    fn primitive(&self, _: FuncId, local: u32) -> Option<Proof> {
        self.numbers.contains(&local).then_some(Proof::Proven)
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

// ─── Why a parameter is `Kept`: the first keeping use (data only) ───────

fn why(f: &Fates, id: FuncId, i: usize) -> String {
    let p = f.of_fn(id).expect("function has fates").params[i];
    assert_eq!(p.fate, Fate::Kept, "only Kept carries a reason");
    p.why.expect("Kept has a reason").to_string()
}

fn this_why(f: &Fates, id: FuncId) -> String {
    let p = f.of_fn(id).expect("function has fates").this;
    assert_eq!(p.fate, Fate::Kept);
    p.why.expect("Kept has a reason").to_string()
}

fn closure(body: Vec<Stmt>, captures_this: bool) -> Expr {
    Expr::Closure {
        func_id: 2,
        params: vec![],
        return_type: Type::Any,
        body,
        captures: vec![],
        mutable_captures: vec![],
        captures_this,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: true,
    }
}

#[test]
fn why_below_kept_is_none() {
    let f = fates(&[score()], &TestProof::reading(&[(1, "x"), (1, "y")]));
    assert_eq!(f.of_fn(1).unwrap().params[0].why, None);
}

#[test]
fn why_not_ordinary_names_the_part() {
    let mut f1 = func(1, &[10], vec![]);
    f1.is_async = true;
    assert_eq!(
        why(&fates(&[f1], &TestProof::default()), 1, 0),
        "not-ordinary(async)"
    );
    let mut f2 = func(1, &[10], vec![]);
    f2.params[0].is_rest = true;
    assert_eq!(why(&fates(&[f2], &TestProof::default()), 1, 0), "rest");
}

#[test]
fn why_use_names_the_parent_node() {
    let f1 = func(1, &[10], vec![ret(add(lg(10), Expr::Number(1.0)))]);
    assert_eq!(
        why(&fates(&[f1], &TestProof::default()), 1, 0),
        "use(Binary)"
    );
    let f2 = func(1, &[10], vec![Stmt::Throw(get(lg(10), "x"))]);
    assert_eq!(why(&fates(&[f2], &TestProof::default()), 1, 0), "read(x)");
    let f3 = func(
        1,
        &[10],
        vec![
            let_(11, lg(10)),
            Stmt::Expr(Expr::LocalSet(11, Box::new(Expr::Number(1.0)))),
        ],
    );
    assert_eq!(why(&fates(&[f3], &TestProof::default()), 1, 0), "use(Let)");
}

#[test]
fn why_use_names_the_store_role() {
    // `p.v = 1`: stored into; `o.v = p`: stored into another object;
    // `this.v = p` with the store not proven.
    let f1 = func(
        1,
        &[10],
        vec![Stmt::Expr(set(lg(10), "v", Expr::Number(1.0)))],
    );
    assert_eq!(
        why(&fates(&[f1], &TestProof::default()), 1, 0),
        "use(PropertySet.object)"
    );
    let f2 = func(1, &[10], vec![Stmt::Expr(set(lg(900), "v", lg(10)))]);
    assert_eq!(
        why(&fates(&[f2], &TestProof::default()), 1, 0),
        "use(PropertySet.value)"
    );
    let f3 = func(1, &[10], vec![Stmt::Expr(set(Expr::This, "v", lg(10)))]);
    assert_eq!(
        why(&fates(&[f3], &TestProof::default()), 1, 0),
        "use(PropertySet.value-to-this)"
    );
}

#[test]
fn why_first_use_wins() {
    let f1 = func(
        1,
        &[10],
        vec![
            Stmt::Throw(lg(10)),
            Stmt::Expr(Expr::LocalSet(901, Box::new(lg(10)))),
        ],
    );
    assert_eq!(
        why(&fates(&[f1], &TestProof::default()), 1, 0),
        "use(Throw)"
    );
}

#[test]
fn why_read_names_the_key() {
    let f = fates(&[score()], &TestProof::reading(&[(1, "x")]));
    assert_eq!(why(&f, 1, 0), "read(y)");
}

#[test]
fn why_store_keeps_this() {
    let f1 = func(
        1,
        &[10],
        vec![Stmt::Expr(set(Expr::This, "v", Expr::Number(1.0)))],
    );
    assert_eq!(
        this_why(&fates(&[f1], &TestProof::default()), 1),
        "store(v)"
    );
}

#[test]
fn why_receiver_of_unknown_method() {
    let f1 = func(
        1,
        &[10],
        vec![Stmt::Expr(call(get(lg(10), "slice"), vec![]))],
    );
    assert_eq!(
        why(&fates(&[f1], &TestProof::default()), 1, 0),
        "receiver(slice)"
    );
}

#[test]
fn why_receiver_kept_by_known_method() {
    // `m` keeps its `this` (stores it in a module variable).
    let m = func(
        2,
        &[],
        vec![Stmt::Expr(Expr::LocalSet(901, Box::new(Expr::This)))],
    );
    let f1 = func(1, &[10], vec![Stmt::Expr(call(get(lg(10), "m"), vec![]))]);
    let proof = TestProof {
        methods: vec![("m", 2)],
        ..TestProof::default()
    };
    let f = fates(&[f1, m], &proof);
    assert_eq!(this_why(&f, 2), "use(LocalSet)");
    assert_eq!(why(&f, 1, 0), "receiver-kept");
}

#[test]
fn why_argument_of_unresolved_call() {
    let f1 = func(1, &[10], vec![push(lg(10))]);
    assert_eq!(
        why(&fates(&[f1], &TestProof::default()), 1, 0),
        "arg-unresolved(PropertyGet.push)"
    );
}

#[test]
fn why_argument_to_a_known_keeper_and_call_arg_carries_its_reason() {
    let g = func(2, &[20], vec![push(lg(20))]);
    let f1 = func(
        1,
        &[10],
        vec![Stmt::Expr(call(Expr::FuncRef(2), vec![lg(10)]))],
    );
    let f = fates(&[f1, g], &TestProof::default());
    assert_eq!(why(&f, 1, 0), "arg-to(Kept)");
    let at = f.call_arg(&[2], 0);
    assert_eq!(
        at.why.map(|w| w.to_string()).as_deref(),
        Some("arg-unresolved(PropertyGet.push)")
    );
}

#[test]
fn why_argument_to_a_returner() {
    let g = func(2, &[20], vec![ret(lg(20))]);
    let f1 = func(
        1,
        &[10],
        vec![Stmt::Expr(call(Expr::FuncRef(2), vec![lg(10)]))],
    );
    assert_eq!(
        why(&fates(&[f1, g], &TestProof::default()), 1, 0),
        "arg-to(Returned)"
    );
}

#[test]
fn why_argument_of_a_body_without_fates() {
    let f1 = func(
        1,
        &[10],
        vec![Stmt::Expr(call(Expr::FuncRef(77), vec![lg(10)]))],
    );
    assert_eq!(
        why(&fates(&[f1], &TestProof::default()), 1, 0),
        "arg-no-fate"
    );
}

#[test]
fn why_argument_of_new_unknown_class() {
    let n = Expr::New {
        class_name: "Elsewhere".into(),
        args: vec![lg(10)],
        type_args: vec![],
        byte_offset: 0,
        byte_end: 0,
        cap_args_appended: 0,
    };
    let f1 = func(1, &[10], vec![Stmt::Expr(n)]);
    assert_eq!(
        why(&fates(&[f1], &TestProof::default()), 1, 0),
        "new-unknown(Elsewhere)"
    );
}

#[test]
fn why_captured_and_captures_this() {
    let f1 = func(1, &[10], vec![ret(closure(vec![ret(lg(10))], true))]);
    let f = fates(&[f1], &TestProof::default());
    assert_eq!(why(&f, 1, 0), "captured");
    assert_eq!(this_why(&f, 1), "captures-this");
}

#[test]
fn why_reassigned_updated_mutated() {
    // `q = 1` keeps nothing of q's value (rule c); `q = v; kept.push(q)`
    // keeps q's binding, so the incoming value is (conservatively) kept.
    let f1 = func(
        1,
        &[10],
        vec![
            Stmt::Expr(Expr::LocalSet(10, Box::new(Expr::Number(1.0)))),
            push(lg(10)),
        ],
    );
    assert_eq!(
        why(&fates(&[f1], &TestProof::default()), 1, 0),
        "arg-unresolved(PropertyGet.push)"
    );
    let up = Expr::Update {
        id: 10,
        op: perry_hir::UpdateOp::Increment,
        prefix: false,
    };
    let f2 = func(1, &[10], vec![Stmt::Expr(up)]);
    assert_eq!(why(&fates(&[f2], &TestProof::default()), 1, 0), "updated");
    let f3 = func(1, &[10], vec![Stmt::Expr(Expr::ArrayPop(10))]);
    assert_eq!(
        why(&fates(&[f3], &TestProof::default()), 1, 0),
        "mutated(ArrayPop)"
    );
}

#[test]
fn name_of_expr_is_the_variant() {
    assert_eq!(Name::of_expr(&Expr::Null).as_str(), "Null");
    assert_eq!(Name::of_expr(&lg(3)).as_str(), "LocalGet");
    assert_eq!(Name::of_expr(&get(lg(3), "k")).as_str(), "PropertyGet");
    assert_eq!(Name::new(&"x".repeat(40)).as_str().len(), 31);
}

// ─── FATE2 rule a: bodies with default and rest parameters are walked ───

/// `function f(a, b = a.x) { return b.x }`.
fn with_default(default: Expr, body: Vec<Stmt>) -> Function {
    let mut f = func(1, &[10, 11], body);
    f.params[1].default = Some(default);
    f
}

#[test]
fn a_default_that_reads_drops() {
    let f = with_default(get(lg(10), "x"), vec![ret(get(lg(11), "x"))]);
    assert!(
        !super::ordinary(&f),
        "effects.rs still sees a default as not ordinary"
    );
    let f = fates(&[f], &TestProof::reading(&[(1, "x")]));
    assert!(f.of_fn(1).unwrap().params[0].proven_dropped());
    assert!(f.of_fn(1).unwrap().params[1].proven_dropped());
}

#[test]
fn a_default_that_keeps_keeps() {
    // `b = kept.push(a)`: the default runs in the callee and keeps `a`.
    let f = with_default(call(get(lg(900), "push"), vec![lg(10)]), vec![]);
    let f = fates(&[f], &TestProof::default());
    assert_eq!(why(&f, 1, 0), "arg-unresolved(PropertyGet.push)");
    // `b = a; kept.push(b)`: `a` flows into `b`.
    let f = with_default(lg(10), vec![push(lg(11))]);
    let f = fates(&[f], &TestProof::default());
    assert_eq!(why(&f, 1, 0), "use(Default)");
    assert_eq!(fate(&f, 1, 1), Fate::Kept);
}

#[test]
fn a_call_in_a_default_is_an_edge() {
    // `g(q) { kept.push(q) }`, `f(a, b = g(a)) {}`: `a` is kept through g.
    let g = func(2, &[20], vec![push(lg(20))]);
    let f = with_default(call(Expr::FuncRef(2), vec![lg(10)]), vec![]);
    let f = fates(&[f, g], &TestProof::default());
    assert_eq!(why(&f, 1, 0), "arg-to(Kept)");
}

#[test]
fn a_rest_parameter_keeps_what_it_collects_and_the_body_is_walked() {
    // `function f(a, ...r) { return a.x }`
    let mut f = func(1, &[10, 11], vec![ret(get(lg(10), "x"))]);
    f.params[1].is_rest = true;
    let f = fates(&[f], &TestProof::reading(&[(1, "x")]));
    let ff = f.of_fn(1).unwrap();
    assert!(ff.params[0].proven_dropped());
    assert_eq!(why(&f, 1, 1), "rest");
    for i in 1..5 {
        assert_eq!(
            ff.arg(i).fate,
            Fate::Kept,
            "argument {i} lands in the rest array"
        );
    }
}

#[test]
fn arguments_past_the_parameters_reach_arguments() {
    // `function f(a) { ... arguments ... }`: HIR appends the hidden
    // `arguments` parameter; an argument past it is still `arguments[i]`.
    let mut f = func(1, &[10, 11], vec![]);
    f.params[1].is_rest = true;
    f.params[1].arguments_object = Some(perry_hir::ArgumentsObjectMeta {
        strict: false,
        simple_parameters: true,
        mapped_parameter_ids: vec![(0, 10)],
        restricted_callee: false,
    });
    let f = fates(&[f], &TestProof::default());
    let ff = f.of_fn(1).unwrap();
    assert_eq!(
        ff.params[0].fate,
        Fate::Kept,
        "sloppy arguments[0] aliases a"
    );
    assert_eq!(ff.arg(7).fate, Fate::Kept);
    // Without rest or `arguments` an extra argument reaches no binding.
    let f = fates(&[func(1, &[10], vec![])], &TestProof::default());
    assert!(f.of_fn(1).unwrap().arg(3).proven_dropped());
}

// ─── FATE2 rule b: typeof, truthiness and conversions of a primitive ────

fn logical(op: perry_hir::LogicalOp, l: Expr, r: Expr) -> Expr {
    Expr::Logical {
        op,
        left: Box::new(l),
        right: Box::new(r),
    }
}

fn not(e: Expr) -> Expr {
    Expr::Unary {
        op: perry_hir::UnaryOp::Not,
        operand: Box::new(e),
    }
}

fn cond(c: Expr, t: Expr, e: Expr) -> Expr {
    Expr::Conditional {
        condition: Box::new(c),
        then_expr: Box::new(t),
        else_expr: Box::new(e),
    }
}

#[test]
fn typeof_and_truthiness_drop() {
    use perry_hir::LogicalOp::{And, Coalesce, Or};
    let body = vec![
        // if (typeof a === "string") {}
        if_(
            eq(
                Expr::TypeOf(Box::new(lg(10))),
                Expr::String("string".into()),
            ),
            vec![],
        ),
        // if (a) {}  while (!a) {}
        if_(lg(10), vec![]),
        Stmt::While {
            condition: not(lg(10)),
            body: vec![Stmt::Break],
        },
        // let x = a ? 1 : 2; let y = Boolean(b)
        let_(12, cond(lg(10), Expr::Number(1.0), Expr::Number(2.0))),
        let_(13, Expr::BooleanCoerce(Box::new(lg(11)))),
        // if (a || b && (a ?? b)) {}  a && b;
        if_(
            logical(
                Or,
                lg(10),
                logical(And, lg(11), logical(Coalesce, lg(10), lg(11))),
            ),
            vec![],
        ),
        Stmt::Expr(logical(And, lg(10), lg(11))),
        Stmt::Expr(lg(11)),
    ];
    let f = fates(&[func(1, &[10, 11], body)], &TestProof::default());
    assert!(f.of_fn(1).unwrap().params[0].proven_dropped());
    assert!(f.of_fn(1).unwrap().params[1].proven_dropped());
}

#[test]
fn a_value_that_flows_on_from_a_test_keeps() {
    use perry_hir::LogicalOp::{Coalesce, Or};
    // let x = a || {}; kept.push(x): the result of `||` is `a` itself.
    let f1 = func(
        1,
        &[10],
        vec![
            let_(12, logical(Or, lg(10), Expr::Object(vec![]))),
            push(lg(12)),
        ],
    );
    assert_eq!(
        why(&fates(&[f1], &TestProof::default()), 1, 0),
        "use(Logical)"
    );
    // kept.push(a ?? b)
    let f2 = func(1, &[10, 11], vec![push(logical(Coalesce, lg(10), lg(11)))]);
    let f2 = fates(&[f2], &TestProof::default());
    assert_eq!(why(&f2, 1, 0), "use(Logical)");
    assert_eq!(why(&f2, 1, 1), "use(Logical)");
    // return c ? a : 0, in value position
    let f3 = func(
        1,
        &[10, 11],
        vec![push(cond(lg(11), lg(10), Expr::Number(0.0)))],
    );
    let f3 = fates(&[f3], &TestProof::default());
    assert_eq!(why(&f3, 1, 0), "use(Conditional)");
    assert!(
        f3.of_fn(1).unwrap().params[1].proven_dropped(),
        "the condition is only tested"
    );
}

#[test]
fn a_conversion_drops_a_proven_number_only() {
    let numbers = TestProof {
        numbers: [10].into_iter().collect(),
        ..TestProof::default()
    };
    let lt = |l, r| Expr::Compare {
        op: CompareOp::Lt,
        left: Box::new(l),
        right: Box::new(r),
    };
    let loose = |l, r| Expr::Compare {
        op: CompareOp::LooseEq,
        left: Box::new(l),
        right: Box::new(r),
    };
    let body = || {
        vec![
            let_(12, add(lg(10), Expr::Number(1.0))),
            let_(13, lt(lg(10), Expr::Number(3.0))),
            let_(14, loose(lg(10), Expr::Number(3.0))),
            let_(15, Expr::TemplateStringCoerce(Box::new(lg(10)))),
            let_(
                16,
                Expr::Unary {
                    op: perry_hir::UnaryOp::Neg,
                    operand: Box::new(lg(10)),
                },
            ),
        ]
    };
    let f = fates(&[func(1, &[10], body())], &numbers);
    assert!(f.of_fn(1).unwrap().params[0].proven_dropped());
    // Not proven: `a + 1`, `a < 3`, `a == 3`, `${a}`, `-a` call user code.
    let f = fates(&[func(1, &[10], body())], &TestProof::default());
    assert_eq!(why(&f, 1, 0), "use(Binary)");
    for (e, w) in [
        (lt(lg(10), Expr::Number(3.0)), "use(Compare)"),
        (loose(lg(10), Expr::Number(3.0)), "use(Compare)"),
        (
            Expr::TemplateStringCoerce(Box::new(lg(10))),
            "use(TemplateStringCoerce)",
        ),
    ] {
        let f = fates(&[func(1, &[10], vec![let_(12, e)])], &TestProof::default());
        assert_eq!(why(&f, 1, 0), w);
    }
}

// ─── FATE2 rule c: reassignment and never-written copies ────────────────

fn assign(id: u32, v: Expr) -> Stmt {
    Stmt::Expr(Expr::LocalSet(id, Box::new(v)))
}

#[test]
fn reassigning_a_parameter_keeps_nothing() {
    use perry_hir::LogicalOp::{Coalesce, Or};
    // a = a || {}; b = b ?? (c ? b : 0); return a.x + b.x
    let body = vec![
        assign(10, logical(Or, lg(10), Expr::Object(vec![]))),
        assign(
            11,
            logical(Coalesce, lg(11), cond(lg(12), lg(11), Expr::Number(0.0))),
        ),
        ret(add(get(lg(10), "x"), get(lg(11), "x"))),
    ];
    let f = fates(
        &[func(1, &[10, 11, 12], body)],
        &TestProof::reading(&[(1, "x")]),
    );
    for i in 0..3 {
        assert!(f.of_fn(1).unwrap().params[i].proven_dropped(), "param {i}");
    }
}

#[test]
fn assigning_one_parameter_into_another_keeps_it() {
    // a = b || a; kept.push(a): b flows into a's binding.
    let body = vec![
        assign(10, logical(perry_hir::LogicalOp::Or, lg(11), lg(10))),
        push(lg(10)),
    ];
    let f = fates(&[func(1, &[10, 11], body)], &TestProof::default());
    assert_eq!(why(&f, 1, 1), "use(Logical)");
    assert_eq!(fate(&f, 1, 0), Fate::Kept);
}

#[test]
fn a_never_written_copy_is_the_parameter() {
    // const q = a; const r = q; if (r) {} return q.x
    let body = vec![
        let_(20, lg(10)),
        let_(21, lg(20)),
        if_(lg(21), vec![]),
        ret(get(lg(20), "x")),
    ];
    let f = fates(&[func(1, &[10], body)], &TestProof::reading(&[(1, "x")]));
    assert!(f.of_fn(1).unwrap().params[0].proven_dropped());
}

#[test]
fn a_copy_that_keeps_keeps_the_parameter() {
    // const q = a; kept.push(q)
    let f1 = func(1, &[10], vec![let_(20, lg(10)), push(lg(20))]);
    assert_eq!(
        why(&fates(&[f1], &TestProof::default()), 1, 0),
        "arg-unresolved(PropertyGet.push)"
    );
    // const g = () => q; const q = a: the capture reads the copy.
    let mut c = closure(vec![ret(lg(20))], false);
    if let Expr::Closure { captures, .. } = &mut c {
        captures.push(20);
    }
    let f2 = func(1, &[10], vec![let_(30, c), let_(20, lg(10))]);
    assert_eq!(why(&fates(&[f2], &TestProof::default()), 1, 0), "captured");
    // let q = a; q = b; return q.x: q is written, so it is a plain local.
    let f3 = func(
        1,
        &[10, 11],
        vec![let_(20, lg(10)), assign(20, lg(11)), ret(get(lg(20), "x"))],
    );
    let f3 = fates(&[f3], &TestProof::reading(&[(1, "x")]));
    assert_eq!(why(&f3, 1, 0), "use(Let)");
    assert_eq!(why(&f3, 1, 1), "use(LocalSet)");
    // a copy inside a nested list counts too: if (c) { const q = a; kept.push(q) }
    let f4 = func(
        1,
        &[10, 11],
        vec![if_(lg(11), vec![let_(20, lg(10)), push(lg(20))])],
    );
    assert_eq!(
        why(&fates(&[f4], &TestProof::default()), 1, 0),
        "arg-unresolved(PropertyGet.push)"
    );
}

// ─── FATE2 rule d: a literal shape's constructor stores into the new object

fn shape_class(name: &str, extra: Vec<Stmt>) -> perry_hir::Class {
    let field = |n: &str| perry_hir::ClassField {
        origin: perry_hir::ClassFieldOrigin::Definition,
        name: n.to_string(),
        key_expr: None,
        ty: Type::Any,
        init: None,
        is_private: false,
        is_readonly: false,
        decorators: vec![],
    };
    let mut body = vec![
        Stmt::Expr(set(Expr::This, "a", lg(10))),
        Stmt::Expr(set(Expr::This, "b", lg(11))),
    ];
    body.extend(extra);
    perry_hir::Class {
        id: 1,
        name: name.to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![field("a"), field("b")],
        constructor: Some(func(3, &[10, 11], body)),
        methods: Vec::new(),
        getters: Vec::new(),
        setters: Vec::new(),
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
        computed_members: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        aliases: Vec::new(),
        is_nested: false,
        alloc_width_hint: 0,
        specialized_from: None,
    }
}

#[test]
fn a_literal_shape_constructor_keeps_by_receiver() {
    let c = shape_class("__AnonShape_t", vec![]);
    assert!(c.is_literal_shape());
    let f = Fates::compute(&[], &[&c], &TestProof::default());
    let ff = f.of_fn(3).unwrap();
    assert_eq!(ff.params[0].fate, Fate::KeptByReceiver);
    assert_eq!(ff.params[1].fate, Fate::KeptByReceiver);
    assert!(ff.this.proven_dropped());
    assert_eq!(
        f.construction_arg("__AnonShape_t", 1).fate,
        Fate::KeptByReceiver
    );
}

#[test]
fn a_class_that_is_no_literal_shape_needs_the_store_proof() {
    // A user class of the same body: the proof (here none) decides.
    let c = shape_class("P", vec![]);
    let f = Fates::compute(&[], &[&c], &TestProof::default());
    assert_eq!(why(&f, 3, 0), "use(PropertySet.value-to-this)");
    // An anon-named class whose constructor does more than its stores.
    let c = shape_class("__AnonShape_u", vec![push(lg(10))]);
    assert!(!c.is_literal_shape());
    let f = Fates::compute(&[], &[&c], &TestProof::default());
    assert_eq!(fate(&f, 3, 0), Fate::Kept);
}
