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
                field_writeback: None,
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
        field_writeback: None,
    });
    first.body[2] = Stmt::Return(Some(Expr::LocalGet(20)));

    let mut second = make_simple_producer();
    second.id = 1;
    second.name = "first".to_string();

    let mut module = Module::new("m");
    // #8104: both producers need a fuse site to clear the profitability gate.
    // The subject is the ID ASSIGNMENT ORDER, which is `max_local_id + 1`
    // bumped in ascending FuncId order.
    module.functions = vec![
        first,
        second,
        fuse_consumer(3, 1, 100),
        fuse_consumer(4, 2, 200),
    ];

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
    // Ids come from `max_local_id(module) + 1`, bumped in ascending FuncId
    // order. The highest local in the module is `fuse_consumer(4, 2, 200)`'s
    // `j` = 202, so the producers get 203 (FuncId 1) and 204 (FuncId 2). The
    // SUBJECT is the ORDER, so assert that explicitly too.
    let p1 = func1.params.last().unwrap().id;
    let p2 = func2.params.last().unwrap().id;
    assert_eq!(p1, 203);
    assert_eq!(p2, 204);
    assert_eq!(p2, p1 + 1, "ids are assigned in ascending FuncId order");
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
                byte_offset: 0,
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
    // #8104: the profitability gate needs one fuse site somewhere in the
    // module; the SUBJECT here is the plain caller's rewrite, not the fuse.
    module.functions = vec![helper, caller, fuse_consumer(3, 1, 100)];

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
                byte_offset: 0,
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
        alloc_width_hint: 0,
        specialized_from: None,
        aliases: Vec::new(),
    };

    let mut module = Module::new("m");
    // #8104: one fuse site so the producer clears the profitability gate —
    // the SUBJECT here is the class-member call site's rewrite.
    module.functions = vec![helper, fuse_consumer(3, 1, 100)];
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
    // `let v = []; v = helper(v);`, so the surviving call is a `Stmt::Expr`
    // wrapping a `LocalSet` whose rhs passes the accumulator. A stale `[0]`
    // here is exactly the miscompile.
    //
    // #7661 moved the call one level deeper (inside the `LocalSet`). Unwrapping
    // it is not cosmetic: before, this walk found the call directly under
    // `Stmt::Expr`, and a walk that did NOT unwrap would silently collect
    // nothing and compare `[] == [1]` — a failure, which is the safe
    // direction, but only because the expectation is non-empty.
    let method_after = &module.classes[0].methods[0];
    let mut arities = Vec::new();
    for stmt in &method_after.body {
        let init = match stmt {
            Stmt::Let { init: Some(e), .. } => Some(e),
            Stmt::Expr(e) | Stmt::Throw(e) => Some(e),
            Stmt::Return(Some(e)) => Some(e),
            _ => None,
        };
        let init = match init {
            Some(Expr::LocalSet(_, rhs)) => Some(rhs.as_ref()),
            other => other,
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

// ---------------------------------------------------------------------------
// #7661: the producer must hand its (possibly relocated) head back, and every
// call site must store it over the caller's binding.
//
// `js_array_grow` does not grow in place — it allocates elsewhere, copies, and
// leaves a FORWARDING STUB at the old address. The producer's push write-back
// re-points its own out-param slot; nothing re-points the caller's binding. So
// before this fix `const keep = build(1000)` left `keep` holding a stub as soon
// as the array outgrew `MIN_ARRAY_CAPACITY` (16) — invisible through the
// runtime, which resolves the chain at every entry point, and fatal to emitted
// code that dereferences the head (the #7612 / #7660 SIGBUS, at N = 17).
//
// These assert the HIR shape rather than behaviour on purpose: behaviour cannot
// see it. Every runtime path resolves the chain and prints the right answer
// either way, which is exactly why it went unnoticed.
// ---------------------------------------------------------------------------

/// A module with `function f() { const out = []; out.push(1); return out; }`
/// and a caller `function g() { const v = f(); return v; }`.
fn producer_and_plain_caller() -> Module {
    let caller = Function {
        id: 2,
        name: "g".to_string(),
        type_params: vec![],
        params: vec![],
        return_type: Type::Array(Box::new(Type::Number)),
        body: vec![
            Stmt::Let {
                id: 30,
                name: "v".to_string(),
                ty: Type::Array(Box::new(Type::Number)),
                mutable: false,
                init: Some(Expr::Call {
                    callee: Box::new(Expr::FuncRef(1)),
                    args: vec![],
                    type_args: vec![],
                    byte_offset: 0,
                }),
            },
            Stmt::Return(Some(Expr::LocalGet(30))),
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
    // #8104: one fuse site so the producer clears the profitability gate —
    // these fixtures are about the PLAIN call site's rewrite (#7661).
    module.functions = vec![make_simple_producer(), caller, fuse_consumer(3, 1, 100)];
    module
}

#[test]
fn producer_returns_the_out_param_not_undefined() {
    let mut module = producer_and_plain_caller();
    run(&mut module);

    let producer = module.functions.iter().find(|f| f.id == 1).unwrap();
    let out_param = producer.params.last().expect("synthetic param").id;
    match producer.body.last() {
        Some(Stmt::Return(Some(Expr::LocalGet(id)))) => assert_eq!(
            *id, out_param,
            "the producer must return its out-param — the head AFTER every \
             realloc write-back — not the caller's pre-growth pointer"
        ),
        other => panic!(
            "producer must end in `return <out_param>`, got {other:?}. Dropping \
             the return (the pre-#7661 behaviour) leaves the caller holding a \
             growth-forwarding stub."
        ),
    }
}

#[test]
fn plain_call_site_stores_the_returned_head_back_over_the_binding() {
    let mut module = producer_and_plain_caller();
    run(&mut module);

    let caller = module.functions.iter().find(|f| f.id == 2).unwrap();
    // `let v = []` then `v = f(v)`.
    match &caller.body[0] {
        Stmt::Let {
            id,
            init: Some(Expr::Array(elems)),
            mutable,
            ..
        } => {
            assert_eq!(*id, 30);
            assert!(elems.is_empty(), "binding is seeded with an empty literal");
            assert!(
                *mutable,
                "the binding is written twice now (literal, then the returned \
                 live head), so it must be mutable even though the source said \
                 `const`"
            );
        }
        other => panic!("expected the seeding `let`, got {other:?}"),
    }
    match &caller.body[1] {
        Stmt::Expr(Expr::LocalSet(id, rhs)) => {
            assert_eq!(
                *id, 30,
                "the returned head must be stored back over the SAME binding"
            );
            match rhs.as_ref() {
                Expr::Call { callee, args, .. } => {
                    assert!(matches!(callee.as_ref(), Expr::FuncRef(1)));
                    assert_eq!(args.len(), 1, "the binding is passed as the out-param");
                    assert!(matches!(args[0], Expr::LocalGet(30)));
                }
                other => panic!("expected the producer call, got {other:?}"),
            }
        }
        other => panic!(
            "expected `v = f(v)`, got {other:?}. A bare `Stmt::Expr(Call)` here \
             is the #7661 bug: the call grows the array, growth relocates, and \
             the binding keeps the stub."
        ),
    }
}

#[test]
fn rejects_deforest_when_class_method_uses_super() {
    // Regression for #5780 cluster A / #5772. A producer called from a
    // class method that also uses `super.prop` must NOT be deforested —
    // the call-site rewrite introduces synthetic locals that corrupt the
    // method's [[HomeObject]] setup, causing `super.x` to throw at
    // runtime ("Cannot convert undefined or null to object").
    //
    //   function helper() { const out = []; out.push(1); return out; }
    //   class C extends Base {
    //     m() { const v = helper(); return super.foo; }
    //   }
    let helper = make_simple_producer(); // id=1, the producer

    let method_with_super = Function {
        id: 2,
        name: "m".to_string(),
        type_params: vec![],
        params: vec![],
        return_type: Type::Any,
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
            // return super.foo;
            Stmt::Return(Some(Expr::SuperPropertyGet {
                property: "foo".to_string(),
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
        methods: vec![method_with_super],
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
        alloc_width_hint: 0,
        specialized_from: None,
        aliases: Vec::new(),
    };

    let mut module = Module::new("m");
    module.functions = vec![helper];
    module.classes = vec![class];

    // Detection must exclude the producer — the super-using body makes
    // it unsafe to deforest.
    assert!(
        detect_producers(&module).is_empty(),
        "producer called from a super-using method must not be deforested (#5780)"
    );

    // run() must leave the producer's signature untouched.
    run(&mut module);
    let helper_after = module.functions.iter().find(|f| f.id == 1).unwrap();
    assert!(
        helper_after.params.is_empty(),
        "producer signature must be unchanged when its call site is in a super-using method"
    );
}

#[test]
fn still_deforests_when_method_has_no_super() {
    // Control for `rejects_deforest_when_class_method_uses_super`: the
    // SAME producer called from a class method that does NOT use super
    // is still deforested (the existing #5772 fix must not regress).
    //
    // This is the same scenario as `deforests_producer_called_from_class_method`
    // but added here for symmetry with the super test above.
    let helper = make_simple_producer(); // id=1

    let plain_method = Function {
        id: 2,
        name: "m".to_string(),
        type_params: vec![],
        params: vec![],
        return_type: Type::Any,
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
                byte_offset: 0,
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
        methods: vec![plain_method],
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
        alloc_width_hint: 0,
        specialized_from: None,
        aliases: Vec::new(),
    };

    let mut module = Module::new("m");
    // #8104: one fuse site so the producer clears the profitability gate —
    // the SUBJECT here is the class-member call site's rewrite.
    module.functions = vec![helper, fuse_consumer(3, 1, 100)];
    module.classes = vec![class];

    assert!(
        !detect_producers(&module).is_empty(),
        "producer with a super-free class-method caller should still deforest"
    );

    run(&mut module);
    let helper_after = module.functions.iter().find(|f| f.id == 1).unwrap();
    assert_eq!(
        helper_after.params.len(),
        1,
        "producer should gain the synthetic accumulator param"
    );
}

// ── #8104: profitability ───────────────────────────────────────────────────

#[test]
fn a_producer_with_no_fuse_site_is_left_alone() {
    // `const arr = build();` with no consume loop anywhere. The value-binding
    // rewrite would turn it into `let arr = []; arr = build(arr);` — the same
    // one array, but now behind a REASSIGNED binding, which costs every
    // representation fact keyed on a write-once local. On
    // `benchmarks/suite/bench_numeric_array_numeric.ts` that is +2926%
    // instructions and +103% peak RSS for a transform that removed nothing.
    let mut module = producer_and_plain_caller();
    // Drop the fuse site `producer_and_plain_caller` adds for the OTHER tests.
    module.functions.retain(|f| f.id != 3);

    assert!(
        !detect_producers(&module).is_empty(),
        "the shape still DETECTS as a producer — the refusal is profitability, \
         not detection, and this test is vacuous if detection already declines"
    );

    run(&mut module);

    let producer = module.functions.iter().find(|f| f.id == 1).unwrap();
    assert!(
        producer.params.is_empty(),
        "a producer with no consumer-fuse call site must keep its signature"
    );
    let caller = module.functions.iter().find(|f| f.id == 2).unwrap();
    assert!(
        matches!(
            &caller.body[0],
            Stmt::Let {
                mutable: false,
                init: Some(Expr::Call { .. }),
                ..
            }
        ),
        "the caller's binding must stay write-once and keep its direct call, \
         got {:?}",
        caller.body[0]
    );
}

#[test]
fn one_fuse_site_admits_the_producer_at_every_call_site() {
    // The discriminating half: the SAME module, plus one fuse site, and the
    // producer is transformed — including its non-fuse call site, which must
    // move in lock-step because the signature gained a parameter.
    let mut module = producer_and_plain_caller();
    run(&mut module);

    let producer = module.functions.iter().find(|f| f.id == 1).unwrap();
    assert_eq!(
        producer.params.len(),
        1,
        "one fuse site anywhere in the module admits the producer"
    );
    let caller = module.functions.iter().find(|f| f.id == 2).unwrap();
    assert!(
        matches!(&caller.body[1], Stmt::Expr(Expr::LocalSet(30, _))),
        "the non-fuse call site must be rewritten too, or its arity no longer \
         matches the producer's signature (#5136's SIGSEGV shape)"
    );
}

#[test]
fn the_recursive_fuse_inside_a_producer_body_counts() {
    // ABC451D — the shape the transform was built for. `f` consumes its OWN
    // recursive result into `out`, and that fuse lives inside the producer
    // body, not at an external call site. The gate must see it, or the one
    // workload deforestation demonstrably wins on stops being deforested.
    let mut producer = make_simple_producer();
    // function f() {
    //   const out = [];
    //   out.push(1);
    //   const child = f();
    //   for (let j = 0; j < child.length; j++) out.push(child[j]);
    //   return out;
    // }
    producer.body.insert(
        2,
        Stmt::Let {
            id: 40,
            name: "child".to_string(),
            ty: Type::Array(Box::new(Type::Number)),
            mutable: false,
            init: Some(Expr::Call {
                callee: Box::new(Expr::FuncRef(1)),
                args: vec![],
                type_args: vec![],
                byte_offset: 0,
            }),
        },
    );
    producer.body.insert(
        3,
        Stmt::For {
            init: Some(Box::new(Stmt::Let {
                id: 41,
                name: "j".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Integer(0)),
            })),
            condition: Some(Expr::Compare {
                op: perry_hir::CompareOp::Lt,
                left: Box::new(Expr::LocalGet(41)),
                right: Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(40)),
                    property: "length".to_string(),
                    byte_offset: 0,
                }),
            }),
            update: Some(Expr::Update {
                id: 41,
                op: perry_hir::UpdateOp::Increment,
                prefix: false,
            }),
            body: vec![Stmt::Expr(Expr::ArrayPush {
                array_id: 10,
                value: Box::new(Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(40)),
                    index: Box::new(Expr::LocalGet(41)),
                }),
                field_writeback: None,
            })],
        },
    );

    let mut module = Module::new("m");
    module.functions = vec![producer];
    run(&mut module);

    let after = module.functions.iter().find(|f| f.id == 1).unwrap();
    assert_eq!(
        after.params.len(),
        1,
        "a recursive fuse inside the producer's own body must admit it"
    );
}

/// #8104: a function containing ONE consumer-fuse call site for `producer`.
///
/// `run` now refuses producers with no fuse site, because the fuse is the only
/// call-site shape that removes work (the value-binding rewrite reallocates
/// nothing and costs the caller's binding its write-once proof — measured at
/// +2926% instructions and +103% peak RSS on
/// `benchmarks/suite/bench_numeric_array_numeric.ts`). Fixtures whose SUBJECT
/// is a different call-site shape therefore need one of these in the module so
/// the producer still qualifies.
///
/// Deliberately does NOT end in `return outer` — that would make the consumer
/// a producer too, and shift the synthetic-parameter numbering the callers of
/// this helper assert on.
fn fuse_consumer(id: FuncId, producer: FuncId, base: LocalId) -> Function {
    let outer = base;
    let child = base + 1;
    let j = base + 2;
    Function {
        id,
        name: format!("fuse_consumer_{id}"),
        type_params: vec![],
        params: vec![],
        return_type: Type::Number,
        body: vec![
            Stmt::Let {
                id: outer,
                name: "outer".to_string(),
                ty: Type::Array(Box::new(Type::Number)),
                mutable: false,
                init: Some(Expr::Array(vec![])),
            },
            Stmt::Let {
                id: child,
                name: "child".to_string(),
                ty: Type::Array(Box::new(Type::Number)),
                mutable: false,
                init: Some(Expr::Call {
                    callee: Box::new(Expr::FuncRef(producer)),
                    args: vec![],
                    type_args: vec![],
                    byte_offset: 0,
                }),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: j,
                    name: "j".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: perry_hir::CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(j)),
                    right: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(child)),
                        property: "length".to_string(),
                        byte_offset: 0,
                    }),
                }),
                update: Some(Expr::Update {
                    id: j,
                    op: perry_hir::UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::Expr(Expr::ArrayPush {
                    array_id: outer,
                    value: Box::new(Expr::IndexGet {
                        object: Box::new(Expr::LocalGet(child)),
                        index: Box::new(Expr::LocalGet(j)),
                    }),
                    field_writeback: None,
                })],
            },
            Stmt::Return(Some(Expr::Integer(0))),
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
                field_writeback: None,
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
