use super::*;
use perry_hir::types::Type;
use perry_hir::{ArgumentsObjectMeta, Param};

fn function(body: Vec<Stmt>) -> Function {
    Function {
        id: 1,
        name: "copies".into(),
        type_params: vec![],
        params: vec![Param {
            id: 1,
            name: "x".into(),
            ty: Type::Any,
            default: None,
            decorators: vec![],
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: true,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    }
}

fn alias(id: u32, src: u32) -> Stmt {
    Stmt::Let {
        id,
        name: format!("v{id}"),
        ty: Type::Any,
        mutable: true,
        init: Some(Expr::LocalGet(src)),
    }
}

fn ret(id: u32) -> Stmt {
    Stmt::Return(Some(Expr::LocalGet(id)))
}

fn set(id: u32, value: Expr) -> Stmt {
    Stmt::Expr(Expr::LocalSet(id, Box::new(value)))
}

fn optimize(f: Function) -> Function {
    let mut module = Module::new("copies");
    module.functions.push(f);
    run(&mut module);
    module.functions.remove(0)
}

fn unchanged(f: Function) {
    let before = format!("{:?}", f.body);
    assert_eq!(format!("{:?}", optimize(f).body), before);
}

#[test]
fn transitive_aliases_become_the_original_value() {
    let f = optimize(function(vec![
        alias(2, 1),
        alias(3, 2),
        alias(4, 3),
        ret(4),
    ]));
    assert!(matches!(
        f.body.as_slice(),
        [Stmt::Return(Some(Expr::LocalGet(1)))]
    ));
}

#[test]
fn unread_assignments_and_their_declaration_disappear() {
    let f = optimize(function(vec![
        alias(2, 1),
        set(2, Expr::LocalGet(1)),
        set(2, Expr::Number(3.0)),
        ret(1),
    ]));
    assert!(matches!(
        f.body.as_slice(),
        [Stmt::Return(Some(Expr::LocalGet(1)))]
    ));
}

#[test]
fn source_or_alias_writes_keep_the_copy() {
    unchanged(function(vec![
        alias(2, 1),
        set(1, Expr::Number(3.0)),
        ret(2),
    ]));
    unchanged(function(vec![
        alias(2, 1),
        set(2, Expr::Number(3.0)),
        ret(2),
    ]));
}

#[test]
fn discarded_assignments_keep_effectful_rhs_and_storage() {
    let call = Expr::Call {
        callee: Box::new(Expr::GlobalGet(99)),
        args: vec![],
        type_args: vec![],
        byte_offset: 0,
    };
    unchanged(function(vec![alias(2, 1), set(2, call), ret(1)]));
    // An assignment nested in another expression also needs the declaration.
    unchanged(function(vec![
        alias(2, 1),
        Stmt::Expr(Expr::Void(Box::new(Expr::LocalSet(
            2,
            Box::new(Expr::Number(3.0)),
        )))),
        ret(1),
    ]));
}

#[test]
fn unread_const_assignments_still_throw() {
    let mut declaration = alias(2, 1);
    if let Stmt::Let { mutable, .. } = &mut declaration {
        *mutable = false;
    }
    unchanged(function(vec![
        declaration,
        set(2, Expr::LocalGet(1)),
        ret(1),
    ]));
}

#[test]
fn forward_reads_and_writes_keep_tdz_behavior() {
    unchanged(function(vec![
        Stmt::Expr(Expr::LocalGet(2)),
        alias(2, 1),
        ret(2),
    ]));
    unchanged(function(vec![
        set(2, Expr::Number(3.0)),
        alias(2, 1),
        ret(1),
    ]));
    unchanged(function(vec![
        Stmt::PreallocateTdzBoxes(vec![2]),
        alias(2, 1),
        ret(2),
    ]));
}

#[test]
fn arguments_defaults_async_and_captures_are_excluded() {
    let f = function(vec![alias(2, 1), ret(2)]);
    let mut args = f.clone();
    args.params[0].arguments_object = Some(ArgumentsObjectMeta {
        strict: false,
        simple_parameters: true,
        mapped_parameter_ids: vec![(0, 1)],
        restricted_callee: false,
    });
    unchanged(args);
    let mut defaults = f.clone();
    defaults.params[0].default = Some(Expr::Number(3.0));
    unchanged(defaults);
    let mut async_f = f.clone();
    async_f.is_async = true;
    unchanged(async_f);
    let mut generator = f.clone();
    generator.is_generator = true;
    unchanged(generator);
    let mut capture = f.clone();
    capture.captures.push(9);
    unchanged(capture);
    let mut module = Module::new("retained_capture");
    let mut sibling = function(vec![ret(2)]);
    sibling.captures.push(2);
    module.functions = vec![f.clone(), sibling];
    run(&mut module);
    assert_eq!(
        format!("{:?}", module.functions[0].body),
        format!("{:?}", f.body)
    );
}

#[test]
fn loops_and_local_id_intrinsics_are_excluded() {
    unchanged(function(vec![
        alias(2, 1),
        Stmt::While {
            condition: Expr::Bool(false),
            body: vec![ret(2)],
        },
        ret(2),
    ]));
    unchanged(function(vec![
        alias(2, 1),
        Stmt::Expr(Expr::ArrayPop(2)),
        ret(2),
    ]));
}

#[test]
fn effectful_use_keeps_evaluation_order_but_loses_the_alias() {
    let call = Expr::Call {
        callee: Box::new(Expr::LocalGet(1)),
        args: vec![Expr::LocalGet(2)],
        type_args: vec![],
        byte_offset: 17,
    };
    let f = optimize(function(vec![alias(2, 1), Stmt::Return(Some(call))]));
    assert_eq!(f.body.len(), 1);
    let Stmt::Return(Some(Expr::Call {
        args, byte_offset, ..
    })) = &f.body[0]
    else {
        panic!()
    };
    assert!(matches!(args.as_slice(), [Expr::LocalGet(1)]));
    assert_eq!(*byte_offset, 17);
}
