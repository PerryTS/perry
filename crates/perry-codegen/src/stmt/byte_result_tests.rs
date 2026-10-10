use perry_hir::{types::Type, CompareOp, Expr, Function, Module, Param, Stmt, UpdateOp};
fn ir() -> String {
    let mut m = Module::new("general_byte_scanner");
    let param = |id, name: &str, ty| Param {
        id,
        name: name.into(),
        ty,
        default: None,
        decorators: vec![],
        is_rest: false,
        arguments_object: None,
    };
    m.functions.push(Function {
        id: 1,
        name: "scan".into(),
        type_params: vec![],
        params: vec![
            param(1, "bytes", Type::Named("Uint8Array".into())),
            param(2, "i", Type::Number),
        ],
        return_type: Type::Any,
        body: vec![
            Stmt::Let {
                id: 3,
                name: "short".into(),
                ty: Type::Number,
                mutable: false,
                init: Some(Expr::Integer(20)),
            },
            Stmt::For {
                init: None,
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(2)),
                    right: Box::new(Expr::LocalGet(3)),
                }),
                update: Some(Expr::Update {
                    id: 2,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![
                    Stmt::Let {
                        id: 4,
                        name: "c".into(),
                        ty: Type::Any,
                        mutable: false,
                        init: Some(Expr::IndexGet {
                            object: Box::new(Expr::LocalGet(1)),
                            index: Box::new(Expr::LocalGet(2)),
                        }),
                    },
                    Stmt::If {
                        condition: Expr::Compare {
                            op: CompareOp::Eq,
                            left: Box::new(Expr::LocalGet(4)),
                            right: Box::new(Expr::Integer(34)),
                        },
                        then_branch: vec![Stmt::Return(Some(Expr::LocalGet(2)))],
                        else_branch: None,
                    },
                ],
            },
            Stmt::Return(Some(Expr::Integer(-1))),
        ],
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
fn admitted_general_byte_result_does_not_use_a_pointer_slot() {
    let ir = ir();
    let fast = ir
        .find("\nfor.number_locals_fast.body")
        .expect("bounded byte loop");
    let read = fast
        + ir[fast..]
            .find("\nta.read.done")
            .expect("common checked read");
    let end = read + ir[read..].find("\nif.then").expect("result comparison");
    let result = &ir[read..end];
    assert!(
        result.contains("store double"),
        "byte result did not use an ordinary value slot:\n{result}"
    );
    assert!(
        !result.contains("store ptr addrspace(1)"),
        "Number-or-undefined was put in a GC pointer slot:\n{result}"
    );
    assert_eq!(
        result.matches("32766").count(),
        1,
        "only the constant-side Number normalizer needs a compact tag check:\n{result}"
    );
    assert!(ir.contains("ta.read.oob"));
    let slow = ir
        .find("\nfor.number_locals_slow.body")
        .expect("erased receiver fallback");
    assert!(
        ir[slow..].contains("store ptr addrspace(1)"),
        "lying receivers must retain heap-result roots"
    );
}
