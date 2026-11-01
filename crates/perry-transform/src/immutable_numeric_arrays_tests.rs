use super::*;
use perry_hir::types::Type;
use perry_hir::Function;

fn function(body: Vec<Stmt>) -> Function {
    Function {
        id: 1,
        name: "scan".into(),
        params: vec![],
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: false,
        type_params: vec![],
        is_strict: true,
        is_exported: false,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    }
}
fn read(index: Expr) -> Expr {
    Expr::IndexGet {
        object: Box::new(Expr::LocalGet(1)),
        index: Box::new(index),
        byte_offset: 0,
        byte_end: 0,
    }
}
fn length() -> Expr {
    Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(1)),
        property: "length".into(),
        byte_offset: 0,
        byte_end: 0,
    }
}
fn fixture(compound: bool, live: bool) -> Module {
    let mut m = Module::new("private_array.ts");
    m.init.push(Stmt::Let {
        id: 1,
        name: "steps".into(),
        ty: Type::Array(Box::new(Type::Number)),
        mutable: false,
        init: Some(Expr::Array(
            vec![-17, -16, -15, -1, 1, 15, 16, 17]
                .into_iter()
                .map(Expr::Integer)
                .collect(),
            None,
        )),
    });
    let update = if compound {
        Expr::LocalSet(
            2,
            Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::LocalGet(2)),
                right: Box::new(Expr::Integer(1)),
            }),
        )
    } else {
        Expr::Update {
            id: 2,
            op: UpdateOp::Increment,
            prefix: false,
        }
    };
    m.functions.push(function(vec![Stmt::For {
        init: Some(Box::new(Stmt::Let {
            id: 2,
            name: "j".into(),
            ty: Type::Number,
            mutable: true,
            init: Some(Expr::Integer(0)),
        })),
        condition: Some(Expr::Compare {
            op: CompareOp::Lt,
            left: Box::new(Expr::LocalGet(2)),
            right: Box::new(if live { length() } else { Expr::Integer(8) }),
        }),
        update: Some(update),
        body: vec![Stmt::Return(Some(read(Expr::LocalGet(2))))],
    }]));
    m
}
fn has_read(m: &Module) -> bool {
    let mut found = false;
    perry_hir::tdz_check::for_each_module_expr(m, &mut |e| {
        found |= matches!(e, Expr::IndexGet { .. })
    });
    found
}
fn eval(e: &Expr, i: i64) -> i64 {
    match e {
        Expr::Integer(n) => *n,
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            let Expr::Compare {
                op: CompareOp::Lt,
                right,
                ..
            } = condition.as_ref()
            else {
                panic!("{e:?}")
            };
            eval(
                if i < integer(right).unwrap() {
                    then_expr
                } else {
                    else_expr
                },
                i,
            )
        }
        _ => panic!("unexpected folded value {e:?}"),
    }
}

#[test]
fn live_and_fixed_scans_fold_for_both_increment_spellings() {
    for compound in [false, true] {
        for live in [false, true] {
            let mut m = fixture(compound, live);
            run(&mut m);
            assert!(
                !has_read(&m),
                "accepted element reads must actually fold: {m:?}"
            );
            let Stmt::For {
                condition: Some(Expr::Compare { right, .. }),
                body,
                ..
            } = &m.functions[0].body[0]
            else {
                panic!()
            };
            assert!(matches!(right.as_ref(), Expr::Integer(8)));
            let Stmt::Return(Some(value)) = &body[0] else {
                panic!()
            };
            for (i, n) in [-17, -16, -15, -1, 1, 15, 16, 17].iter().enumerate() {
                assert_eq!(eval(value, i as i64), *n);
            }
        }
    }
}

#[test]
fn aliases_exports_deletes_and_hidden_uses_decline() {
    for case in 0..7 {
        let mut m = fixture(true, true);
        match case {
            0 => m.init.push(Stmt::Let {
                id: 4,
                name: "alias".into(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::LocalGet(1)),
            }),
            1 => m.exported_objects.push("steps".into()),
            2 => m
                .init
                .push(Stmt::Expr(Expr::Delete(Box::new(read(Expr::Integer(0)))))),
            3 => m.init.push(Stmt::Expr(Expr::ArrayPop(1))),
            4 => m.functions[0].captures.push(1),
            5 => m.functions[0]
                .body
                .push(Stmt::Return(Some(Expr::LocalGet(1)))),
            6 => m.functions[0].body.push(Stmt::Expr(Expr::LocalSet(
                1,
                Box::new(Expr::Array(vec![], None)),
            ))),
            _ => unreachable!(),
        }
        run(&mut m);
        assert!(
            has_read(&m),
            "escape/mutation case {case} must retain reads"
        );
    }
}

#[test]
fn counter_writes_and_out_of_bounds_reads_keep_lookup() {
    for case in 0..5 {
        let mut m = fixture(true, true);
        let Stmt::For { body, .. } = &mut m.functions[0].body[0] else {
            panic!()
        };
        match case {
            0 => body.insert(
                0,
                Stmt::Expr(Expr::LocalSet(2, Box::new(Expr::Integer(20)))),
            ),
            1 => body[0] = Stmt::Return(Some(read(Expr::Number(0.5)))),
            2 => body[0] = Stmt::Return(Some(read(Expr::Integer(8)))),
            3 => body[0] = Stmt::Return(Some(read(Expr::Number(f64::NAN)))),
            4 => body[0] = Stmt::Return(Some(read(Expr::String("0".into())))),
            _ => unreachable!(),
        }
        run(&mut m);
        assert!(
            has_read(&m),
            "case {case} needs ordinary key/prototype semantics"
        );
    }
}

#[test]
fn literal_values_preserve_nan_and_negative_zero_but_holes_decline() {
    let mut m = fixture(true, true);
    let Stmt::Let {
        init: Some(Expr::Array(values, _)),
        ..
    } = &mut m.init[0]
    else {
        panic!()
    };
    values[0] = Expr::Number(-0.0);
    values[1] = Expr::Number(f64::NAN);
    m.functions[0].body = vec![
        Stmt::Return(Some(read(Expr::Integer(0)))),
        Stmt::Return(Some(read(Expr::Integer(1)))),
    ];
    run(&mut m);
    let Stmt::Return(Some(Expr::Number(n))) = &m.functions[0].body[0] else {
        panic!()
    };
    assert_eq!(n.to_bits(), (-0.0f64).to_bits());
    let Stmt::Return(Some(Expr::Number(n))) = &m.functions[0].body[1] else {
        panic!()
    };
    assert!(n.is_nan());
    let mut m = fixture(true, true);
    let Stmt::Let {
        init: Some(Expr::Array(values, _)),
        ..
    } = &mut m.init[0]
    else {
        panic!()
    };
    values[0] = Expr::Undefined;
    run(&mut m);
    assert!(has_read(&m));
}

#[test]
fn folded_reads_keep_temporal_dead_zone_checks() {
    let mut m = fixture(true, true);
    let mut e = read(Expr::Integer(0));
    let Expr::IndexGet { object, .. } = &mut e else {
        panic!()
    };
    *object = Box::new(Expr::Sequence(vec![
        perry_hir::tdz_check::check(1, "steps"),
        Expr::LocalGet(1),
    ]));
    m.functions[0].body = vec![Stmt::Return(Some(e))];
    run(&mut m);
    let Stmt::Return(Some(Expr::Sequence(parts))) = &m.functions[0].body[0] else {
        panic!("{:?}", m.functions[0].body)
    };
    assert_eq!(perry_hir::tdz_check::checked_binding(&parts[0]), Some(1));
    assert!(matches!(parts[1], Expr::Integer(-17)));
}

#[test]
fn an_unproven_read_keeps_even_length_and_other_inbounds_reads_live() {
    let mut m = fixture(true, true);
    m.functions[0].body.push(Stmt::Expr(read(Expr::Integer(9))));
    run(&mut m);
    let Stmt::For {
        condition: Some(Expr::Compare { right, .. }),
        body,
        ..
    } = &m.functions[0].body[0]
    else {
        panic!()
    };
    assert!(
        matches!(right.as_ref(), Expr::PropertyGet { .. }),
        "an inherited getter can change length"
    );
    assert!(
        matches!(&body[0], Stmt::Return(Some(Expr::IndexGet { .. }))),
        "that getter can also replace own values"
    );
}

#[test]
fn a_rejected_array_cannot_prove_another_arrays_index_range() {
    let mut m = fixture(true, true);
    m.init.push(Stmt::Let {
        id: 3,
        name: "other".into(),
        ty: Type::Any,
        mutable: false,
        init: Some(Expr::Array(vec![Expr::Integer(10); 8], None)),
    });
    let Stmt::For { body, .. } = &mut m.functions[0].body[0] else {
        panic!()
    };
    let Stmt::Return(Some(Expr::IndexGet { object, .. })) = &mut body[0] else {
        panic!()
    };
    *object = Box::new(Expr::LocalGet(3));
    m.functions[0].body.push(Stmt::Expr(read(Expr::Integer(9))));
    run(&mut m);
    let Stmt::For { body, .. } = &m.functions[0].body[0] else {
        panic!()
    };
    assert!(matches!(
        &body[0],
        Stmt::Return(Some(Expr::IndexGet { .. }))
    ));
}

#[test]
fn captured_counter_metadata_cannot_establish_an_own_read() {
    let mut m = fixture(true, true);
    m.functions[0].captures.push(2);
    run(&mut m);
    assert!(has_read(&m));
}

#[test]
fn construction_proof_does_not_depend_on_the_array_annotation() {
    let mut m = fixture(true, true);
    let Stmt::Let { ty, .. } = &mut m.init[0] else {
        panic!()
    };
    *ty = Type::Any;
    run(&mut m);
    assert!(!has_read(&m));
}
