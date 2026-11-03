use perry_hir::types::Type;
use perry_hir::{Expr, Param, Stmt};

fn runtime(method: &str, args: Vec<Expr>) -> Expr {
    Expr::NativeMethodCall {
        module: "__perry_runtime".into(),
        class_name: None,
        object: None,
        method: method.into(),
        args,
    }
}
#[test]
fn private_resume_frame_uses_parent_owner_and_nonzero_offset() {
    crate::temp_root_coverage::under_both_lowerings(|mode| {
        let let_any = |id, init| Stmt::Let {
            id,
            name: format!("frame{id}"),
            ty: Type::Any,
            mutable: true,
            init: Some(init),
        };
        let params = [100, 101]
            .into_iter()
            .map(|id| Param {
                id,
                name: format!("p{id}"),
                ty: Type::Any,
                default: None,
                decorators: vec![],
                is_rest: false,
                arguments_object: None,
            })
            .collect();
        let body = vec![
            Stmt::Expr(Expr::LocalSet(
                7,
                Box::new(Expr::Array(vec![Expr::Number(4.0)], Default::default())),
            )),
            Stmt::Expr(Expr::LocalSet(5, Box::new(Expr::Number(0.0)))),
            Stmt::Return(Some(Expr::LocalGet(7))),
        ];
        let closure = Expr::Closure {
            func_id: 1,
            params,
            return_type: Type::Any,
            body,
            captures: vec![1, 2, 3, 4, 5, 6, 7],
            mutable_captures: vec![],
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: false,
            is_strict: true,
            is_async: false,
            is_generator: false,
        };
        let link = Expr::LinkGeneratorPrototype {
            obj: Box::new(closure),
            is_async: false,
            fixed_state: true,
            i32_controls: vec![1, 5],
            i1_controls: vec![2, 4],
        };
        let mut init: Vec<_> = (1..=7).map(|id| let_any(id, Expr::Undefined)).collect();
        init.extend([
            let_any(10, runtime("privateResumeFrame", vec![link])),
            let_any(11, Expr::Undefined),
            Stmt::Expr(runtime(
                "privateResumeFrameStep",
                vec![
                    Expr::LocalGet(10),
                    Expr::Integer(1),
                    Expr::LocalSet(11, Box::new(runtime("iteratorStepOutput", vec![]))),
                ],
            )),
            crate::temp_root_coverage::console_log(vec![Expr::LocalGet(11)]),
        ]);
        let module = crate::temp_root_coverage::module_with_init(
            "frame_owner",
            vec![Stmt::If {
                condition: Expr::Bool(true),
                then_branch: init,
                else_branch: None,
            }],
        );
        let ir = String::from_utf8(
            crate::compile_module(&module, crate::temp_root_coverage::entry_opts()).unwrap(),
        )
        .unwrap();
        let definition = ir
            .lines()
            .find(|l| l.starts_with("define ") && l.contains("@perry_resume_frame__"))
            .expect("private definition");
        assert!(
            !definition.contains("optsize"),
            "{mode}: private continuation uses execution policy"
        );
        assert!(
            definition.contains("i32 %frame_offset")
                && definition.contains("i32 %frame_op")
                && definition.contains("ptr %frame_out_kind"),
            "{mode}: {definition}"
        );
        let call = ir
            .lines()
            .find(|l| l.contains("call double @perry_resume_frame__"))
            .expect("live private call");
        assert!(call.contains("i32 1, i32 0, double"), "{mode}: {call}");
        assert!(!ir.contains("call double @js_generator_create("));
        assert!(!ir.contains("call i32 @js_iterator_step("));
        assert!(
            ir.contains(", 80"),
            "managed slot includes the fixed nonzero frame offset"
        );
        assert!(
            ir.contains("call void @js_write_barrier_slot_validated_parent("),
            "managed store retains parent barrier"
        );
        assert!(
            ir.contains("ptr %frame_out_kind"),
            "raw output kind is native stack storage"
        );
        // The owner's root is still the allocation start. Only slot addresses
        // use the offset; the explicit bind/statepoint machinery remains live.
        assert!(ir.contains("%this_closure"));
        assert!(
            ir.contains("js_shadow_slot_bind") || ir.contains("addrspace(1)"),
            "{mode}: owner root"
        );
    });
}

#[test]
fn async_adapter_queues_the_allocation_owner_and_keeps_managed_slot_barriers() {
    crate::temp_root_coverage::under_both_lowerings(|mode| {
        let params = [100, 101]
            .into_iter()
            .map(|id| Param {
                id,
                name: format!("p{id}"),
                ty: if id == 101 { Type::Boolean } else { Type::Any },
                default: None,
                decorators: vec![],
                is_rest: false,
                arguments_object: None,
            })
            .collect();
        let closure = Expr::Closure {
            func_id: 1,
            params,
            return_type: Type::Any,
            body: vec![
                Stmt::Expr(Expr::LocalSet(
                    8,
                    Box::new(Expr::Binary {
                        op: perry_hir::BinaryOp::Add,
                        left: Box::new(Expr::LocalGet(8)),
                        right: Box::new(Expr::Integer(1)),
                    }),
                )),
                Stmt::Expr(Expr::LocalSet(
                    7,
                    Box::new(Expr::Array(vec![Expr::Number(4.0)], None)),
                )),
                Stmt::Return(Some(Expr::AsyncStepDone {
                    value: Box::new(Expr::LocalGet(7)),
                    step_closure: Box::new(Expr::CurrentStepClosure),
                })),
            ],
            captures: vec![7, 8],
            mutable_captures: vec![],
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: false,
            is_strict: true,
            is_async: false,
            is_generator: false,
        };
        let init = vec![
            Stmt::Let {
                id: 8,
                name: "ordinary_spelling".into(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Number(0.0)),
            },
            Stmt::Let {
                id: 7,
                name: "pointer".into(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Undefined),
            },
            Stmt::Let {
                id: 10,
                name: "owner".into(),
                ty: Type::Any,
                mutable: false,
                init: Some(runtime(
                    "privateAsyncResumeFrame",
                    vec![
                        closure,
                        Expr::Sequence(vec![Expr::LocalGet(8)]),
                        Expr::Sequence(vec![]),
                    ],
                )),
            },
            Stmt::Expr(Expr::AsyncFirstCall {
                step_closure: Box::new(Expr::LocalGet(10)),
            }),
        ];
        let module = crate::temp_root_coverage::module_with_init(
            "async_frame_owner",
            vec![Stmt::If {
                condition: Expr::Bool(true),
                then_branch: init,
                else_branch: None,
            }],
        );
        let ir = String::from_utf8(
            crate::compile_module(&module, crate::temp_root_coverage::entry_opts()).unwrap(),
        )
        .unwrap();
        assert!(
            ir.contains("fptosi double"),
            "{mode}: explicit scalar-control descriptor lost"
        );
        assert!(
            ir.contains("@js_async_first_call("),
            "{mode}: live queue entry"
        );
        let adapter = ir
            .lines()
            .find(|l| l.starts_with("define ") && l.contains("%sent") && l.contains("%error"))
            .expect("callable parent adapter");
        assert!(
            adapter.contains("i64 %this_closure, i64 %js_this, double %sent, double %error"),
            "{mode}: {adapter}"
        );
        let call = ir
            .lines()
            .find(|l| l.contains("call double @perry_resume_frame__"))
            .expect("live async private call");
        assert!(
            call.contains("i64 %this_closure, i32 1, i32"),
            "{mode}: parent plus nonzero child offset"
        );
        assert!(
            ir.contains("call void @js_write_barrier_slot_validated_parent("),
            "{mode}: managed slot barrier"
        );
        assert!(
            call.contains("ptr null")
                && !ir
                    .lines()
                    .any(|line| line.contains("store") && line.contains("ptr %frame_out_kind")),
            "{mode}: async adapter needs no native output-kind stack slot"
        );
        assert!(
            ir.contains("js_shadow_slot_bind") || ir.contains("addrspace(1)"),
            "{mode}: allocation-owner statepoint root"
        );
    });
}

fn private_resume_ir(body: Vec<Stmt>) -> String {
    let let_any = |id, init| Stmt::Let {
        id,
        name: format!("frame{id}"),
        ty: Type::Any,
        mutable: true,
        init: Some(init),
    };
    let params = [100, 101]
        .into_iter()
        .map(|id| Param {
            id,
            name: format!("p{id}"),
            ty: Type::Any,
            default: None,
            decorators: vec![],
            is_rest: false,
            arguments_object: None,
        })
        .collect();
    let closure = Expr::Closure {
        func_id: 1,
        params,
        return_type: Type::Any,
        body,
        captures: vec![1, 2, 3, 4, 5, 6, 7],
        mutable_captures: vec![],
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: false,
        is_strict: true,
        is_async: false,
        is_generator: false,
    };
    let link = Expr::LinkGeneratorPrototype {
        obj: Box::new(closure),
        is_async: false,
        fixed_state: true,
        i32_controls: vec![1, 5],
        i1_controls: vec![2, 4],
    };
    let mut init: Vec<_> = (1..=7).map(|id| let_any(id, Expr::Undefined)).collect();
    init.extend([
        let_any(10, runtime("privateResumeFrame", vec![link])),
        let_any(11, Expr::Undefined),
        Stmt::Expr(runtime(
            "privateResumeFrameStep",
            vec![
                Expr::LocalGet(10),
                Expr::Integer(1),
                Expr::LocalSet(11, Box::new(runtime("iteratorStepOutput", vec![]))),
            ],
        )),
        crate::temp_root_coverage::console_log(vec![Expr::LocalGet(11)]),
    ]);
    let module = crate::temp_root_coverage::module_with_init(
        "frame_owner",
        vec![Stmt::If {
            condition: Expr::Bool(true),
            then_branch: init,
            else_branch: None,
        }],
    );
    String::from_utf8(
        crate::compile_module(&module, crate::temp_root_coverage::entry_opts()).unwrap(),
    )
    .unwrap()
}

fn private_definition(ir: &str) -> String {
    let start = ir
        .lines()
        .position(|l| l.starts_with("define ") && l.contains("@perry_resume_frame__"))
        .expect("private definition");
    ir.lines()
        .skip(start)
        .take_while(|l| *l != "}")
        .collect::<Vec<_>>()
        .join("\n")
}

/// RLF: the state, kind and done controls are resume-local SSA values. The
/// body must not re-read them from the frame and every exit must write the
/// written ones back. Each `ret` and each explicit throw is an exit.
#[test]
fn resume_locals_write_back_on_every_exit() {
    crate::temp_root_coverage::under_both_lowerings(|mode| {
        let set = |id, value| Stmt::Expr(Expr::LocalSet(id, Box::new(value)));
        let body = vec![
            set(1, Expr::Number(3.0)),
            set(5, Expr::Number(1.0)),
            set(2, Expr::Bool(true)),
            Stmt::If {
                condition: Expr::Bool(false),
                then_branch: vec![set(1, Expr::Number(4.0)), Stmt::Throw(Expr::Number(1.0))],
                else_branch: None,
            },
            Stmt::Return(Some(Expr::LocalGet(7))),
        ];
        let def = private_definition(&private_resume_ir(body));
        let exits = def.matches("ret double").count() + def.matches("@js_throw(").count();
        let writes = def.matches("; rlf.write_back").count();
        assert!(exits >= 2, "{mode}: body has a return and a throw\n{def}");
        assert_eq!(
            writes, exits,
            "{mode}: every exit writes the controls back\n{def}"
        );
    });
}

/// The controls are never `executing`: capture index three stays in the
/// frame, so a re-entrant resume still observes it.
#[test]
fn resume_locals_keep_executing_in_the_frame() {
    crate::temp_root_coverage::under_both_lowerings(|mode| {
        let set = |id, value| Stmt::Expr(Expr::LocalSet(id, Box::new(value)));
        let body = vec![
            set(4, Expr::Bool(true)),
            set(1, Expr::Number(2.0)),
            Stmt::Return(Some(Expr::LocalGet(7))),
        ];
        let def = private_definition(&private_resume_ir(body));
        // state and kind are localized (entry allocas); executing (id 4, frame
        // word index 3 + 1) is a direct frame store, never copied.
        assert!(def.contains("alloca i32"), "{mode}: resume locals\n{def}");
        let writes = def.matches("; rlf.write_back").count();
        assert_eq!(writes, 1, "{mode}: one return, one write-back\n{def}");
    });
}
