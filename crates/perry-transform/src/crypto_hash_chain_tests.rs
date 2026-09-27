use super::*;
use perry_hir::types::Type;

fn function(body: Vec<Stmt>, is_async: bool) -> Function {
    Function {
        id: 1,
        name: "f".into(),
        type_params: vec![],
        params: vec![Param {
            id: 1,
            name: "x".into(),
            ty: Type::Any,
            default: None,
            decorators: vec![],
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Any,
        body,
        is_async,
        is_generator: false,
        is_strict: true,
        is_exported: true,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    }
}

fn crypto() -> Expr {
    Expr::NativeModuleRef("crypto".into())
}

fn call(recv: Expr, method: &str, args: Vec<Expr>) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(recv),
            property: method.into(),
            byte_offset: 0,
        }),
        args,
        type_args: vec![],
        byte_offset: 0,
    }
}

fn s(v: &str) -> Expr {
    Expr::String(v.into())
}

fn x() -> Expr {
    Expr::LocalGet(1)
}

fn create_hash(alg: &str) -> Expr {
    call(crypto(), "createHash", vec![s(alg)])
}

fn create_hmac(alg: &str) -> Expr {
    call(crypto(), "createHmac", vec![s(alg), s("key")])
}

fn let_h(id: u32, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: "h".into(),
        ty: Type::Named("Hash".into()),
        mutable: false,
        init: Some(init),
    }
}

fn h(id: u32) -> Expr {
    Expr::LocalGet(id)
}

fn run_fn(body: Vec<Stmt>) -> Vec<Stmt> {
    run_fn_async(body, false)
}

fn run_fn_async(body: Vec<Stmt>, is_async: bool) -> Vec<Stmt> {
    let mut module = Module::new("m");
    module.functions.push(function(body, is_async));
    run(&mut module);
    module.functions.remove(0).body
}

fn dbg(body: &[Stmt]) -> String {
    format!("{body:?}")
}

fn assert_handle_free(body: &[Stmt]) {
    let d = dbg(body);
    assert!(
        d.contains(CHAIN_DIGEST),
        "expected the handle-free chain: {d}"
    );
    assert!(
        !d.contains("\"createHash\"") && !d.contains("\"createHmac\""),
        "no handle-creating call may remain: {d}"
    );
}

fn assert_unchanged(body: Vec<Stmt>) {
    let before = dbg(&body);
    let after = run_fn(body);
    assert_eq!(
        dbg(&after),
        before,
        "escaping object must keep the handle path"
    );
}

#[test]
fn inline_chain_with_several_updates_lowers_to_one_frame_state() {
    // crypto.createHash("sha1").update(x).update("y", "utf8").digest("base64")
    let chain = call(
        call(
            call(create_hash("sha1"), "update", vec![x()]),
            "update",
            vec![s("y"), s("utf8")],
        ),
        "digest",
        vec![s("base64")],
    );
    let out = run_fn(vec![Stmt::Return(Some(chain))]);
    assert_handle_free(&out);
    let expected = call(
        crypto(),
        CHAIN_DIGEST,
        vec![
            call(
                crypto(),
                CHAIN_UPDATE,
                vec![
                    call(
                        crypto(),
                        CHAIN_UPDATE,
                        vec![call(crypto(), CHAIN_INIT_HASH, vec![s("sha1")]), x()],
                    ),
                    s("y"),
                    s("utf8"),
                ],
            ),
            s("base64"),
        ],
    );
    assert_eq!(dbg(&out), dbg(&[Stmt::Return(Some(expected))]));
}

#[test]
fn inline_hmac_chain_with_no_digest_encoding_is_rewritten() {
    let chain = call(
        call(create_hmac("sha512"), "update", vec![x()]),
        "digest",
        vec![],
    );
    let out = run_fn(vec![Stmt::Return(Some(chain))]);
    assert_handle_free(&out);
    assert!(dbg(&out).contains(CHAIN_INIT_HMAC));
}

#[test]
fn literal_sha256_fast_path_is_left_to_its_direct_helper() {
    let chain = call(
        call(create_hash("sha256"), "update", vec![s("abc")]),
        "digest",
        vec![s("hex")],
    );
    assert_unchanged(vec![Stmt::Return(Some(chain))]);
}

#[test]
fn inline_chain_awaiting_in_an_argument_keeps_the_handle() {
    let chain = call(
        call(
            create_hash("sha1"),
            "update",
            vec![Expr::Await(Box::new(x()))],
        ),
        "digest",
        vec![s("hex")],
    );
    let body = vec![Stmt::Return(Some(chain))];
    let before = dbg(&body);
    assert_eq!(dbg(&run_fn_async(body, true)), before);
}

#[test]
fn block_local_hash_used_only_by_update_and_digest_is_rewritten() {
    // const h = crypto.createHash("sha1"); h.update(x); return h.digest("hex");
    let body = vec![
        let_h(2, create_hash("sha1")),
        Stmt::Expr(call(h(2), "update", vec![x()])),
        Stmt::Return(Some(call(h(2), "digest", vec![s("hex")]))),
    ];
    let out = run_fn(body);
    assert_handle_free(&out);
    let d = dbg(&out);
    // The binding's declared `Hash` type is dropped: it holds a frame address.
    assert!(d.contains("ty: Any"), "{d}");
    assert!(
        !d.contains("\"update\"") && !d.contains("\"digest\""),
        "{d}"
    );
}

#[test]
fn block_local_in_a_loop_body_with_chained_updates_is_rewritten() {
    // for (;;) { const h = createHash("md5"); h.update(x).update(x); s = h.digest(); }
    let body = vec![Stmt::While {
        condition: Expr::Bool(true),
        body: vec![
            let_h(2, create_hash("md5")),
            Stmt::Expr(call(call(h(2), "update", vec![x()]), "update", vec![x()])),
            Stmt::Expr(Expr::LocalSet(1, Box::new(call(h(2), "digest", vec![])))),
            Stmt::Break,
        ],
    }];
    assert_handle_free(&run_fn(body));
}

#[test]
fn jwa_sequence_shape_is_rewritten() {
    // var hmac = createHmac(alg, key); var sig = (hmac.update(x), hmac.digest('base64'));
    let body = vec![
        let_h(2, create_hmac("sha256")),
        Stmt::Let {
            id: 3,
            name: "sig".into(),
            ty: Type::Any,
            mutable: true,
            init: Some(Expr::Sequence(vec![
                call(h(2), "update", vec![x()]),
                call(h(2), "digest", vec![s("base64")]),
            ])),
        },
        Stmt::Return(Some(Expr::LocalGet(3))),
    ];
    assert_handle_free(&run_fn(body));
}

#[test]
fn update_in_a_nested_branch_before_digest_is_rewritten() {
    let body = vec![
        let_h(2, create_hash("sha1")),
        Stmt::If {
            condition: x(),
            then_branch: vec![Stmt::Expr(call(h(2), "update", vec![x()]))],
            else_branch: None,
        },
        Stmt::Return(Some(call(h(2), "digest", vec![s("hex")]))),
    ];
    assert_handle_free(&run_fn(body));
}

#[test]
fn hash_passed_to_a_function_escapes() {
    let body = vec![
        let_h(2, create_hash("sha1")),
        Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::LocalGet(1)),
            args: vec![h(2)],
            type_args: vec![],
            byte_offset: 0,
        }),
        Stmt::Return(Some(call(h(2), "digest", vec![s("hex")]))),
    ];
    assert_unchanged(body);
}

#[test]
fn stored_hash_escapes() {
    let body = vec![
        let_h(2, create_hash("sha1")),
        Stmt::Expr(Expr::LocalSet(1, Box::new(h(2)))),
        Stmt::Return(Some(call(h(2), "digest", vec![s("hex")]))),
    ];
    assert_unchanged(body);
}

#[test]
fn returned_hash_escapes() {
    let body = vec![
        let_h(2, create_hash("sha1")),
        Stmt::Expr(call(h(2), "update", vec![x()])),
        Stmt::Return(Some(h(2))),
    ];
    assert_unchanged(body);
}

#[test]
fn copy_escapes() {
    let body = vec![
        let_h(2, create_hash("sha1")),
        Stmt::Let {
            id: 3,
            name: "c".into(),
            ty: Type::Any,
            mutable: false,
            init: Some(call(h(2), "copy", vec![])),
        },
        Stmt::Return(Some(call(h(2), "digest", vec![s("hex")]))),
    ];
    assert_unchanged(body);
}

#[test]
fn stream_use_escapes() {
    let body = vec![
        let_h(2, create_hash("sha1")),
        Stmt::Expr(call(h(2), "write", vec![x()])),
        Stmt::Expr(call(h(2), "end", vec![])),
        Stmt::Return(Some(call(h(2), "digest", vec![s("hex")]))),
    ];
    assert_unchanged(body);
}

#[test]
fn update_result_used_as_a_value_escapes() {
    // const r = h.update(x) aliases the object.
    let body = vec![
        let_h(2, create_hash("sha1")),
        Stmt::Let {
            id: 3,
            name: "r".into(),
            ty: Type::Any,
            mutable: false,
            init: Some(call(h(2), "update", vec![x()])),
        },
        Stmt::Return(Some(call(h(2), "digest", vec![s("hex")]))),
    ];
    assert_unchanged(body);
}

#[test]
fn hash_captured_by_a_closure_escapes() {
    let closure = Expr::Closure {
        func_id: 9,
        params: vec![],
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(call(h(2), "digest", vec![s("hex")])))],
        captures: vec![2],
        mutable_captures: vec![],
        captures_this: false,
        captures_new_target: false,
        is_strict: true,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
    };
    let body = vec![
        let_h(2, create_hash("sha1")),
        Stmt::Expr(call(h(2), "update", vec![x()])),
        Stmt::Return(Some(closure)),
    ];
    assert_unchanged(body);
}

#[test]
fn await_between_declaration_and_digest_keeps_the_handle() {
    let body = vec![
        let_h(2, create_hash("sha1")),
        Stmt::Expr(Expr::Await(Box::new(x()))),
        Stmt::Return(Some(call(h(2), "digest", vec![s("hex")]))),
    ];
    let before = dbg(&body);
    assert_eq!(dbg(&run_fn_async(body, true)), before);
}

#[test]
fn reference_from_another_function_escapes() {
    // A second function naming the same LocalId (a hoisted inner function
    // capturing it) must block the rewrite even though it is not in `f`.
    let mut module = Module::new("m");
    module.functions.push(function(
        vec![
            let_h(2, create_hash("sha1")),
            Stmt::Return(Some(call(h(2), "digest", vec![s("hex")]))),
        ],
        false,
    ));
    let mut other = function(vec![Stmt::Return(Some(h(2)))], false);
    other.id = 2;
    module.functions.push(other);
    let before = dbg(&module.functions[0].body);
    run(&mut module);
    assert_eq!(dbg(&module.functions[0].body), before);
}
