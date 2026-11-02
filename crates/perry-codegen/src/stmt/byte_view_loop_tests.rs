use perry_hir::{types::Type, BinaryOp, CompareOp, Expr, Function, Module, Param, Stmt, UpdateOp};

/// `function inc(buf: Buffer) { for (let i = 0; i < buf.length; i++) buf[i] = (buf[i] + 1) & 255; }`
/// with `extra` appended to the loop body.
fn ir(extra: Vec<Stmt>) -> String {
    let mut m = Module::new("byte_view_loop");
    let element = || Expr::Uint8ArrayGet {
        array: Box::new(Expr::LocalGet(1)),
        index: Box::new(Expr::LocalGet(2)),
    };
    let mut body = vec![Stmt::Expr(Expr::Uint8ArraySet {
        array: Box::new(Expr::LocalGet(1)),
        index: Box::new(Expr::LocalGet(2)),
        value: Box::new(Expr::Binary {
            op: BinaryOp::BitAnd,
            left: Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(element()),
                right: Box::new(Expr::Integer(1)),
            }),
            right: Box::new(Expr::Integer(255)),
        }),
    })];
    body.extend(extra);
    m.functions.push(Function {
        id: 1,
        name: "inc".into(),
        type_params: vec![],
        params: vec![Param {
            id: 1,
            name: "buf".into(),
            ty: Type::Named("Buffer".into()),
            default: None,
            decorators: vec![],
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Number,
        body: vec![
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 2,
                    name: "i".into(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(2)),
                    right: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(1)),
                        property: "length".into(),
                        byte_offset: 0,
                        byte_end: 0,
                    }),
                }),
                update: Some(Expr::Update {
                    id: 2,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body,
            },
            Stmt::Return(Some(Expr::Integer(0))),
        ],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: true,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    });
    String::from_utf8(
        crate::compile_module(
            &m,
            crate::CompileOptions {
                emit_ir_only: true,
                ..Default::default()
            },
        )
        .unwrap(),
    )
    .unwrap()
}

/// The generic entry's text: the fast copy's blocks run from the merge
/// block to the original copy's first block.
fn copies(ir: &str) -> (&str, &str) {
    let start = ir
        .find("define double @perry_fn_byte_view_loop__inc(")
        .or_else(|| ir.find("@perry_fn_byte_view_loop__inc("))
        .expect("generic entry");
    let function = &ir[start..];
    let function = &function[..function.find("\n}\n").expect("function end")];
    let fast = function
        .find("\nfor.byte_view.merge")
        .expect("versioned loop");
    let slow = function
        .find("\nfor.byte_view_slow.")
        .expect("original copy");
    (&function[fast..slow], &function[slow..])
}

#[test]
fn admitted_byte_loop_has_no_runtime_arm() {
    let ir = ir(vec![]);
    let (fast, slow) = copies(&ir);
    for helper in ["@js_uint8array_get", "@js_uint8array_set", "u8c."] {
        assert!(!fast.contains(helper), "fast copy kept {helper}:\n{fast}");
    }
    assert!(
        fast.contains("load i8") && fast.contains("store i8"),
        "{fast}"
    );
    // The refused receiver keeps the runtime accessors.
    assert!(slow.contains("@js_uint8array_get") && slow.contains("@js_uint8array_set"));
    // One admission: the receiver proof and the named-length flag, before
    // the branch to the fast copy.
    let preheader = &ir[..ir.find("\nfor.byte_view.fast.preheader").unwrap()];
    assert!(preheader.contains("PERRY_TYPED_NAMED_PROPS_INVALIDATED"));
}

#[test]
fn a_call_in_the_body_keeps_the_ordinary_loop() {
    let ir = ir(vec![Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::LocalGet(1)),
        args: vec![],
        type_args: vec![],
        byte_offset: 0,
        byte_end: 0,
    })]);
    assert!(
        !ir.contains("for.byte_view"),
        "a call can detach the receiver"
    );
}

#[test]
fn a_non_counter_key_keeps_the_ordinary_loop() {
    let ir = ir(vec![Stmt::Expr(Expr::Uint8ArraySet {
        array: Box::new(Expr::LocalGet(1)),
        index: Box::new(Expr::Integer(0)),
        value: Box::new(Expr::Integer(1)),
    })]);
    assert!(
        !ir.contains("for.byte_view"),
        "only `P[i]` is bounded by the guard"
    );
}
