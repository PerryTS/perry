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

pub(super) fn ir() -> String {
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
    String::from_utf8(
        crate::compile_module(&module, crate::temp_root_coverage::entry_opts()).unwrap(),
    )
    .unwrap()
}
