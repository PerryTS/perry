//! Native owners must reach the existing typed fast tiers through their
//! header's data word. Inline-only address derivation is a planted fault.
use perry_hir::{types::Type, BinaryOp, CompareOp, Expr, Function, Module, Param, Stmt, UpdateOp};

fn param(id: u32, ty: Type) -> Param {
    Param {
        id,
        name: format!("v{id}"),
        ty,
        default: None,
        decorators: vec![],
        is_rest: false,
        arguments_object: None,
    }
}
fn local(id: u32, ty: Type, mutable: bool, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: format!("v{id}"),
        ty,
        mutable,
        init: Some(init),
    }
}
fn get(id: u32, index: Expr) -> Expr {
    Expr::IndexGet {
        object: Box::new(Expr::LocalGet(id)),
        index: Box::new(index),
    }
}
fn add(left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(left),
        right: Box::new(right),
    }
}
fn loop_body(bound: Expr, body: Vec<Stmt>) -> Stmt {
    Stmt::For {
        init: Some(Box::new(local(10, Type::Number, true, Expr::Integer(0)))),
        condition: Some(Expr::Compare {
            op: CompareOp::Lt,
            left: Box::new(Expr::LocalGet(10)),
            right: Box::new(bound),
        }),
        update: Some(Expr::Update {
            id: 10,
            op: UpdateOp::Increment,
            prefix: false,
        }),
        body,
    }
}
fn compile(name: &str, params: Vec<Param>, body: Vec<Stmt>, kind: u8) -> String {
    let argc = params.len();
    let mut m = Module::new(name);
    m.functions.push(Function {
        id: 1,
        name: "probe".into(),
        type_params: vec![],
        params,
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
    // A 65536-element owner is Native under the production placement policy.
    m.init.push(local(
        100,
        Type::Any,
        false,
        Expr::TypedArrayNew {
            kind,
            arg: Some(Box::new(Expr::Integer(65536))),
        },
    ));
    let args = match argc {
        1 => vec![Expr::LocalGet(100)],
        2 => vec![Expr::LocalGet(100), Expr::Integer(0)],
        3 => {
            m.init.push(local(
                101,
                Type::Any,
                false,
                Expr::TypedArrayNew {
                    kind,
                    arg: Some(Box::new(Expr::Integer(65536))),
                },
            ));
            vec![
                Expr::Array(vec![Expr::Integer(0), Expr::Integer(65535)]),
                Expr::LocalGet(100),
                Expr::LocalGet(101),
            ]
        }
        _ => unreachable!(),
    };
    m.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args,
        type_args: vec![],
        byte_offset: 0,
    }));
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
fn native_f64_region_resolves_the_owner_data_word() {
    let output = compile(
        "native_f64_region",
        vec![param(1, Type::Any)],
        vec![
            local(11, Type::Number, true, Expr::Number(0.0)),
            loop_body(
                Expr::Integer(65536),
                vec![Stmt::Expr(Expr::LocalSet(
                    11,
                    Box::new(add(
                        Expr::LocalGet(11),
                        add(get(1, Expr::LocalGet(10)), get(1, Expr::LocalGet(10))),
                    )),
                ))],
            ),
            Stmt::Return(Some(Expr::LocalGet(11))),
        ],
        perry_hir::TYPED_ARRAY_KIND_FLOAT64,
    );
    assert!(
        output.contains("rloop.ta.data.external"),
        "region must resolve Native data"
    );
    assert!(
        !output.contains(", 8421631"),
        "region admission must allow the OOL bit"
    );
    assert!(
        output.contains("rloop.fast"),
        "the region proof must remain active"
    );
}

#[test]
fn native_packed_columns_resolve_each_owner_data_word() {
    let key = || get(1, Expr::LocalGet(10));
    // Keep every entity read in the one leading statement admitted by B4.
    // x[e] = y[e] = x[e] = y[e] gives both columns two receiver accesses.
    let set = |dst, value| Expr::IndexSet {
        object: Box::new(Expr::LocalGet(dst)),
        index: Box::new(key()),
        value: Box::new(value),
    };
    let output = compile(
        "native_packed_columns",
        vec![
            param(1, Type::Any),
            param(2, Type::Any),
            param(3, Type::Any),
        ],
        vec![
            loop_body(
                Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(1)),
                    property: "length".into(),
                    byte_offset: 0,
                },
                vec![Stmt::Expr(set(2, set(3, set(2, get(3, key())))))],
            ),
            Stmt::Return(Some(Expr::Integer(0))),
        ],
        perry_hir::TYPED_ARRAY_KIND_UINT32,
    );
    assert!(
        output.contains("call i64 @js_packed_ecs_u32_loop_guard"),
        "fused admission must execute"
    );
    assert!(
        output.matches("stable_packed.u32.data.external").count() >= 2,
        "both columns must resolve Native data"
    );
}

#[test]
fn native_rmw_resolves_data_before_read_and_after_rhs() {
    let output = compile(
        "native_rmw",
        vec![
            param(1, Type::Named("Uint32Array".into())),
            param(2, Type::Number),
        ],
        vec![
            local(3, Type::Any, false, Expr::LocalGet(1)),
            local(4, Type::Any, false, Expr::LocalGet(2)),
            Stmt::Expr(Expr::IndexSet {
                object: Box::new(Expr::LocalGet(3)),
                index: Box::new(Expr::LocalGet(4)),
                value: Box::new(add(get(3, Expr::LocalGet(4)), Expr::Integer(1))),
            }),
            Stmt::Return(Some(Expr::Integer(0))),
        ],
        perry_hir::TYPED_ARRAY_KIND_UINT32,
    );
    for marker in [
        "ta.rmw.read.data.external",
        "ta.rmw.write.data.external",
        "ta.rmw.set_fallback",
    ] {
        assert!(output.contains(marker), "RMW must retain {marker}");
    }
    assert!(
        !output.contains(", 8454399"),
        "RMW admission must allow the OOL bit"
    );
}

#[test]
fn inline_only_data_derivation_turns_all_native_tier_witnesses_red() {
    for name in [
        "native_f64_region_resolves_the_owner_data_word",
        "native_packed_columns_resolve_each_owner_data_word",
        "native_rmw_resolves_data_before_read_and_after_rhs",
    ] {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                &format!("expr::byte_cell::native_owner_tests::{name}"),
                "--nocapture",
            ])
            .env("PERRY_B4_SABOTAGE", "inline_data")
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&child.stdout).contains("running 1 test"));
        assert!(!child.status.success(), "inline-only {name} must fail");
    }
}
