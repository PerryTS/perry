use super::*;
use perry_hir::types::Type;

fn param(id: u32) -> Param {
    Param {
        id,
        name: format!("p{id}"),
        ty: Type::Any,
        default: None,
        is_rest: false,
        arguments_object: None,
        decorators: vec![],
    }
}
fn binding(id: u32, expr: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: format!("v{id}"),
        ty: Type::Any,
        mutable: false,
        init: Some(expr),
    }
}
fn closure(id: u32, params: Vec<Param>, body: Vec<Stmt>, captures: Vec<u32>) -> Expr {
    Expr::Closure {
        func_id: id,
        params,
        body,
        captures,
        mutable_captures: vec![],
        return_type: Type::Any,
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: true,
    }
}
fn call(id: u32, args: Vec<Expr>) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::LocalGet(id)),
        args,
        type_args: vec![],
        byte_offset: 0,
    }
}
fn module() -> Module {
    let mut m = Module::new("factory-proof");
    let inner = closure(
        20,
        vec![param(4)],
        vec![
            binding(5, Expr::ObjectCreate(Box::new(Expr::LocalGet(3)), None)),
            Stmt::Return(Some(Expr::LocalGet(5))),
        ],
        vec![3],
    );
    let factory = closure(
        10,
        vec![param(2)],
        vec![
            binding(3, Expr::Object(vec![("tag".into(), Expr::LocalGet(2))])),
            Stmt::Return(Some(inner)),
        ],
        vec![],
    );
    m.init = vec![
        binding(1, factory),
        binding(6, call(1, vec![Expr::String("a".into())])),
        binding(7, call(1, vec![Expr::String("b".into())])),
    ];
    m
}

#[test]
fn repeated_run_once_calls_get_distinct_bindings_and_closure_abis() {
    let mut m = module();
    m.closure_display_names.insert(20, "returned".into());
    let r = specialize_prototype_factories(&mut m, true);
    assert_eq!(r.expanded, 2);
    let mut closures = vec![];
    for s in &m.init {
        if let Stmt::Let {
            id: 6 | 7,
            init:
                Some(Expr::Closure {
                    func_id,
                    params,
                    captures,
                    body,
                    ..
                }),
            ..
        } = s
        {
            let Stmt::Let {
                init: Some(Expr::ObjectCreate(proto, _)),
                ..
            } = &body[0]
            else {
                panic!("birth")
            };
            assert!(matches!(proto.as_ref(), Expr::LocalGet(id) if captures == &vec![*id]));
            assert!(params[0].id > 7);
            assert_eq!(
                m.closure_display_names.get(func_id).map(String::as_str),
                Some("returned")
            );
            closures.push((*func_id, params[0].id, captures[0]));
        }
    }
    assert_eq!(closures.len(), 2);
    assert_ne!(closures[0].0, closures[1].0);
    assert_ne!(closures[0].1, closures[1].1);
    assert_ne!(closures[0].2, closures[1].2);
}

#[test]
fn capability_and_repeatable_positions_are_refused() {
    let mut m = module();
    let original = format!("{:?}", m.init);
    let r = specialize_prototype_factories(&mut m, false);
    assert_eq!(r.expanded, 0);
    assert_eq!(r.refused["module-capability"], 2);
    assert_eq!(original, format!("{:?}", m.init));
    let calls = m.init.split_off(1);
    m.init.push(Stmt::While {
        condition: Expr::Bool(false),
        body: calls,
    });
    let original = format!("{:?}", m.init);
    assert_eq!(specialize_prototype_factories(&mut m, true).expanded, 0);
    assert_eq!(original, format!("{:?}", m.init));
}

#[test]
fn argument_effects_are_materialized_once_in_source_order() {
    let mut m = module();
    let Stmt::Let {
        init: Some(Expr::Closure { params, .. }),
        ..
    } = &mut m.init[0]
    else {
        unreachable!()
    };
    params.push(param(8));
    m.init.truncate(2);
    let Stmt::Let {
        init: Some(Expr::Call { args, .. }),
        ..
    } = &mut m.init[1]
    else {
        unreachable!()
    };
    *args = vec![call(91, vec![]), call(92, vec![])];
    assert_eq!(specialize_prototype_factories(&mut m, true).expanded, 1);
    for (s, callee) in m.init[1..3].iter().zip([91, 92]) {
        assert!(
            matches!(s, Stmt::Let { init: Some(Expr::Call { callee: c, .. }), .. } if matches!(c.as_ref(), Expr::LocalGet(id) if *id == callee))
        );
    }
}

#[test]
fn own_dynamic_receiver_and_arity_are_preserved() {
    let mut m = module();
    let Stmt::Let {
        init: Some(Expr::Closure { body, .. }),
        ..
    } = &mut m.init[0]
    else {
        unreachable!()
    };
    let Stmt::Let {
        init: Some(Expr::Object(entries)),
        ..
    } = &mut body[0]
    else {
        unreachable!()
    };
    entries.push(("receiver".into(), Expr::This));
    assert_eq!(specialize_prototype_factories(&mut m, true).expanded, 0);
    let mut m = module();
    let Stmt::Let {
        init: Some(Expr::Call { args, .. }),
        ..
    } = &mut m.init[1]
    else {
        unreachable!()
    };
    args.clear();
    let r = specialize_prototype_factories(&mut m, true);
    assert_eq!(r.expanded, 1);
    assert_eq!(r.refused["arity"], 1);
}
