use super::*;

// Sanity tests at the helper level. End-to-end tests live in
// test-files/test_deforest_*.ts (compiled + run vs Node).

#[test]
fn detects_simple_producer() {
    // function f() { const out = []; out.push(1); return out; }
    let func = Function {
        id: 1,
        name: "f".to_string(),
        type_params: vec![],
        params: vec![],
        return_type: Type::Array(Box::new(Type::Number)),
        body: vec![
            Stmt::Let {
                id: 10,
                name: "out".to_string(),
                ty: Type::Array(Box::new(Type::Number)),
                mutable: false,
                init: Some(Expr::Array(vec![])),
            },
            Stmt::Expr(Expr::ArrayPush {
                array_id: 10,
                value: Box::new(Expr::Integer(1)),
            }),
            Stmt::Return(Some(Expr::LocalGet(10))),
        ],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    };
    let info = analyze_producer(&func).expect("should detect producer");
    assert_eq!(info.out_local_id, 10);
    assert_eq!(info.original_param_count, 0);
    assert!(matches!(info.elem_ty, Type::Number));
}

#[test]
fn rejects_async_producer() {
    let mut func = make_simple_producer();
    func.is_async = true;
    assert!(analyze_producer(&func).is_none());
}

#[test]
fn rejects_producer_with_out_passed_to_call() {
    // function f() { const out = []; helper(out); return out; }
    // Passing `out` to `helper` is unsafe — it might escape.
    let mut func = make_simple_producer();
    // Replace the push with `helper(out)`.
    func.body[1] = Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(99)),
        args: vec![Expr::LocalGet(10)],
        type_args: vec![],
        byte_offset: 0,
    });
    assert!(analyze_producer(&func).is_none());
}

#[test]
fn rejects_producer_with_reassignment() {
    // function f() { const out = []; out = [1, 2]; return out; }
    let mut func = make_simple_producer();
    func.body[1] = Stmt::Expr(Expr::LocalSet(
        10,
        Box::new(Expr::Array(vec![Expr::Integer(1)])),
    ));
    assert!(analyze_producer(&func).is_none());
}

#[test]
fn rejects_producer_with_multiple_returns() {
    // function f(cond) { const out = []; if (cond) return []; return out; }
    let mut func = make_simple_producer();
    func.body.insert(
        1,
        Stmt::If {
            condition: Expr::Bool(true),
            then_branch: vec![Stmt::Return(Some(Expr::Array(vec![])))],
            else_branch: None,
        },
    );
    assert!(analyze_producer(&func).is_none());
}

#[test]
fn synthetic_out_params_are_assigned_by_function_id() {
    let mut first = make_simple_producer();
    first.id = 2;
    first.name = "second".to_string();
    first.body[0] = Stmt::Let {
        id: 20,
        name: "out2".to_string(),
        ty: Type::Array(Box::new(Type::Number)),
        mutable: false,
        init: Some(Expr::Array(vec![])),
    };
    first.body[1] = Stmt::Expr(Expr::ArrayPush {
        array_id: 20,
        value: Box::new(Expr::Integer(1)),
    });
    first.body[2] = Stmt::Return(Some(Expr::LocalGet(20)));

    let mut second = make_simple_producer();
    second.id = 1;
    second.name = "first".to_string();

    let mut module = Module::new("m");
    module.functions = vec![first, second];

    run(&mut module);

    let func1 = module
        .functions
        .iter()
        .find(|func| func.id == 1)
        .expect("function must exist");
    let func2 = module
        .functions
        .iter()
        .find(|func| func.id == 2)
        .expect("function must exist");
    assert_eq!(func1.params.last().unwrap().id, 21);
    assert_eq!(func2.params.last().unwrap().id, 22);
}

#[test]
fn rejects_producer_called_inside_closure() {
    // Refs #5136. A producer whose ONLY call site lives inside a
    // closure body must NOT be deforested: the call-site rewriter
    // never descends into closures, so rewriting the producer's
    // signature (adding the +1 accumulator param) while the in-closure
    // call keeps the original arity miscompiles to a SIGSEGV.
    //
    //   function helper() { const out = []; out.push(1); return out; }
    //   function factory() {
    //     const generate = () => { const v = helper(); return v.length; };
    //     return generate;
    //   }
    let helper = make_simple_producer(); // id=1, the producer

    let closure = Expr::Closure {
        func_id: 2,
        params: vec![],
        return_type: Type::Number,
        body: vec![
            // const v = helper();
            Stmt::Let {
                id: 30,
                name: "v".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::Call {
                    callee: Box::new(Expr::FuncRef(1)),
                    args: vec![],
                    type_args: vec![],
                    byte_offset: 0,
                }),
            },
            // return v.length;
            Stmt::Return(Some(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(30)),
                property: "length".to_string(),
            })),
        ],
        captures: vec![],
        mutable_captures: vec![],
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: false,
    };

    let factory = Function {
        id: 3,
        name: "factory".to_string(),
        type_params: vec![],
        params: vec![],
        return_type: Type::Any,
        body: vec![
            Stmt::Let {
                id: 31,
                name: "generate".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(closure),
            },
            Stmt::Return(Some(Expr::LocalGet(31))),
        ],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    };

    let mut module = Module::new("m");
    module.functions = vec![helper, factory];

    // Detection must drop the producer entirely.
    assert!(
        detect_producers(&module).is_empty(),
        "producer called inside a closure must not be deforested"
    );

    // And `run` must leave the producer's signature untouched (no
    // synthetic accumulator param added).
    run(&mut module);
    let helper_after = module.functions.iter().find(|f| f.id == 1).unwrap();
    assert!(
        helper_after.params.is_empty(),
        "producer signature must be unchanged when only called from a closure"
    );
}

#[test]
fn still_deforests_when_caller_is_not_a_closure() {
    // Control for `rejects_producer_called_inside_closure`: the SAME
    // producer, but called from a plain statement, is still rewritten.
    //
    //   function helper() { const out = []; out.push(1); return out; }
    //   function caller() { const v = helper(); /* ...used... */ }
    let helper = make_simple_producer(); // id=1

    let caller = Function {
        id: 2,
        name: "caller".to_string(),
        type_params: vec![],
        params: vec![],
        return_type: Type::Any,
        body: vec![Stmt::Let {
            id: 30,
            name: "v".to_string(),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::Call {
                callee: Box::new(Expr::FuncRef(1)),
                args: vec![],
                type_args: vec![],
                byte_offset: 0,
            }),
        }],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    };

    let mut module = Module::new("m");
    module.functions = vec![helper, caller];

    assert!(
        !detect_producers(&module).is_empty(),
        "producer with a plain (non-closure) caller should still deforest"
    );

    run(&mut module);
    let helper_after = module.functions.iter().find(|f| f.id == 1).unwrap();
    assert_eq!(
        helper_after.params.len(),
        1,
        "producer should gain the synthetic accumulator param"
    );
}

#[test]
fn deforests_producer_called_from_class_method() {
    // Regression: a producer called via `let v = helper()` inside a CLASS
    // METHOD must have its call site rewritten in lock-step with the
    // producer's signature. `detect_producers` already scans method bodies,
    // so it admits the producer — but before phase-3 covered class member
    // bodies the method's call site kept its original 0-arg form while
    // `helper` gained the `__deforest_out` param. Codegen then passed
    // `undefined` for the missing arg and the body operated on a non-array,
    // SIGSEGVing (same arity-mismatch class as the in-closure bail, #5136).
    //
    //   function helper() { const out = []; out.push(1); return out; }
    //   class C { m() { const v = helper(); return v.length; } }
    let helper = make_simple_producer(); // id=1, the producer

    let method = Function {
        id: 2,
        name: "m".to_string(),
        type_params: vec![],
        params: vec![],
        return_type: Type::Number,
        body: vec![
            Stmt::Let {
                id: 30,
                name: "v".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::Call {
                    callee: Box::new(Expr::FuncRef(1)),
                    args: vec![],
                    type_args: vec![],
                    byte_offset: 0,
                }),
            },
            Stmt::Return(Some(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(30)),
                property: "length".to_string(),
            })),
        ],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    };

    let class = perry_hir::Class {
        id: 10,
        name: "C".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: Vec::new(),
        constructor: None,
        methods: vec![method],
        getters: Vec::new(),
        setters: Vec::new(),
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        computed_members: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        is_nested: false,
        aliases: Vec::new(),
    };

    let mut module = Module::new("m");
    module.functions = vec![helper];
    module.classes = vec![class];

    assert!(
        !detect_producers(&module).is_empty(),
        "producer with a class-method caller should still deforest"
    );

    run(&mut module);

    let helper_after = module.functions.iter().find(|f| f.id == 1).unwrap();
    assert_eq!(
        helper_after.params.len(),
        1,
        "producer should gain the synthetic accumulator param"
    );

    // Every call to the producer (id=1) in the method body must now match
    // the rewritten arity (1). The rewrite turns `let v = helper()` into
    // `let v = []; helper(v);`, so the surviving call is a `Stmt::Expr`
    // passing the accumulator. A stale `[0]` here is exactly the miscompile.
    let method_after = &module.classes[0].methods[0];
    let mut arities = Vec::new();
    for stmt in &method_after.body {
        let init = match stmt {
            Stmt::Let { init: Some(e), .. } => Some(e),
            Stmt::Expr(e) | Stmt::Throw(e) => Some(e),
            Stmt::Return(Some(e)) => Some(e),
            _ => None,
        };
        if let Some(Expr::Call { callee, args, .. }) = init {
            if matches!(callee.as_ref(), Expr::FuncRef(1)) {
                arities.push(args.len());
            }
        }
    }
    assert_eq!(
        arities,
        vec![1],
        "the method's producer call site must be rewritten to pass the out-param"
    );
}

fn make_simple_producer() -> Function {
    Function {
        id: 1,
        name: "f".to_string(),
        type_params: vec![],
        params: vec![],
        return_type: Type::Array(Box::new(Type::Number)),
        body: vec![
            Stmt::Let {
                id: 10,
                name: "out".to_string(),
                ty: Type::Array(Box::new(Type::Number)),
                mutable: false,
                init: Some(Expr::Array(vec![])),
            },
            Stmt::Expr(Expr::ArrayPush {
                array_id: 10,
                value: Box::new(Expr::Integer(1)),
            }),
            Stmt::Return(Some(Expr::LocalGet(10))),
        ],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    }
}
