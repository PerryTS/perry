//! An argument is substituted for its parameter only when nothing the
//! inlined body does can change what it reads (`WriteCensus::is_stable_arg`).
//! JS evaluates `g(m)`'s argument before g's body runs; a substituted `m`
//! would be read after the body's `bump()` wrote it.
//!
//! A read of a written local is still substituted when every read of the
//! parameter comes before anything in the body, or in a later argument,
//! that could run code (`WriteCensus::substitutes_arg`, `arg_read_order`).
use super::call_inliner::{inline_calls_in_stmts, try_inline_call, try_inline_simple_call};
use super::*;
use perry_hir::{BinaryOp, CompareOp, Module, Param, UpdateOp};

const M: LocalId = 1;
const A: LocalId = 10;
const BUMP: FuncId = 2;
const G: FuncId = 3;

fn param(id: LocalId) -> Param {
    Param {
        id,
        name: format!("p{id}"),
        ty: Type::Number,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

fn function(id: FuncId, params: Vec<Param>, body: Vec<Stmt>) -> Function {
    Function {
        id,
        name: format!("f{id}"),
        type_params: Vec::new(),
        params,
        return_type: Type::Number,
        body,
        is_async: false,
        is_generator: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
        is_strict: false,
    }
}

fn call(callee: Expr, args: Vec<Expr>) -> Expr {
    Expr::Call {
        byte_end: 0,
        callee: Box::new(callee),
        args,
        type_args: Vec::new(),
        byte_offset: 0,
    }
}

fn bump_call() -> Expr {
    call(Expr::FuncRef(BUMP), Vec::new())
}

/// The census of a module in which `bump` writes `M` when `written`.
fn census(written: bool) -> WriteCensus {
    census_storing(written.then_some(Expr::Integer(2)))
}

/// The census of a module in which `bump` stores `stored` into `M`. `M` only
/// ever holds a primitive unless `stored` is a read of some other local.
fn census_storing(stored: Option<Expr>) -> WriteCensus {
    let mut module = Module::new("test");
    let body = match stored {
        Some(value) => vec![Stmt::Expr(Expr::LocalSet(M, Box::new(value)))],
        None => vec![Stmt::Expr(Expr::LocalGet(M))],
    };
    module.functions.push(function(BUMP, Vec::new(), body));
    module.init.push(Stmt::Let {
        id: M,
        name: "m".into(),
        ty: Type::Number,
        mutable: true,
        init: Some(Expr::Integer(1)),
    });
    WriteCensus::of(&module)
}

/// `g(a) { return bump() + a }`
fn g_expr_body() -> HashMap<FuncId, Function> {
    let body = Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(bump_call()),
        right: Box::new(Expr::LocalGet(A)),
    };
    HashMap::from([(
        G,
        function(G, vec![param(A)], vec![Stmt::Return(Some(body))]),
    )])
}

fn inline_expr(arg: Expr, census: &WriteCensus) -> (Vec<Stmt>, Expr) {
    try_inline_simple_call(
        &call(Expr::FuncRef(G), vec![arg]),
        &g_expr_body(),
        &HashMap::new(),
        &HashMap::new(),
        &ExactReceiverFacts::new(),
        &mut 1000,
        census,
        None,
        &HashMap::new(),
        false,
    )
    .expect("g inlines")
}

/// The argument is read once, by a `let` before the body: `[let p = arg]`.
fn assert_bound_first(setup: &[Stmt], arg: &Expr) {
    assert!(
        matches!(setup.first(), Some(Stmt::Let { init: Some(init), .. }) if format!("{init:?}") == format!("{arg:?}")),
        "the argument must be bound before the body: {setup:?}"
    );
}

#[test]
fn a_written_local_argument_is_read_before_the_body() {
    let (setup, result) = inline_expr(Expr::LocalGet(M), &census(true));
    assert_bound_first(&setup, &Expr::LocalGet(M));
    assert!(!format!("{result:?}").contains("LocalGet(1)"), "{result:?}");
}

#[test]
fn an_unwritten_local_argument_is_substituted() {
    let (setup, result) = inline_expr(Expr::LocalGet(M), &census(false));
    assert!(setup.is_empty(), "{setup:?}");
    assert!(format!("{result:?}").contains("LocalGet(1)"), "{result:?}");
}

#[test]
fn a_global_argument_is_read_before_the_body() {
    let (setup, _) = inline_expr(Expr::GlobalGet(5), &census(false));
    assert_bound_first(&setup, &Expr::GlobalGet(5));
}

#[test]
fn a_local_minted_after_the_census_is_read_before_the_body() {
    let (setup, _) = inline_expr(Expr::LocalGet(5000), &census(false));
    assert_bound_first(&setup, &Expr::LocalGet(5000));
}

/// `f(a) { bump(); return a }` through the statement-level path.
#[test]
fn the_statement_path_reads_a_written_argument_before_the_body() {
    let body = vec![
        Stmt::Expr(bump_call()),
        Stmt::Return(Some(Expr::LocalGet(A))),
    ];
    let funcs = HashMap::from([(G, function(G, vec![param(A)], body))]);
    let (setup, _) = try_inline_call(
        &call(Expr::FuncRef(G), vec![Expr::LocalGet(M)]),
        &funcs,
        &HashMap::new(),
        &HashMap::new(),
        &ExactReceiverFacts::new(),
        &mut 1000,
        &census(true),
        None,
        &HashMap::new(),
        false,
    )
    .expect("f inlines");
    assert_bound_first(&setup, &Expr::LocalGet(M));
}

/// `obj.add(m)` with `add(a) { bump(); this.s = a }`.
#[test]
fn the_method_path_reads_a_written_argument_before_the_body() {
    let body = vec![
        Stmt::Expr(bump_call()),
        Stmt::Expr(Expr::PropertySet {
            object: Box::new(Expr::This),
            property: "s".into(),
            value: Box::new(Expr::LocalGet(A)),
            byte_offset: 0,
            byte_end: 0,
        }),
    ];
    let methods = HashMap::from([(
        ("Acc".to_string(), "add".to_string()),
        MethodCandidate {
            func: function(4, vec![param(A)], body),
            this_param_id: None,
            method_lookup_safe: true,
            required_extern_imports: Vec::new(),
        },
    )]);
    let obj: LocalId = 7;
    let facts = ExactReceiverFacts::from([(
        obj,
        ExactReceiverFact {
            class_name: "Acc".into(),
        },
    )]);
    let callee = Expr::PropertyGet {
        byte_end: 0,
        object: Box::new(Expr::LocalGet(obj)),
        property: "add".into(),
        byte_offset: 0,
    };
    let inline = |census: &WriteCensus| {
        try_inline_call(
            &call(callee.clone(), vec![Expr::LocalGet(M)]),
            &HashMap::new(),
            &methods,
            &HashMap::new(),
            &facts,
            &mut 1000,
            census,
            None,
            &HashMap::new(),
            false,
        )
        .expect("add inlines")
        .0
    };
    assert_bound_first(&inline(&census(true)), &Expr::LocalGet(M));
    let twin = inline(&census(false));
    assert!(
        !twin.iter().any(|s| matches!(
            s,
            Stmt::Let {
                init: Some(Expr::LocalGet(M)),
                ..
            }
        )),
        "an unwritten argument is substituted: {twin:?}"
    );
}

const B: LocalId = 11;
const ON_BOARD: FuncId = 5;

fn binary(op: BinaryOp, left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op,
        left: Box::new(left),
        right: Box::new(right),
    }
}

/// `onBoard(a) { return (a & 0x88) === 0 }`: `a` is read before `&` converts.
fn on_board() -> Function {
    let body = Expr::Compare {
        op: CompareOp::Eq,
        left: Box::new(binary(
            BinaryOp::BitAnd,
            Expr::LocalGet(A),
            Expr::Integer(0x88),
        )),
        right: Box::new(Expr::Integer(0)),
    };
    function(ON_BOARD, vec![param(A)], vec![Stmt::Return(Some(body))])
}

/// Inline `callee(args)` through the expression path, in a module where `M`
/// is written and may hold an object.
fn inline_simple(callee: Function, args: Vec<Expr>) -> (Vec<Stmt>, Expr) {
    inline_simple_in(callee, args, &census_storing(Some(Expr::LocalGet(99))))
}

fn inline_simple_in(callee: Function, args: Vec<Expr>, census: &WriteCensus) -> (Vec<Stmt>, Expr) {
    let id = callee.id;
    try_inline_simple_call(
        &call(Expr::FuncRef(id), args),
        &HashMap::from([(id, callee)]),
        &HashMap::new(),
        &HashMap::new(),
        &ExactReceiverFacts::new(),
        &mut 1000,
        census,
        None,
        &HashMap::new(),
        false,
    )
    .expect("the callee inlines")
}

fn reads_m(e: &Expr) -> bool {
    format!("{e:?}").contains("LocalGet(1)")
}

#[test]
fn a_written_local_is_substituted_into_a_body_that_reads_it_first() {
    let (setup, result) = inline_simple(on_board(), vec![Expr::LocalGet(M)]);
    assert!(setup.is_empty(), "{setup:?}");
    assert!(reads_m(&result), "{result:?}");
}

#[test]
fn a_local_minted_after_the_census_is_substituted_into_such_a_body() {
    let (setup, result) = inline_simple(on_board(), vec![Expr::LocalGet(5000)]);
    assert!(setup.is_empty(), "{setup:?}");
    assert!(
        format!("{result:?}").contains("LocalGet(5000)"),
        "{result:?}"
    );
}

/// `inRange(a) { return a >= 0 && a < 128 && (a & 0x88) === 0 }`: `>=` would
/// convert an object before the second read.
fn in_range() -> Function {
    let cmp = |op: CompareOp, right: i64| Expr::Compare {
        op,
        left: Box::new(Expr::LocalGet(A)),
        right: Box::new(Expr::Integer(right)),
    };
    let and = |left: Expr, right: Expr| Expr::Logical {
        op: perry_hir::LogicalOp::And,
        left: Box::new(left),
        right: Box::new(right),
    };
    let Stmt::Return(Some(masked)) = on_board().body.remove(0) else {
        unreachable!()
    };
    let body = and(and(cmp(CompareOp::Ge, 0), cmp(CompareOp::Lt, 128)), masked);
    function(ON_BOARD, vec![param(A)], vec![Stmt::Return(Some(body))])
}

/// A local that only ever holds primitives converts without running code,
/// so reads after an operator over it are still in time.
#[test]
fn reads_after_an_operator_over_a_primitive_local_are_substituted() {
    // `m = 1` at its `let`, `m = 2` in `bump`: only primitives.
    let (setup, result) = inline_simple_in(in_range(), vec![Expr::LocalGet(M)], &census(true));
    assert!(setup.is_empty(), "{setup:?}");
    assert!(reads_m(&result), "{result:?}");
    // `m = m + 1` is primitive whatever `m` held.
    let sum = binary(BinaryOp::Add, Expr::LocalGet(M), Expr::Integer(1));
    let (setup, _) = inline_simple_in(
        in_range(),
        vec![Expr::LocalGet(M)],
        &census_storing(Some(sum)),
    );
    assert!(setup.is_empty(), "{setup:?}");
    // Control: `m = other` may store an object.
    let (setup, result) = inline_simple(in_range(), vec![Expr::LocalGet(M)]);
    assert_bound_first(&setup, &Expr::LocalGet(M));
    assert!(!reads_m(&result), "{result:?}");
    // Control: a local the census never saw declared (a parameter).
    let (setup, _) = inline_simple_in(in_range(), vec![Expr::LocalGet(77)], &census(true));
    assert_bound_first(&setup, &Expr::LocalGet(77));
}

/// `while (inRange(m)) { m = m + 1 }`: a loop condition has no place for a
/// `let`, so the call is inlined only when the argument is substituted.
#[test]
fn a_loop_condition_over_a_written_local_is_inlined() {
    let mut stmts = vec![Stmt::While {
        condition: call(Expr::FuncRef(ON_BOARD), vec![Expr::LocalGet(M)]),
        body: vec![Stmt::Expr(Expr::LocalSet(
            M,
            Box::new(binary(BinaryOp::Add, Expr::LocalGet(M), Expr::Integer(1))),
        ))],
    }];
    inline_calls_in_stmts(
        &mut stmts,
        &HashMap::from([(ON_BOARD, in_range())]),
        &HashMap::new(),
        &HashMap::new(),
        &mut HashMap::new(),
        &mut ExactReceiverFacts::new(),
        &mut 1000,
        &census(true),
        None,
        &HashMap::new(),
        false,
    );
    let [Stmt::While { condition, .. }] = stmts.as_slice() else {
        panic!("the loop stays one statement: {stmts:?}");
    };
    assert!(!format!("{condition:?}").contains("Call"), "{condition:?}");
    assert!(reads_m(condition), "{condition:?}");
}

/// `sq(a) { return a * a }`: both reads come before `*` converts.
#[test]
fn two_reads_before_the_operator_are_substituted() {
    let body = binary(BinaryOp::Mul, Expr::LocalGet(A), Expr::LocalGet(A));
    let f = function(G, vec![param(A)], vec![Stmt::Return(Some(body))]);
    let (setup, result) = inline_simple(f, vec![Expr::LocalGet(M)]);
    assert!(setup.is_empty(), "{setup:?}");
    assert!(reads_m(&result), "{result:?}");
}

/// `f(a) { return (a & 1) + a }`: `&` may run `valueOf` before the second read.
#[test]
fn a_read_after_a_conversion_is_bound_first() {
    let body = binary(
        BinaryOp::Add,
        binary(BinaryOp::BitAnd, Expr::LocalGet(A), Expr::Integer(1)),
        Expr::LocalGet(A),
    );
    let f = function(G, vec![param(A)], vec![Stmt::Return(Some(body))]);
    let (setup, result) = inline_simple(f, vec![Expr::LocalGet(M)]);
    assert_bound_first(&setup, &Expr::LocalGet(M));
    assert!(!reads_m(&result), "{result:?}");
}

/// `f(a) { return a.x + a }`: the property read may run a getter.
#[test]
fn a_read_after_a_property_read_is_bound_first() {
    let body = binary(
        BinaryOp::Add,
        Expr::PropertyGet {
            byte_end: 0,
            object: Box::new(Expr::LocalGet(A)),
            property: "x".into(),
            byte_offset: 0,
        },
        Expr::LocalGet(A),
    );
    let f = function(G, vec![param(A)], vec![Stmt::Return(Some(body))]);
    let (setup, result) = inline_simple(f, vec![Expr::LocalGet(M)]);
    assert_bound_first(&setup, &Expr::LocalGet(M));
    assert!(!reads_m(&result), "{result:?}");
}

/// `f(a) { return (m = 5) === a }` with `m` the module local passed for `a`.
#[test]
fn a_read_after_a_store_to_the_argument_local_is_bound_first() {
    let body = Expr::Compare {
        op: CompareOp::Eq,
        left: Box::new(Expr::LocalSet(M, Box::new(Expr::Integer(5)))),
        right: Box::new(Expr::LocalGet(A)),
    };
    let f = function(G, vec![param(A)], vec![Stmt::Return(Some(body))]);
    let (setup, _) = inline_simple(f, vec![Expr::LocalGet(M)]);
    assert_bound_first(&setup, &Expr::LocalGet(M));
}

/// `f(a) { return () => a }`: the closure reads `a` whenever it is called.
#[test]
fn a_read_inside_a_closure_is_not_scanned_as_early() {
    let closure = Expr::Closure {
        func_id: 77,
        params: Vec::new(),
        return_type: Type::Number,
        body: vec![Stmt::Return(Some(Expr::LocalGet(A)))],
        captures: vec![A],
        mutable_captures: Vec::new(),
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: false,
    };
    let body = vec![Stmt::Return(Some(closure))];
    assert!(!crate::arg_read_order::reads_precede_effects(
        &body,
        A,
        M,
        Default::default()
    ));
}

/// `add2(m, m++)`: the later argument writes the earlier one.
#[test]
fn a_later_argument_that_writes_the_local_keeps_the_earlier_one_bound() {
    let body = binary(BinaryOp::Add, Expr::LocalGet(A), Expr::LocalGet(B));
    let f = function(G, vec![param(A), param(B)], vec![Stmt::Return(Some(body))]);
    let bump_m = Expr::Update {
        id: M,
        op: UpdateOp::Increment,
        prefix: false,
    };
    let (setup, result) = inline_simple(f.clone(), vec![Expr::LocalGet(M), bump_m]);
    assert_bound_first(&setup, &Expr::LocalGet(M));
    assert!(!reads_m(&result), "{result:?}");
    // A later argument that calls anything, too.
    let (setup, _) = inline_simple(f.clone(), vec![Expr::LocalGet(M), bump_call()]);
    assert_bound_first(&setup, &Expr::LocalGet(M));
    // Twin: a later argument that is only a read.
    let (setup, result) = inline_simple(f, vec![Expr::LocalGet(M), Expr::LocalGet(M)]);
    assert!(setup.is_empty(), "{setup:?}");
    assert!(reads_m(&result), "{result:?}");
}

/// `f(a) { let t = a; while (t === a) { t = a.x } return t }`: the loop runs
/// code, so its reads of `a` are late on the second trip.
#[test]
fn a_read_in_a_loop_that_runs_code_is_late() {
    let t: LocalId = 12;
    let cond = Expr::Compare {
        op: CompareOp::Eq,
        left: Box::new(Expr::LocalGet(t)),
        right: Box::new(Expr::LocalGet(A)),
    };
    let lets = Stmt::Let {
        id: t,
        name: "t".into(),
        ty: Type::Number,
        mutable: true,
        init: Some(Expr::LocalGet(A)),
    };
    let ret = Stmt::Return(Some(Expr::LocalGet(t)));
    let looped = |step: Expr| {
        vec![
            lets.clone(),
            Stmt::While {
                condition: cond.clone(),
                body: vec![Stmt::Expr(Expr::LocalSet(t, Box::new(step)))],
            },
            ret.clone(),
        ]
    };
    let getter = Expr::PropertyGet {
        byte_end: 0,
        object: Box::new(Expr::LocalGet(t)),
        property: "x".into(),
        byte_offset: 0,
    };
    assert!(!crate::arg_read_order::reads_precede_effects(
        &looped(getter),
        A,
        M,
        Default::default()
    ));
    // Twin: a loop that runs nothing.
    assert!(crate::arg_read_order::reads_precede_effects(
        &looped(Expr::Integer(0)),
        A,
        M,
        Default::default()
    ));
}

/// `f(a) { return a === 0 }` through the statement path, and the method
/// path with `is(a) { return a === this.k }` (`this.k` is read after `a`).
#[test]
fn the_statement_and_method_paths_substitute_an_early_read() {
    let eq = |right: Expr| Expr::Compare {
        op: CompareOp::Eq,
        left: Box::new(Expr::LocalGet(A)),
        right: Box::new(right),
    };
    let f = function(
        G,
        vec![param(A)],
        vec![Stmt::Return(Some(eq(Expr::Integer(0))))],
    );
    let (setup, result) = try_inline_call(
        &call(Expr::FuncRef(G), vec![Expr::LocalGet(M)]),
        &HashMap::from([(G, f)]),
        &HashMap::new(),
        &HashMap::new(),
        &ExactReceiverFacts::new(),
        &mut 1000,
        &census(true),
        None,
        &HashMap::new(),
        false,
    )
    .expect("f inlines");
    let all = format!("{setup:?} {result:?}");
    assert!(!all.contains("init: Some(LocalGet(1))"), "{all}");
    assert!(all.contains("LocalGet(1)"), "{all}");

    let this_k = Expr::PropertyGet {
        byte_end: 0,
        object: Box::new(Expr::This),
        property: "k".into(),
        byte_offset: 0,
    };
    let method = |body: Expr| {
        HashMap::from([(
            ("Acc".to_string(), "is".to_string()),
            MethodCandidate {
                func: function(4, vec![param(A)], vec![Stmt::Return(Some(body))]),
                this_param_id: None,
                method_lookup_safe: true,
                required_extern_imports: Vec::new(),
            },
        )])
    };
    let obj: LocalId = 7;
    let facts = ExactReceiverFacts::from([(
        obj,
        ExactReceiverFact {
            class_name: "Acc".into(),
        },
    )]);
    let callee = Expr::PropertyGet {
        byte_end: 0,
        object: Box::new(Expr::LocalGet(obj)),
        property: "is".into(),
        byte_offset: 0,
    };
    let inline = |body: Expr| {
        try_inline_simple_call(
            &call(callee.clone(), vec![Expr::LocalGet(M)]),
            &HashMap::new(),
            &method(body),
            &HashMap::new(),
            &facts,
            &mut 1000,
            &census(true),
            None,
            &HashMap::new(),
            false,
        )
        .expect("is inlines")
    };
    let (setup, result) = inline(eq(this_k.clone()));
    assert!(setup.is_empty(), "{setup:?}");
    assert!(reads_m(&result), "{result:?}");
    // Control: `this.k === a` reads the field (a getter may run) first.
    let late = Expr::Compare {
        op: CompareOp::Eq,
        left: Box::new(this_k),
        right: Box::new(Expr::LocalGet(A)),
    };
    let (setup, result) = inline(late);
    assert_bound_first(&setup, &Expr::LocalGet(M));
    assert!(!reads_m(&result), "{result:?}");
}
