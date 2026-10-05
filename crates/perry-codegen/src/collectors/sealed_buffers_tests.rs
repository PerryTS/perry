use super::*;
use perry_hir::types::Type;
use perry_hir::{Function, Param, TYPED_ARRAY_KIND_FLOAT64};

fn let_stmt(id: u32, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: format!("v{id}"),
        ty: Type::Any,
        mutable: false,
        init: Some(init),
    }
}

fn fresh(id: u32) -> Stmt {
    let_stmt(
        id,
        Expr::TypedArrayNew {
            kind: TYPED_ARRAY_KIND_FLOAT64,
            arg: Some(Box::new(Expr::Integer(8))),
        },
    )
}

fn call(fid: u32, args: Vec<Expr>) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::FuncRef(fid)),
        args,
        type_args: vec![],
        byte_offset: 0,
    }
}

fn get(recv: u32, index: Expr) -> Expr {
    Expr::IndexGet {
        object: Box::new(Expr::LocalGet(recv)),
        index: Box::new(index),
    }
}

fn prop(recv: u32, name: &str) -> Expr {
    Expr::PropertyGet {
        byte_offset: 0,
        object: Box::new(Expr::LocalGet(recv)),
        property: name.to_string(),
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

fn func(id: u32, params: Vec<Param>, body: Vec<Stmt>) -> Function {
    Function {
        id,
        name: format!("f{id}"),
        type_params: vec![],
        params,
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

fn module(init: Vec<Stmt>, functions: Vec<Function>) -> Module {
    let mut m = Module::new("sealed_test");
    m.init = init;
    m.functions = functions;
    m
}

#[test]
fn element_reads_length_and_a_sealed_callee_keep_the_seal() {
    // const a = new Float64Array(8); a[0]; a.length; f(a);  f(p) { p[1]; }
    let m = module(
        vec![
            fresh(10),
            Stmt::Expr(get(10, Expr::Integer(0))),
            Stmt::Expr(prop(10, "length")),
            Stmt::Expr(call(1, vec![Expr::LocalGet(10)])),
        ],
        vec![func(
            1,
            vec![param(20)],
            vec![Stmt::Expr(get(20, Expr::Integer(1)))],
        )],
    );
    let exposed = buffer_exposure(&m).exposed;
    assert!(!exposed.contains(&10));
    assert!(!exposed.contains(&20));
}

#[test]
fn reading_buffer_exposes_the_binding() {
    let m = module(vec![fresh(10), Stmt::Expr(prop(10, "buffer"))], vec![]);
    assert!(buffer_exposure(&m).exposed.contains(&10));
}

#[test]
fn a_callee_that_exposes_its_param_exposes_the_argument() {
    // f(a);  f(p) { g(p); }  g(q) { q.buffer; }
    let m = module(
        vec![fresh(10), Stmt::Expr(call(1, vec![Expr::LocalGet(10)]))],
        vec![
            func(
                1,
                vec![param(20)],
                vec![Stmt::Expr(call(2, vec![Expr::LocalGet(20)]))],
            ),
            func(2, vec![param(30)], vec![Stmt::Expr(prop(30, "buffer"))]),
        ],
    );
    let exposed = buffer_exposure(&m).exposed;
    assert!(exposed.contains(&30));
    assert!(exposed.contains(&20));
    assert!(exposed.contains(&10));
}

#[test]
fn an_unknown_callee_or_missing_param_exposes_the_argument() {
    // f(a) where f has no param at that position; h(b) where h is not a
    // module function.
    let m = module(
        vec![
            fresh(10),
            fresh(11),
            Stmt::Expr(call(1, vec![Expr::LocalGet(10)])),
            Stmt::Expr(call(99, vec![Expr::LocalGet(11)])),
        ],
        vec![func(1, vec![], vec![])],
    );
    let exposed = buffer_exposure(&m).exposed;
    assert!(exposed.contains(&10));
    assert!(exposed.contains(&11));
}

#[test]
fn a_key_that_may_be_a_string_exposes_the_receiver() {
    // const k = "buffer"; a[k];   const j = 3; b[j + 1];   c[o.name] (a shape of unknown type)
    let m = module(
        vec![
            fresh(10),
            fresh(11),
            fresh(14),
            Stmt::Expr(get(14, prop(13, "name"))),
            let_stmt(12, Expr::String("buffer".to_string())),
            let_stmt(13, Expr::Integer(3)),
            Stmt::Expr(get(10, Expr::LocalGet(12))),
            Stmt::Expr(get(
                11,
                Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(Expr::LocalGet(13)),
                    right: Box::new(Expr::Integer(1)),
                },
            )),
        ],
        vec![],
    );
    let exposed = buffer_exposure(&m).exposed;
    assert!(exposed.contains(&10));
    assert!(!exposed.contains(&11));
    assert!(exposed.contains(&14));
}

#[test]
fn a_parameter_key_is_never_a_string_only_if_every_call_passes_a_number() {
    // f(a, 2) and g(b, "buffer"):  f(p, i) { p[i]; }  g(q, k) { q[k]; }
    let m = module(
        vec![
            fresh(10),
            fresh(11),
            Stmt::Expr(call(1, vec![Expr::LocalGet(10), Expr::Integer(2)])),
            Stmt::Expr(call(
                2,
                vec![Expr::LocalGet(11), Expr::String("buffer".to_string())],
            )),
        ],
        vec![
            func(
                1,
                vec![param(20), param(21)],
                vec![Stmt::Expr(get(20, Expr::LocalGet(21)))],
            ),
            func(
                2,
                vec![param(30), param(31)],
                vec![Stmt::Expr(get(30, Expr::LocalGet(31)))],
            ),
        ],
    );
    let exposed = buffer_exposure(&m).exposed;
    assert!(!exposed.contains(&10));
    assert!(exposed.contains(&11));
}

#[test]
fn an_element_read_of_a_fresh_array_is_a_number_key() {
    // const k = b[0]; a[k];
    let m = module(
        vec![
            fresh(10),
            fresh(11),
            let_stmt(12, get(11, Expr::Integer(0))),
            Stmt::Expr(get(10, Expr::LocalGet(12))),
        ],
        vec![],
    );
    assert!(!buffer_exposure(&m).exposed.contains(&10));
}

#[test]
fn an_exported_or_reassigned_binding_is_exposed() {
    let mut m = module(
        vec![
            fresh(10),
            fresh(11),
            Stmt::Expr(Expr::LocalSet(11, Box::new(Expr::Integer(0)))),
        ],
        vec![],
    );
    m.exports.push(perry_hir::Export::Named {
        local: "v10".to_string(),
        exported: "v10".to_string(),
    });
    let exposed = buffer_exposure(&m).exposed;
    assert!(exposed.contains(&10));
    assert!(exposed.contains(&11));
}

#[test]
fn a_recursive_callee_stays_sealed() {
    // f(a);  f(p) { p[0]; f(p); }
    let m = module(
        vec![fresh(10), Stmt::Expr(call(1, vec![Expr::LocalGet(10)]))],
        vec![func(
            1,
            vec![param(20)],
            vec![
                Stmt::Expr(get(20, Expr::Integer(0))),
                Stmt::Expr(call(1, vec![Expr::LocalGet(20)])),
            ],
        )],
    );
    let exposed = buffer_exposure(&m).exposed;
    assert!(!exposed.contains(&10));
    assert!(!exposed.contains(&20));
}

#[test]
fn a_sloppy_element_store_keeps_the_seal() {
    // a[1] = 2 in sloppy code lowers to PutValueSet { target: a, receiver: a }.
    let put = |target: u32, receiver: u32| {
        Stmt::Expr(Expr::PutValueSet {
            target: Box::new(Expr::LocalGet(target)),
            key: Box::new(Expr::Integer(1)),
            value: Box::new(Expr::Integer(2)),
            receiver: Box::new(Expr::LocalGet(receiver)),
            strict: false,
        })
    };
    let m = module(
        vec![fresh(10), fresh(11), fresh(12), put(10, 10), put(11, 12)],
        vec![],
    );
    let exposed = buffer_exposure(&m).exposed;
    assert!(!exposed.contains(&10));
    assert!(exposed.contains(&11));
    assert!(exposed.contains(&12));
}

#[test]
fn an_alias_is_sealed_only_if_its_uses_are() {
    // const b = a; b[0];   const d = c; d.buffer;
    let m = module(
        vec![
            fresh(10),
            let_stmt(11, Expr::LocalGet(10)),
            Stmt::Expr(get(11, Expr::Integer(0))),
            fresh(12),
            let_stmt(13, Expr::LocalGet(12)),
            Stmt::Expr(prop(13, "buffer")),
        ],
        vec![],
    );
    let exposed = buffer_exposure(&m).exposed;
    assert!(!exposed.contains(&10));
    assert!(exposed.contains(&12));
}

#[test]
fn a_canonical_numeric_string_key_is_an_element_key() {
    let m = module(
        vec![
            fresh(10),
            fresh(11),
            Stmt::Expr(get(10, Expr::String("0".to_string()))),
            Stmt::Expr(get(11, Expr::String("01".to_string()))),
        ],
        vec![],
    );
    let exposed = buffer_exposure(&m).exposed;
    assert!(!exposed.contains(&10));
    assert!(exposed.contains(&11));
}

#[test]
fn an_exposure_in_a_closure_or_another_function_is_remote() {
    // const a = ...; a.buffer;           (own body: late)
    // const b = ...; () => b.buffer;     (closure: remote)
    let closure = Expr::Closure {
        func_id: 99,
        params: vec![],
        return_type: Type::Any,
        body: vec![Stmt::Expr(prop(11, "buffer"))],
        captures: vec![11],
        mutable_captures: vec![],
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: false,
    };
    let m = module(
        vec![
            fresh(10),
            Stmt::Expr(prop(10, "buffer")),
            fresh(11),
            Stmt::Expr(closure),
        ],
        vec![],
    );
    let exposure = buffer_exposure(&m);
    assert!(exposure.exposed.contains(&10) && !exposure.remote.contains(&10));
    assert!(exposure.exposed.contains(&11) && exposure.remote.contains(&11));
}

#[test]
fn the_statement_probe_flags_every_exposing_use() {
    let receiver = |key: Expr| Stmt::Expr(get(10, key));
    assert!(!stmt_may_expose(&receiver(Expr::Integer(0)), 10));
    assert!(!stmt_may_expose(&Stmt::Expr(prop(10, "length")), 10));
    // A local key needs module facts: the probe counts it as exposing.
    assert!(stmt_may_expose(&receiver(Expr::LocalGet(3)), 10));
    assert!(stmt_may_expose(&Stmt::Expr(prop(10, "buffer")), 10));
    assert!(stmt_may_expose(
        &Stmt::Expr(call(1, vec![Expr::LocalGet(10)])),
        10
    ));
    // Nested in a loop body.
    let looped = Stmt::While {
        condition: Expr::Bool(true),
        body: vec![receiver(Expr::Integer(1)), Stmt::Expr(prop(10, "buffer"))],
    };
    assert!(stmt_may_expose(&looped, 10));
    assert!(!stmt_may_expose(&looped, 11));
}
