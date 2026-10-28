use super::*;

fn fixture(ty: Type) -> Module {
    let mut m = Module::new("private_frame");
    m.functions.push(Function {
        id: 0,
        name: "producer".into(),
        type_params: vec![],
        params: vec![],
        return_type: ty,
        body: vec![Stmt::Expr(Expr::Yield {
            value: Some(Box::new(Expr::Number(7.0))),
            delegate: false,
        })],
        is_async: false,
        is_generator: true,
        is_strict: true,
        is_exported: false,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    });
    m.init = vec![
        Stmt::Let {
            id: 10,
            name: "it".into(),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::Call {
                callee: Box::new(Expr::FuncRef(0)),
                args: vec![],
                type_args: vec![],
                byte_offset: 0,
            }),
        },
        Stmt::Expr(runtime(
            "iteratorStep",
            vec![
                Expr::LocalGet(10),
                Expr::Undefined,
                Expr::LocalSet(11, Box::new(runtime("iteratorStepOutput", vec![]))),
            ],
        )),
    ];
    m
}
fn admitted(m: &Module) -> bool {
    format!("{:?}", m.init).contains("method: \"privateResumeFrame\"")
}

#[test]
fn private_frame_admission_uses_values_and_uses_not_annotations() {
    for ty in [Type::Any, Type::Number, Type::String] {
        let mut m = fixture(ty);
        super::super::transform_generators(&mut m);
        assert!(admitted(&m), "known confined generator refused");
        assert!(format!("{:?}", m.init).contains("privateResumeFrameStep"));
        // Its original public producer remains callable on the old ABI.
        assert!(format!("{:?}", m.functions[0].body).contains("LinkGeneratorPrototype"));
    }
}
#[test]
fn private_frame_escape_and_alias_keep_standalone_record() {
    for escape in [
        Stmt::Return(Some(Expr::LocalGet(10))),
        Stmt::Let {
            id: 12,
            name: "alias".into(),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::LocalGet(10)),
        },
    ] {
        let mut m = fixture(Type::Number);
        m.init.push(escape);
        super::super::transform_generators(&mut m);
        assert!(
            !admitted(&m),
            "escaped generator lost its standalone record"
        );
    }
}
#[test]
fn private_frame_unknown_effect_and_target_keep_standalone_record() {
    for unknown in [
        Expr::Call {
            callee: Box::new(Expr::LocalGet(90)),
            args: vec![],
            type_args: vec![],
            byte_offset: 0,
        },
        Expr::PropertySet {
            object: Box::new(Expr::FuncRef(0)),
            property: "prototype".into(),
            value: Box::new(Expr::Undefined),
        },
        Expr::IndexGet {
            object: Box::new(Expr::FuncRef(0)),
            index: Box::new(Expr::String("caller".into())),
        },
    ] {
        let mut m = fixture(Type::Number);
        m.init.insert(0, Stmt::Expr(unknown));
        super::super::transform_generators(&mut m);
        assert!(
            !admitted(&m),
            "unproven protocol effects must use standalone record"
        );
    }
}
#[test]
fn private_frame_lifetime_cannot_cross_its_consumer_scope() {
    let mut m = fixture(Type::Any);
    let init = std::mem::take(&mut m.init);
    m.init = vec![
        Stmt::If {
            condition: Expr::Bool(true),
            then_branch: init,
            else_branch: None,
        },
        Stmt::Expr(runtime("iteratorNextMethod", vec![Expr::LocalGet(10)])),
    ];
    super::super::transform_generators(&mut m);
    assert!(
        !admitted(&m),
        "an outer-scope protocol use cannot name a child frame"
    );
}
#[test]
fn private_frame_capture_even_without_body_read_is_an_escape() {
    let mut m = fixture(Type::Any);
    let closure = Expr::Closure {
        func_id: 2,
        params: vec![],
        return_type: Type::Any,
        body: vec![],
        captures: vec![10],
        mutable_captures: vec![],
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: true,
    };
    m.init.push(Stmt::Expr(closure));
    super::super::transform_generators(&mut m);
    assert!(!admitted(&m));
}

#[test]
fn private_frame_cloning_gives_every_binding_a_fresh_identity() {
    let mut m = fixture(Type::Any);
    super::super::transform_generators(&mut m);
    assert!(admitted(&m));
    let mut original = HashSet::new();
    visit_stmts(&m.functions[0].body, &mut |e| {
        if let Expr::Closure {
            params,
            body,
            captures,
            ..
        } = e
        {
            original.extend(params.iter().map(|p| p.id));
            original.extend(captures.iter().copied());
            original.extend(crate::inline::collect_body_local_ids(body));
        }
    });
    original.extend(crate::inline::collect_body_local_ids(&m.functions[0].body));
    let mut cloned: HashSet<_> = crate::inline::collect_body_local_ids(&m.init)
        .into_iter()
        .collect();
    visit_stmts(&m.init, &mut |e| {
        if let Expr::Closure { params, body, .. } = e {
            cloned.extend(params.iter().map(|p| p.id));
            cloned.extend(crate::inline::collect_body_local_ids(body));
        }
    });
    assert!(
        original.is_disjoint(&cloned),
        "cloned bindings alias the public producer: {:?}",
        original.intersection(&cloned).collect::<Vec<_>>()
    );
}

#[test]
fn private_frame_terminal_logging_cannot_hide_observable_getters() {
    let mut m = fixture(Type::Any);
    m.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(Expr::GlobalGet(0)),
            property: "log".into(),
            byte_offset: 0,
        }),
        args: vec![Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::String("caller ".into())),
            right: Box::new(Expr::IndexGet {
                object: Box::new(Expr::FuncRef(0)),
                index: Box::new(Expr::String("caller".into())),
            }),
        }],
        type_args: vec![],
        byte_offset: 0,
    }));
    super::super::transform_generators(&mut m);
    assert!(
        !admitted(&m),
        "late reflection is still an observable effect"
    );
}

#[test]
fn private_frame_logging_requires_primitive_writes_not_a_number_annotation() {
    for (value, expected) in [
        (Expr::Number(1.0), true),
        (
            Expr::Closure {
                func_id: 9,
                params: vec![],
                return_type: Type::Any,
                body: vec![Stmt::Return(Some(Expr::Number(1.0)))],
                captures: vec![],
                mutable_captures: vec![],
                captures_this: false,
                captures_new_target: false,
                enclosing_class: None,
                is_arrow: true,
                is_async: false,
                is_generator: false,
                is_strict: true,
            },
            false,
        ),
    ] {
        let mut m = fixture(Type::Any);
        m.init.push(Stmt::Let {
            id: 20,
            name: "logged".into(),
            ty: Type::Number,
            mutable: false,
            init: Some(value),
        });
        m.init.push(Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::GlobalGet(0)),
                property: "log".into(),
                byte_offset: 0,
            }),
            args: vec![Expr::LocalGet(20)],
            type_args: vec![],
            byte_offset: 0,
        }));
        super::super::transform_generators(&mut m);
        assert_eq!(
            admitted(&m),
            expected,
            "logging output must be proven primitive"
        );
    }
}
