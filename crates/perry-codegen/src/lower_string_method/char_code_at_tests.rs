use perry_hir::{types::Type, CompareOp, Expr, Function, Module, Param, Stmt, UpdateOp};

fn scanner_ir(receiver_type: Type) -> String {
    let mut m = Module::new("char_scanner");
    m.functions.push(Function {
        id: 1,
        name: "scan".into(),
        type_params: vec![],
        params: vec![Param {
            id: 1,
            name: "s".into(),
            ty: receiver_type,
            default: None,
            decorators: vec![],
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Any,
        body: vec![Stmt::For {
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
                right: Box::new(Expr::Integer(20)),
            }),
            update: Some(Expr::Update {
                id: 2,
                op: UpdateOp::Increment,
                prefix: false,
            }),
            body: vec![Stmt::Expr(Expr::Call {
                callee: Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(1)),
                    property: "charCodeAt".into(),
                    byte_offset: 0,
                }),
                args: vec![Expr::LocalGet(2)],
                type_args: vec![],
                byte_offset: 0,
            })],
        }],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
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
    assert!(ir.contains("@js_string_index_to_i32"));
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
            ir.contains("icmp ult i32"),
            "heap read must remain bounds checked"
        );
        assert!(!ir.contains("STRING_CACHE") && !ir.contains("CHAR_CACHE"));
    }
}
