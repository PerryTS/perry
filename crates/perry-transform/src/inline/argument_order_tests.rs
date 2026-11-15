//! An argument is substituted for its parameter only when nothing the
//! inlined body does can change what it reads (`WriteCensus::is_stable_arg`).
//! JS evaluates `g(m)`'s argument before g's body runs; a substituted `m`
//! would be read after the body's `bump()` wrote it.
use super::call_inliner::{try_inline_call, try_inline_simple_call};
use super::*;
use perry_hir::{BinaryOp, Module, Param};

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
    let mut module = Module::new("test");
    let body = if written {
        vec![Stmt::Expr(Expr::LocalSet(M, Box::new(Expr::Integer(2))))]
    } else {
        vec![Stmt::Expr(Expr::LocalGet(M))]
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
