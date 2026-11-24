use perry_hir::{types::Type, CompareOp, Expr, Function, Module, Param, Stmt, UpdateOp};

fn scanner_ir(receiver_type: Type) -> String {
    scanner_ir_with(receiver_type, vec![])
}

/// The scanner with `prelude` before its loop.
fn scanner_ir_with(receiver_type: Type, prelude: Vec<Stmt>) -> String {
    let mut m = Module::new("char_scanner");
    m.functions.push(Function {
        byte_offset: 0,
        id: 1,
        name: "scan".into(),
        type_params: vec![],
        params: vec![Param {
            byte_offset: 0,
            id: 1,
            name: "s".into(),
            ty: receiver_type,
            default: None,
            decorators: vec![],
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Any,
        body: prelude
            .into_iter()
            .chain([Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 2,
                    name: "i".into(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    byte_offset: 0,
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(2)),
                    right: Box::new(Expr::Integer(20)),
                }),
                update: Some(Expr::Update {
                    id: 2,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::Expr(Expr::Call {
                    byte_end: 0,
                    callee: Box::new(Expr::PropertyGet {
                        byte_end: 0,
                        object: Box::new(Expr::LocalGet(1)),
                        property: "charCodeAt".into(),
                        byte_offset: 0,
                    }),
                    args: vec![Expr::LocalGet(2)],
                    type_args: vec![],
                    byte_offset: 0,
                })],
            }])
            .collect(),
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
        construct_only: false,
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

#[test]
fn untyped_scanner_uses_the_same_checked_character_load() {
    let ir = scanner_ir(Type::Any);
    assert!(
        ir.contains("cca.fast"),
        "tag-proven Any string did not use the existing emitter"
    );
    assert!(
        ir.contains("load i8"),
        "ASCII scanner has no direct payload load"
    );
    assert!(
        ir.contains("anystr.generic"),
        "non-string method dispatch must remain"
    );
    assert!(
        ir.contains("cca.slow"),
        "Unicode and coercing indices must keep the decoder"
    );
    assert!(ir.contains("@js_string_number_index_to_i32"));
    assert!(ir.contains("@js_string_char_code_at"));
    assert!(
        !ir.contains("\nmsite."),
        "guarded builtin miss duplicated an ordinary method cache"
    );
}

#[test]
fn declared_and_untyped_scanners_share_one_character_emitter() {
    for ty in [Type::String, Type::Any] {
        let ir = scanner_ir(ty);
        assert!(ir.contains("cca.hdr") && ir.contains("cca.sso_fast"));
        assert!(
            ir.contains("cca.oob"),
            "numeric misses must return NaN directly"
        );
        assert!(
            ir.contains("and i32") && ir.contains(", 4"),
            "equal header lengths require the producer's encoding proof"
        );
        assert!(
            ir.contains("icmp ult i32"),
            "heap read must remain bounds checked"
        );
        assert!(!ir.contains("STRING_CACHE") && !ir.contains("CHAR_CACHE"));
    }
}

#[test]
fn a_parameter_scan_reads_unicode_through_one_zeroed_cursor() {
    let ir = scanner_ir(Type::String);
    assert!(
        ir.contains("store [3 x i32] zeroinitializer"),
        "the cursor must read zero before its first use"
    );
    assert!(ir.contains("call i32 @js_string_utf16_unit_at_cursor"));
    assert!(
        ir.contains("cca.cursor_hit"),
        "a found unit must answer without the decoder"
    );
    assert!(
        ir.contains("cca.slow"),
        "a declined read keeps the complete charCodeAt"
    );
}

#[test]
fn a_rebound_parameter_has_no_cursor() {
    // `s = "x"` makes the parameter's slot hold a second string, which could
    // reuse the first one's offsets.
    let rebind = Stmt::Expr(Expr::LocalSet(1, Box::new(Expr::String("x".into()))));
    let ir = scanner_ir_with(Type::String, vec![rebind]);
    assert!(!ir.contains("call i32 @js_string_utf16_unit_at_cursor"));
    assert!(ir.contains("cca.slow"));
}
