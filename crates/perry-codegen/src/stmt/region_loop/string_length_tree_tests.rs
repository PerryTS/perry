//! #12485: a fact tree's `o.k.length` leaf is the string length of a bare
//! read, so a loop summing string lengths folds to `fadd`s in F-body.
use super::*;
use perry_hir::{types::Type, Function, Module, Param, UpdateOp};

fn bin(left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn get(object: Expr, property: &str) -> Expr {
    Expr::PropertyGet {
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

#[test]
fn length_leaves_join_one_receivers_fact_tree() {
    let covered = |_: Recv, _: &str| true;
    let tree = left_leaning_lengths();
    let (r, reads) = fact_tree_leaves(&tree, &covered).expect("a fact tree");
    assert!(r == Recv::Local(3));
    let keys: Vec<_> = reads
        .iter()
        .map(|read| match read {
            Expr::PropertyGet { property, .. } => property.as_str(),
            _ => panic!("the bare read is the inner field read"),
        })
        .collect();
    assert_eq!(keys, ["a", "b", "c", "e"]);

    // Another receiver's length, a call's length, or an uncovered key keep
    // today's lowering.
    let other = bin(length_of("a"), get(get(Expr::LocalGet(4), "b"), "length"));
    assert!(fact_tree_leaves(&other, &covered).is_none());
    let call = Expr::Call {
        callee: Box::new(get(Expr::LocalGet(3), "m")),
        args: vec![],
        type_args: vec![],
        byte_offset: 0,
    };
    assert!(fact_tree_leaves(&bin(length_of("a"), get(call, "length")), &covered).is_none());
    let only_a = |_: Recv, k: &str| k == "a";
    assert!(fact_tree_leaves(&bin(length_of("a"), length_of("b")), &only_a).is_none());
}

fn sum_lengths_ir() -> String {
    let loop_body = vec![
        Stmt::Expr(Expr::PutValueSet {
            target: Box::new(Expr::LocalGet(3)),
            key: Box::new(Expr::String("d".into())),
            value: Box::new(Expr::LocalGet(5)),
            receiver: Box::new(Expr::LocalGet(3)),
            strict: false,
        }),
        Stmt::Expr(Expr::LocalSet(
            4,
            Box::new(bin(Expr::LocalGet(4), left_leaning_lengths())),
        )),
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
fn summed_string_lengths_fold_in_the_fast_body() {
    let ir = sum_lengths_ir();
    assert!(ir.contains("rloop.fast"), "no loop region:\n{ir}");
    assert!(
        ir.contains("rloop.len.sso")
            && ir.contains("rloop.len.heap")
            && ir.contains("rloop.len.arr"),
        "the length leaves are not the inline string and Array arms:\n{ir}"
    );
    assert!(
        ir.contains("rloop.tree.generic"),
        "a non-string value must keep the generic arm:\n{ir}"
    );
    // The fast tree reads `.length` without any call: its NaN arm defers
    // every other receiver to the generic arm, which has run no JS yet.
    let mut in_length_arm = false;
    for line in ir.lines() {
        if !line.starts_with(' ') && line.ends_with(':') {
            in_length_arm = line.starts_with("rloop.len.");
            continue;
        }
        assert!(!(in_length_arm && line.contains("call ")), "{line}");
    }
}
