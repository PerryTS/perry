use super::*;
use perry_hir::types::Type;
use perry_hir::{Function, Module, TYPED_ARRAY_KIND_INT32};

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
    m
}

#[test]
fn literal_length_binding_and_dominated_site() {
    // const P = new Int32Array(4); f(P, 0, 1.5);
    let m = module_with_init(vec![
        let_stmt(10, false, ta_new(TYPED_ARRAY_KIND_INT32, Some(Expr::Integer(4)))),
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
fn array_literal_source_counts_elements() {
    // var A = [1,2,3]; const P = new Int32Array(A); f(P)
    let m = module_with_init(vec![
        let_stmt(
            1,
            true,
            Expr::Array(vec![Expr::Integer(1), Expr::Integer(2), Expr::Integer(3)]),
        ),
        let_stmt(2, false, ta_new(TYPED_ARRAY_KIND_INT32, Some(Expr::LocalGet(1)))),
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
        let_stmt(2, false, ta_new(TYPED_ARRAY_KIND_INT32, Some(Expr::LocalGet(1)))),
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
        let_stmt(3, true, ta_new(TYPED_ARRAY_KIND_INT32, Some(Expr::Integer(4)))),
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
        let_stmt(3, true, ta_new(TYPED_ARRAY_KIND_INT32, Some(Expr::Integer(4)))),
        Stmt::Expr(call(7, vec![Expr::LocalGet(3)])),
    ]);
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
        let_stmt(3, false, ta_new(TYPED_ARRAY_KIND_INT32, Some(Expr::Integer(4)))),
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
        let_stmt(10, false, ta_new(TYPED_ARRAY_KIND_INT32, Some(Expr::Integer(4)))),
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
        let_stmt(10, false, ta_new(TYPED_ARRAY_KIND_INT32, Some(Expr::Integer(2)))),
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
        let_stmt(1, true, Expr::Array(vec![Expr::Integer(1), Expr::Integer(2)])),
        Stmt::Expr(call(8, vec![Expr::LocalGet(1)])),
        let_stmt(2, false, ta_new(TYPED_ARRAY_KIND_INT32, Some(Expr::LocalGet(1)))),
        Stmt::Expr(call(7, vec![Expr::LocalGet(2)])),
    ]);
    let facts = collect_spec_abi_facts(&m);
    let b = facts.ta_bindings.get(&2).expect("still non-view-proven");
    assert_eq!(b.const_len, None);
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
