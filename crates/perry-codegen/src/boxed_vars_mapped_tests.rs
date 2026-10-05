use super::*;
use perry_hir::types::Type;
use perry_hir::{ArgumentsObjectMeta, Expr, Function, Module, Param, Stmt};

fn params(mapped: bool) -> Vec<Param> {
    let param = |id, name: &str| Param {
        id,
        name: name.into(),
        ty: Type::Any,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    };
    let mut args = param(3, "arguments");
    args.arguments_object = Some(ArgumentsObjectMeta {
        strict: !mapped,
        simple_parameters: true,
        mapped_parameter_ids: if mapped { vec![(0, 1), (1, 2)] } else { vec![] },
        restricted_callee: !mapped,
    });
    vec![param(1, "a"), param(2, "b"), args]
}

fn closure(id: u32, params: Vec<Param>, body: Vec<Stmt>, captures: Vec<u32>) -> Expr {
    Expr::Closure {
        func_id: id,
        params,
        return_type: Type::Any,
        body,
        captures,
        mutable_captures: Vec::new(),
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: false,
    }
}

fn write() -> Stmt {
    Stmt::Expr(Expr::IndexSet {
        object: Box::new(Expr::LocalGet(3)),
        index: Box::new(Expr::Integer(0)),
        value: Box::new(Expr::LocalGet(2)),
    })
}

fn module(params: Vec<Param>, body: Vec<Stmt>) -> Module {
    let mut m = Module::new("mapped");
    m.functions.push(Function {
        id: 10,
        name: "outer".into(),
        type_params: Vec::new(),
        params,
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    m
}

#[test]
fn mapped_cells_are_known_to_all_capture_bodies_without_local_writes() {
    let body = vec![Stmt::Expr(closure(
        11,
        vec![],
        vec![Stmt::Expr(closure(12, vec![], vec![write()], vec![2, 3]))],
        vec![2, 3],
    ))];
    let boxed =
        crate::codegen::boxed_locals::collect_module_boxed_vars(&module(params(true), body));
    assert_eq!(boxed, HashSet::from([1, 2]));
}

#[test]
fn function_expression_mapped_cells_reach_nested_arrows() {
    let outer = closure(
        11,
        params(true),
        vec![Stmt::Expr(closure(12, vec![], vec![write()], vec![2, 3]))],
        vec![],
    );
    let mut m = Module::new("expression");
    m.init.push(Stmt::Expr(outer));
    assert_eq!(
        crate::codegen::boxed_locals::collect_module_boxed_vars(&m),
        HashSet::from([1, 2])
    );
}

#[test]
fn escaped_arguments_publish_cells_but_unmapped_arguments_do_not() {
    let body = vec![Stmt::Return(Some(Expr::LocalGet(3)))];
    assert_eq!(
        collect_boxed_param_ids(&params(true), &body),
        HashSet::from([1, 2])
    );
    assert!(collect_boxed_param_ids(&params(false), &body).is_empty());
    let mut nonsimple = params(false);
    let meta = nonsimple[2].arguments_object.as_mut().unwrap();
    meta.strict = false;
    meta.simple_parameters = false;
    assert!(collect_boxed_param_ids(&nonsimple, &body).is_empty());
}

#[test]
fn elided_length_and_index_reads_keep_ordinary_parameter_slots() {
    for read in [
        Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(3)),
            property: "length".into(),
            byte_offset: 0,
        },
        Expr::IndexGet {
            object: Box::new(Expr::LocalGet(3)),
            index: Box::new(Expr::Integer(0)),
        },
    ] {
        assert!(collect_boxed_param_ids(&params(true), &[Stmt::Return(Some(read))]).is_empty());
    }
}

#[test]
fn redeclared_parameters_keep_prologue_cells_in_named_and_expression_bodies() {
    let body = vec![
        Stmt::Let {
            id: 1,
            name: "a".into(),
            ty: Type::Any,
            mutable: true,
            init: Some(Expr::Integer(5)),
        },
        Stmt::Return(Some(Expr::LocalGet(3))),
    ];
    let mut named = module(params(true), body.clone());
    crate::scope_env::group_scope_boxes(&mut named);
    assert!(matches!(named.functions[0].body[0], Stmt::Let { .. }));
    // Codegen must decline even a preallocation handed in by another pass.
    named.functions[0]
        .body
        .insert(0, Stmt::PreallocateBoxes(vec![1]));
    let boxed = crate::codegen::boxed_locals::collect_module_boxed_vars(&named);
    assert!(
        crate::scope_env::ScopeMap::build(&named, &boxed, &HashMap::new())
            .slot(1)
            .is_none()
    );
    let mut expression = Module::new("redeclared_expression");
    expression
        .init
        .push(Stmt::Expr(closure(11, params(true), body, vec![])));
    crate::scope_env::group_scope_boxes(&mut expression);
    let Stmt::Expr(Expr::Closure { body, .. }) = &expression.init[0] else {
        panic!("closure retained")
    };
    assert!(matches!(body[0], Stmt::Let { .. }));
}
