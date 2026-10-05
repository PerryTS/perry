use super::*;
use perry_hir::types::Type;
use perry_hir::{Function, Module, TYPED_ARRAY_KIND_FLOAT64, TYPED_ARRAY_KIND_INT32};

fn let_stmt(id: u32, mutable: bool, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: format!("v{id}"),
        ty: Type::Any,
        mutable,
        init: Some(init),
    }
}

fn ta_new(kind: u8, arg: Option<Expr>) -> Expr {
    Expr::TypedArrayNew {
        kind,
        arg: arg.map(Box::new),
    }
}

fn call(fid: u32, args: Vec<Expr>) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::FuncRef(fid)),
        args,
        type_args: vec![],
        byte_offset: 0,
    }
}

fn func(id: u32, body: Vec<Stmt>) -> Function {
    Function {
        id,
        name: format!("f{id}"),
        type_params: vec![],
        params: vec![],
        return_type: Type::Any,
        body,
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

fn module_with_init(init: Vec<Stmt>) -> Module {
    let mut m = Module::new("spec_abi_test");
    m.init = init;
    add_callee_stubs(&mut m);
    m
}

/// Every `FuncRef` callee resolves to a module function, as in a real module:
/// a missing callee gets a stub whose parameters are never used, so passing a
/// typed array to it keeps the binding sealed (`collectors/sealed_buffers.rs`).
fn add_callee_stubs(m: &mut Module) {
    fn visit_expr(e: &Expr, out: &mut HashMap<u32, usize>) {
        if let Expr::Call { callee, args, .. } = e {
            if let Expr::FuncRef(f) = callee.as_ref() {
                let n = out.entry(*f).or_insert(0);
                *n = (*n).max(args.len());
            }
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| visit_expr(c, out));
    }
    fn visit_stmts(stmts: &[Stmt], out: &mut HashMap<u32, usize>) {
        for s in stmts {
            match s {
                Stmt::Let { init: Some(e), .. }
                | Stmt::Expr(e)
                | Stmt::Return(Some(e))
                | Stmt::Throw(e) => visit_expr(e, out),
                Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    visit_expr(condition, out);
                    visit_stmts(then_branch, out);
                    if let Some(eb) = else_branch {
                        visit_stmts(eb, out);
                    }
                }
                Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                    visit_expr(condition, out);
                    visit_stmts(body, out);
                }
                Stmt::For {
                    init,
                    condition,
                    update,
                    body,
                } => {
                    if let Some(i) = init {
                        visit_stmts(std::slice::from_ref(i.as_ref()), out);
                    }
                    for e in condition.iter().chain(update.iter()) {
                        visit_expr(e, out);
                    }
                    visit_stmts(body, out);
                }
                _ => {}
            }
        }
    }
    let mut callees = HashMap::new();
    visit_stmts(&m.init, &mut callees);
    for (id, arity) in callees {
        if m.functions.iter().any(|f| f.id == id) {
            continue;
        }
        let mut stub = func(id, vec![]);
        stub.params = (0..arity as u32)
            .map(|i| perry_hir::Param {
                id: 100_000 + id * 100 + i,
                name: format!("s{id}_{i}"),
                ty: Type::Any,
                default: None,
                decorators: vec![],
                is_rest: false,
                arguments_object: None,
            })
            .collect();
        m.functions.push(stub);
    }
}

#[test]
fn literal_length_binding_and_dominated_site() {
    // const P = new Int32Array(4); f(P, 0, 1.5);
    let m = module_with_init(vec![
        let_stmt(
            10,
            false,
            ta_new(TYPED_ARRAY_KIND_INT32, Some(Expr::Integer(4))),
        ),
        Stmt::Expr(call(
            7,
            vec![Expr::LocalGet(10), Expr::Integer(0), Expr::Number(1.5)],
        )),
    ]);
    let facts = collect_spec_abi_facts(&m);
    let b = facts.ta_bindings.get(&10).expect("binding proven");
    assert_eq!(b.kind, TYPED_ARRAY_KIND_INT32);
    assert_eq!(b.const_len, Some(4));
    let sites = facts.call_sites.get(&7).expect("site judged");
    assert_eq!(
        sites[0],
        vec![
            SpecParamRep::TaPtr {
                kind: TYPED_ARRAY_KIND_INT32,
                const_len: Some(4)
            },
            SpecParamRep::I32,
            SpecParamRep::F64,
        ]
    );
}

#[test]
fn literal_length_local_proves_typed_array_binding() {
    // #7221: const nodes = 10_000; const values = new Float64Array(nodes);
    // fill(values, nodes, dirty, frame). The constructor's numeric local is
    // still a non-view length and all integer locals can use raw i32 params.
    let m = module_with_init(vec![
        let_stmt(1, false, Expr::Integer(10_000)),
        let_stmt(
            2,
            false,
            ta_new(TYPED_ARRAY_KIND_FLOAT64, Some(Expr::LocalGet(1))),
        ),
        let_stmt(3, false, Expr::Integer(1_000)),
        let_stmt(4, true, Expr::Integer(0)),
        Stmt::Expr(Expr::Update {
            id: 4,
            op: perry_hir::UpdateOp::Increment,
            prefix: false,
        }),
        Stmt::Expr(call(
            7,
            vec![
                Expr::LocalGet(2),
                Expr::LocalGet(1),
                Expr::LocalGet(3),
                Expr::LocalGet(4),
            ],
        )),
    ]);
    let facts = collect_spec_abi_facts(&m);
    assert_eq!(
        facts.ta_bindings.get(&2).map(|binding| binding.const_len),
        Some(Some(10_000))
    );
    assert_eq!(
        facts.call_sites.get(&7).unwrap()[0],
        vec![
            SpecParamRep::TaPtr {
                kind: TYPED_ARRAY_KIND_FLOAT64,
                const_len: Some(10_000)
            },
            SpecParamRep::I32,
            SpecParamRep::I32,
            SpecParamRep::I32,
        ]
    );
}

#[test]
fn mutable_literal_length_local_does_not_prove_typed_array_binding() {
    // A mutable length slot can stop being numeric before construction, so
    // this provenance shortcut is deliberately limited to `const` bindings.
    let m = module_with_init(vec![
        let_stmt(1, true, Expr::Integer(10_000)),
        let_stmt(
            2,
            false,
            ta_new(TYPED_ARRAY_KIND_FLOAT64, Some(Expr::LocalGet(1))),
        ),
        Stmt::Expr(call(7, vec![Expr::LocalGet(2)])),
    ]);
    let facts = collect_spec_abi_facts(&m);
    assert!(!facts.ta_bindings.contains_key(&2));
    assert_eq!(
        facts.call_sites.get(&7).unwrap()[0],
        vec![SpecParamRep::Boxed]
    );
}

#[test]
fn local_with_non_integer_write_stays_boxed_at_spec_call() {
    let m = module_with_init(vec![
        let_stmt(1, true, Expr::Integer(7)),
        Stmt::Expr(Expr::LocalSet(
            1,
            Box::new(Expr::String("not an integer".to_string())),
        )),
        Stmt::Expr(call(7, vec![Expr::LocalGet(1)])),
    ]);
    let facts = collect_spec_abi_facts(&m);
    assert_eq!(
        facts.call_sites.get(&7).unwrap()[0],
        vec![SpecParamRep::Boxed]
    );
}

#[test]
fn array_literal_source_counts_elements() {
    // var A = [1,2,3]; const P = new Int32Array(A); f(P)
    let m = module_with_init(vec![
        let_stmt(
            1,
            true,
            Expr::Array(vec![Expr::Integer(1), Expr::Integer(2), Expr::Integer(3)]),
        ),
        let_stmt(
            2,
            false,
            ta_new(TYPED_ARRAY_KIND_INT32, Some(Expr::LocalGet(1))),
        ),
        Stmt::Expr(call(7, vec![Expr::LocalGet(2)])),
    ]);
    let facts = collect_spec_abi_facts(&m);
    assert_eq!(
        facts.ta_bindings.get(&2).map(|b| b.const_len),
        Some(Some(3))
    );
}

#[test]
fn unproven_ctor_arg_rejects_binding() {
    // Potential view form: `new Int32Array(x)` where x's provenance is
    // unknown (could be an ArrayBuffer) — must NOT prove.
    let m = module_with_init(vec![
        let_stmt(1, false, Expr::Undefined),
        let_stmt(
            2,
            false,
            ta_new(TYPED_ARRAY_KIND_INT32, Some(Expr::LocalGet(1))),
        ),
        Stmt::Expr(call(7, vec![Expr::LocalGet(2)])),
    ]);
    let facts = collect_spec_abi_facts(&m);
    assert!(!facts.ta_bindings.contains_key(&2));
    assert_eq!(
        facts.call_sites.get(&7).unwrap()[0],
        vec![SpecParamRep::Boxed]
    );
}

#[test]
fn reassignment_rejects_binding() {
    // let P = new Int32Array(4); P = undefined; f(P)
    let m = module_with_init(vec![
        let_stmt(
            3,
            true,
            ta_new(TYPED_ARRAY_KIND_INT32, Some(Expr::Integer(4))),
        ),
        Stmt::Expr(Expr::LocalSet(3, Box::new(Expr::Undefined))),
        Stmt::Expr(call(7, vec![Expr::LocalGet(3)])),
    ]);
    let facts = collect_spec_abi_facts(&m);
    assert!(!facts.ta_bindings.contains_key(&3));
}

#[test]
fn reassignment_in_another_function_rejects_binding() {
    // The write scan is module-wide: a reassignment hiding in a different
    // function body must disqualify the init-scope binding.
    let mut m = module_with_init(vec![
        let_stmt(
            3,
            true,
            ta_new(TYPED_ARRAY_KIND_INT32, Some(Expr::Integer(4))),
        ),
        Stmt::Expr(call(7, vec![Expr::LocalGet(3)])),
    ]);
    m.functions.retain(|f| f.id != 7);
    m.functions.push(func(
        7,
        vec![Stmt::Expr(Expr::LocalSet(3, Box::new(Expr::Undefined)))],
    ));
    let facts = collect_spec_abi_facts(&m);
    assert!(!facts.ta_bindings.contains_key(&3));
}

#[test]
fn closure_reference_rejects_binding() {
    let closure = Expr::Closure {
        func_id: 99,
        params: vec![],
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(Expr::LocalGet(3)))],
        captures: vec![3],
        mutable_captures: vec![],
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: false,
    };
    let m = module_with_init(vec![
        let_stmt(
            3,
            false,
            ta_new(TYPED_ARRAY_KIND_INT32, Some(Expr::Integer(4))),
        ),
        let_stmt(4, false, closure),
        Stmt::Expr(call(7, vec![Expr::LocalGet(3)])),
    ]);
    let facts = collect_spec_abi_facts(&m);
    assert!(!facts.ta_bindings.contains_key(&3));
}

#[test]
fn call_before_binding_is_not_dominated() {
    // f(P) textually BEFORE the Let: the sequential judgment must not prove.
    let m = module_with_init(vec![
        Stmt::Expr(call(7, vec![Expr::LocalGet(10)])),
        let_stmt(
            10,
            false,
            ta_new(TYPED_ARRAY_KIND_INT32, Some(Expr::Integer(4))),
        ),
        Stmt::Expr(call(7, vec![Expr::LocalGet(10)])),
    ]);
    let facts = collect_spec_abi_facts(&m);
    let sites = facts.call_sites.get(&7).unwrap();
    assert_eq!(sites[0], vec![SpecParamRep::Boxed]);
    assert!(matches!(sites[1][0], SpecParamRep::TaPtr { .. }));
}

#[test]
fn site_nested_in_loop_after_toplevel_let_is_proven() {
    // The enc_real shape: Lets, then `for (...) f(lr, 0, P, S)`.
    let m = module_with_init(vec![
        let_stmt(
            10,
            false,
            ta_new(TYPED_ARRAY_KIND_INT32, Some(Expr::Integer(2))),
        ),
        Stmt::While {
            condition: Expr::Bool(true),
            body: vec![Stmt::Expr(call(
                7,
                vec![Expr::LocalGet(10), Expr::Integer(0)],
            ))],
        },
    ]);
    let facts = collect_spec_abi_facts(&m);
    let sites = facts.call_sites.get(&7).unwrap();
    assert!(matches!(sites[0][0], SpecParamRep::TaPtr { .. }));
    assert_eq!(sites[0][1], SpecParamRep::I32);
}

#[test]
fn sites_inside_closures_are_never_judged() {
    let closure = Expr::Closure {
        func_id: 99,
        params: vec![],
        return_type: Type::Any,
        body: vec![Stmt::Expr(call(7, vec![Expr::Integer(1)]))],
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
    let m = module_with_init(vec![let_stmt(4, false, closure)]);
    let facts = collect_spec_abi_facts(&m);
    assert!(facts.call_sites.get(&7).is_none());
}

#[test]
fn length_unsafe_source_use_demotes_const_len_only() {
    // var A = [1,2]; g(A); const P = new Int32Array(A): A stays a proven
    // plain array (never reassigned) so P is still non-view, but its length
    // is no longer a compile-time constant.
    let m = module_with_init(vec![
        let_stmt(
            1,
            true,
            Expr::Array(vec![Expr::Integer(1), Expr::Integer(2)]),
        ),
        Stmt::Expr(call(8, vec![Expr::LocalGet(1)])),
        let_stmt(
            2,
            false,
            ta_new(TYPED_ARRAY_KIND_INT32, Some(Expr::LocalGet(1))),
        ),
        Stmt::Expr(call(7, vec![Expr::LocalGet(2)])),
    ]);
    let facts = collect_spec_abi_facts(&m);
    let b = facts.ta_bindings.get(&2).expect("still non-view-proven");
    assert_eq!(b.const_len, None);
}

#[test]
fn put_value_set_receiver_demotes_source_const_len() {
    let source = Expr::LocalGet(1);
    let m = module_with_init(vec![
        let_stmt(
            1,
            true,
            Expr::Array(vec![Expr::Integer(1), Expr::Integer(2)]),
        ),
        Stmt::Expr(Expr::PutValueSet {
            target: Box::new(source.clone()),
            key: Box::new(Expr::Integer(0)),
            value: Box::new(Expr::Integer(3)),
            receiver: Box::new(source),
            strict: false,
        }),
        let_stmt(
            2,
            false,
            ta_new(TYPED_ARRAY_KIND_INT32, Some(Expr::LocalGet(1))),
        ),
    ]);
    let facts = collect_spec_abi_facts(&m);
    assert_eq!(facts.ta_bindings.get(&2).map(|b| b.const_len), Some(None));
}

#[test]
fn bigint_kind_binding_is_rejected() {
    // BigInt64Array elements are BigInt, not Number — never a `TaPtr` binding.
    let m = module_with_init(vec![
        let_stmt(
            3,
            false,
            ta_new(perry_hir::TYPED_ARRAY_KIND_BIGINT64, Some(Expr::Integer(4))),
        ),
        Stmt::Expr(call(7, vec![Expr::LocalGet(3)])),
    ]);
    let facts = collect_spec_abi_facts(&m);
    assert!(!facts.ta_bindings.contains_key(&3));
    assert_eq!(
        facts.call_sites.get(&7).unwrap()[0],
        vec![SpecParamRep::Boxed]
    );
}

#[test]
fn boxed_prealloc_binding_is_rejected() {
    // A TDZ/prealloc-flagged id's slot holds a BOX pointer, not the value —
    // masking it would yield the box address, so it can never prove `TaPtr`.
    let m = module_with_init(vec![
        Stmt::PreallocateTdzBoxes(vec![3]),
        let_stmt(
            3,
            false,
            ta_new(TYPED_ARRAY_KIND_INT32, Some(Expr::Integer(4))),
        ),
        Stmt::Expr(call(7, vec![Expr::LocalGet(3)])),
    ]);
    let facts = collect_spec_abi_facts(&m);
    assert!(!facts.ta_bindings.contains_key(&3));
    assert_eq!(
        facts.call_sites.get(&7).unwrap()[0],
        vec![SpecParamRep::Boxed]
    );
}

#[test]
fn local_is_reassigned_sees_closure_writes() {
    let closure = Expr::Closure {
        func_id: 99,
        params: vec![],
        return_type: Type::Any,
        body: vec![Stmt::Expr(Expr::LocalSet(5, Box::new(Expr::Integer(1))))],
        captures: vec![5],
        mutable_captures: vec![5],
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: false,
    };
    let stmts = vec![let_stmt(4, false, closure)];
    assert!(local_is_reassigned(&stmts, 5));
    assert!(!local_is_reassigned(&stmts, 6));
}

#[test]
fn guarded_number_array_rejects_stale_or_aliasable_reads() {
    let read = || Expr::IndexGet {
        object: Box::new(Expr::LocalGet(5)),
        index: Box::new(Expr::Integer(0)),
    };
    let write = || {
        Stmt::Expr(Expr::IndexSet {
            object: Box::new(Expr::LocalGet(5)),
            index: Box::new(Expr::Integer(0)),
            value: Box::new(Expr::Integer(1)),
        })
    };
    let loop_stmt = || Stmt::While {
        condition: Expr::Bool(false),
        body: Vec::new(),
    };

    assert!(guarded_number_array_param_eligible(
        &[let_stmt(6, false, read()), loop_stmt(), write(),],
        5,
    ));
    assert!(!guarded_number_array_param_eligible(
        &[write(), loop_stmt(), Stmt::Return(Some(read()))],
        5,
    ));
    assert!(!guarded_number_array_param_eligible(
        &[
            let_stmt(6, false, read()),
            loop_stmt(),
            Stmt::Expr(call(9, Vec::new())),
        ],
        5,
    ));
}

#[test]
fn a_var_redeclared_parameter_is_demoted_like_a_reassigned_one() {
    // #11802: `function f(n) { var n = b; ... }` binds the parameter's id with
    // a `Stmt::Let`, not a `LocalSet`; the specialized entry's proofs about
    // the Number `n` arrived as must not survive either form.
    let param = |id: u32| perry_hir::Param {
        id,
        name: format!("p{id}"),
        ty: Type::Number,
        default: None,
        decorators: vec![],
        is_rest: false,
        arguments_object: None,
    };
    let demoted = |body: Vec<Stmt>| {
        let mut f = func(1, body);
        f.params = vec![param(1), param(2)];
        callee_demoted_params(&f)
    };
    assert_eq!(demoted(vec![]), [false, false]);
    assert_eq!(
        demoted(vec![Stmt::Expr(Expr::LocalSet(
            1,
            Box::new(Expr::LocalGet(99))
        ))]),
        [true, false],
        "an assignment demotes its parameter"
    );
    assert_eq!(
        demoted(vec![let_stmt(2, true, Expr::LocalGet(99))]),
        [false, true],
        "a `var` re-declaration demotes its parameter"
    );
    assert_eq!(
        demoted(vec![let_stmt(3, true, Expr::LocalGet(1))]),
        [false, false],
        "binding a different local demotes nothing"
    );
}

fn rows_length() -> Expr {
    // `rows.length`, with `rows` (local 1) of unknown provenance.
    Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(1)),
        property: "length".to_string(),
        byte_offset: 0,
    }
}

fn times_seven(e: Expr) -> Expr {
    Expr::Binary {
        op: perry_hir::BinaryOp::Mul,
        left: Box::new(e),
        right: Box::new(Expr::Integer(7)),
    }
}

#[test]
fn computed_length_proves_the_length_form_without_a_constant() {
    // #11810: `new Float64Array(rows.length * 7)`, directly and through an
    // immutable local. A product is never an Object, so the construction is
    // the length form whatever `rows` holds; its value is not constant.
    let direct = module_with_init(vec![
        let_stmt(1, false, Expr::Undefined),
        let_stmt(
            2,
            false,
            ta_new(TYPED_ARRAY_KIND_FLOAT64, Some(times_seven(rows_length()))),
        ),
        Stmt::Expr(call(7, vec![Expr::LocalGet(2)])),
    ]);
    let via_local = module_with_init(vec![
        let_stmt(1, false, Expr::Undefined),
        let_stmt(3, false, times_seven(rows_length())),
        let_stmt(
            2,
            false,
            ta_new(TYPED_ARRAY_KIND_FLOAT64, Some(Expr::LocalGet(3))),
        ),
        Stmt::Expr(call(7, vec![Expr::LocalGet(2)])),
    ]);
    for m in [direct, via_local] {
        let facts = collect_spec_abi_facts(&m);
        assert_eq!(
            facts.ta_bindings.get(&2).map(|b| b.const_len),
            Some(None),
            "{:?}",
            facts.ta_bindings
        );
        assert_eq!(
            facts.call_sites.get(&7).unwrap()[0],
            vec![SpecParamRep::TaPtr {
                kind: TYPED_ARRAY_KIND_FLOAT64,
                const_len: None
            }]
        );
    }
}

#[test]
fn a_property_read_or_mutable_computed_length_is_not_a_length_form() {
    // `new Float64Array(rows.length)`: a property read may be an Object (an
    // ArrayBuffer getter), so it is not judged. A MUTABLE local holding a
    // product may be rebound to one before the construction.
    let property = module_with_init(vec![
        let_stmt(1, false, Expr::Undefined),
        let_stmt(
            2,
            false,
            ta_new(TYPED_ARRAY_KIND_FLOAT64, Some(rows_length())),
        ),
        Stmt::Expr(call(7, vec![Expr::LocalGet(2)])),
    ]);
    let mutable = module_with_init(vec![
        let_stmt(1, false, Expr::Undefined),
        let_stmt(3, true, times_seven(rows_length())),
        Stmt::Expr(Expr::LocalSet(3, Box::new(Expr::LocalGet(1)))),
        let_stmt(
            2,
            false,
            ta_new(TYPED_ARRAY_KIND_FLOAT64, Some(Expr::LocalGet(3))),
        ),
        Stmt::Expr(call(7, vec![Expr::LocalGet(2)])),
    ]);
    for m in [property, mutable] {
        let facts = collect_spec_abi_facts(&m);
        assert!(
            !facts.ta_bindings.contains_key(&2),
            "{:?}",
            facts.ta_bindings
        );
        assert_eq!(
            facts.call_sites.get(&7).unwrap()[0],
            vec![SpecParamRep::Boxed]
        );
    }
}

/// `var A = [1,2,3]; A[<key>] = 0; const P = new Int32Array(A); f(P)` with the
/// extra `pre` statements ahead of the store. Returns P's proven length.
fn store_key_const_len(pre: Vec<Stmt>, key: Expr) -> Option<Option<i64>> {
    let mut body = vec![let_stmt(
        1,
        true,
        Expr::Array(vec![Expr::Integer(1), Expr::Integer(2), Expr::Integer(3)]),
    )];
    body.extend(pre);
    body.push(Stmt::Expr(Expr::IndexSet {
        object: Box::new(Expr::LocalGet(1)),
        index: Box::new(key),
        value: Box::new(Expr::Integer(0)),
    }));
    body.push(let_stmt(
        2,
        false,
        ta_new(TYPED_ARRAY_KIND_INT32, Some(Expr::LocalGet(1))),
    ));
    body.push(Stmt::Expr(call(7, vec![Expr::LocalGet(2)])));
    let facts = collect_spec_abi_facts(&module_with_init(body));
    facts.ta_bindings.get(&2).map(|b| b.const_len)
}

#[test]
fn element_store_with_a_numeric_key_keeps_the_constant_length() {
    // Growing or overwriting through a numeric key never shrinks the array.
    assert_eq!(store_key_const_len(vec![], Expr::Integer(1)), Some(Some(3)));
    assert_eq!(
        store_key_const_len(
            vec![let_stmt(3, false, Expr::Integer(2))],
            Expr::LocalGet(3)
        ),
        Some(Some(3))
    );
}

#[test]
fn element_store_that_may_be_length_demotes_the_constant_length() {
    // #11973: `for (a["length"] of [2]) {}` lowers to `a["length"] = 2` as an
    // `IndexSet`, which truncates `a`. The literal key, a local holding the
    // string, and a `+` that builds the string must all demote; the binding
    // stays proven non-view.
    assert_eq!(
        store_key_const_len(vec![], Expr::String("length".into())),
        Some(None)
    );
    assert_eq!(
        store_key_const_len(
            vec![let_stmt(3, false, Expr::String("length".into()))],
            Expr::LocalGet(3)
        ),
        Some(None)
    );
    assert_eq!(
        store_key_const_len(
            vec![],
            Expr::Binary {
                op: perry_hir::BinaryOp::Add,
                left: Box::new(Expr::String("len".into())),
                right: Box::new(Expr::String("gth".into())),
            }
        ),
        Some(None)
    );
}
