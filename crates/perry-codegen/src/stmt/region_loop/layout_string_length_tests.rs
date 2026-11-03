//! #12485: a fact tree's `o.k.length` leaf is the string length of a bare
//! read, so a loop summing string lengths folds to `fadd`s in F-body.
use perry_hir::{types::Type, Function, Module, Param, UpdateOp};
use perry_hir::{BinaryOp, CompareOp, Expr, Stmt};

fn bin(left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn get(object: Expr, property: &str) -> Expr {
    Expr::PropertyGet {
        byte_end: 0,
        object: Box::new(object),
        property: property.into(),
        byte_offset: 0,
    }
}

fn length_of(key: &str) -> Expr {
    get(get(Expr::LocalGet(3), key), "length")
}

/// `((o.a.length + o.b.length) + o.c.length) + o.e.length`
fn left_leaning_lengths() -> Expr {
    bin(
        bin(bin(length_of("a"), length_of("b")), length_of("c")),
        length_of("e"),
    )
}

fn sum_lengths_ir_with(tree: Expr) -> String {
    let loop_body = vec![
        Stmt::Expr(Expr::PutValueSet {
            target: Box::new(Expr::LocalGet(3)),
            key: Box::new(Expr::String("d".into())),
            value: Box::new(Expr::LocalGet(5)),
            receiver: Box::new(Expr::LocalGet(3)),
            strict: false,
            byte_offset: 0,
            byte_end: 0,
        }),
        Stmt::Expr(Expr::LocalSet(4, Box::new(bin(Expr::LocalGet(4), tree)))),
    ];
    let body = vec![
        Stmt::Let {
            id: 4,
            name: "h".into(),
            ty: Type::Number,
            mutable: true,
            init: Some(Expr::Integer(0)),
        },
        Stmt::For {
            init: Some(Box::new(Stmt::Let {
                id: 5,
                name: "k".into(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Integer(0)),
            })),
            condition: Some(Expr::Compare {
                op: CompareOp::Lt,
                left: Box::new(Expr::LocalGet(5)),
                right: Box::new(Expr::LocalGet(2)),
            }),
            update: Some(Expr::Update {
                id: 5,
                op: UpdateOp::Increment,
                prefix: false,
            }),
            body: loop_body,
        },
        Stmt::Return(Some(Expr::LocalGet(4))),
    ];
    let param = |id: u32, name: &str, ty: Type| Param {
        id,
        name: name.into(),
        ty,
        default: None,
        decorators: vec![],
        is_rest: false,
        arguments_object: None,
    };
    let mut m = Module::new("strlen_tree");
    m.functions.push(Function {
        id: 1,
        name: "run".into(),
        type_params: vec![],
        params: vec![param(2, "n", Type::Number), param(3, "o", Type::Any)],
        return_type: Type::Number,
        body,
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
                is_entry_module: false,
                ..crate::CompileOptions::default()
            },
        )
        .expect("string length tree compiles"),
    )
    .unwrap()
}

#[test]
fn nested_string_length_inputs_retain_live_property_reads() {
    let call = Expr::Call {
        byte_end: 0,
        callee: Box::new(get(Expr::LocalGet(3), "m")),
        args: vec![],
        type_args: vec![],
        byte_offset: 0,
    };
    for tree in [
        left_leaning_lengths(),
        bin(length_of("a"), get(get(Expr::LocalGet(4), "b"), "length")),
        bin(length_of("a"), get(call, "length")),
        bin(length_of("a"), length_of("b")),
    ] {
        let ir = sum_lengths_ir_with(tree);
        crate::testing::verify_ir(&ir, "strlen_tree").unwrap();
        assert!(ir.contains("length"), "the source keeps length GetValue");
        assert!(
            !ir.contains("rloop.tree"),
            "there is no stale array/string fact tree"
        );
    }
}
#[test]
fn summed_string_lengths_keep_numeric_addition_and_generic_conversion() {
    let ir = sum_lengths_ir();
    crate::testing::verify_ir(&ir, "strlen_tree").unwrap();
    assert!(
        ir.contains("fadd double") || ir.contains("js_dynamic_string_or_number_add"),
        "source summation remains"
    );
    assert!(
        ir.contains("js_value_length") || ir.contains("object_get_field"),
        "an unproven length retains semantic GetValue"
    );
}

fn sum_lengths_ir() -> String {
    sum_lengths_ir_with(left_leaning_lengths())
}
