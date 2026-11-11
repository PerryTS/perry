use super::*;
use perry_hir::SwitchCase;

fn function(body: Vec<Stmt>) -> Function {
    Function {
        id: 1,
        name: "scoped".into(),
        type_params: vec![],
        params: vec![],
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

fn bind(id: u32, ty: Type, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: format!("v{id}"),
        ty,
        mutable: false,
        init: Some(init),
    }
}

fn copy(id: u32, src: u32) -> Stmt {
    bind(id, Type::Number, Expr::LocalGet(src))
}

fn value(id: u32) -> Stmt {
    bind(id, Type::Number, Expr::Integer(7))
}

fn read(id: u32) -> Stmt {
    Stmt::Expr(Expr::Void(Box::new(Expr::LocalGet(id))))
}

fn looped(body: Vec<Stmt>) -> Stmt {
    Stmt::While {
        condition: Expr::Bool(true),
        body,
    }
}

fn optimize(body: Vec<Stmt>) -> Vec<Stmt> {
    let mut module = Module::new("scoped");
    module.functions.push(function(body));
    run(&mut module);
    module.functions.remove(0).body
}

fn text(body: &[Stmt]) -> String {
    format!("{body:?}")
}

fn unchanged(body: Vec<Stmt>) {
    let before = text(&body);
    assert_eq!(text(&optimize(body)), before);
}

#[test]
fn a_copy_in_a_loop_body_becomes_its_source() {
    let got = optimize(vec![looped(vec![value(2), copy(3, 2), read(3), read(3)])]);
    let want = vec![looped(vec![value(2), read(2), read(2)])];
    assert_eq!(text(&got), text(&want));
}

#[test]
fn a_source_from_an_enclosing_list_and_a_chain_fold() {
    let got = optimize(vec![
        value(2),
        looped(vec![copy(3, 2), copy(4, 3), looped(vec![read(4)])]),
    ]);
    let want = vec![value(2), looped(vec![looped(vec![read(2)])])];
    assert_eq!(text(&got), text(&want));
}

#[test]
fn a_written_source_or_copy_stays() {
    let write = |id| Stmt::Expr(Expr::LocalSet(id, Box::new(Expr::Integer(1))));
    unchanged(vec![looped(vec![value(2), copy(3, 2), write(2), read(3)])]);
    unchanged(vec![looped(vec![value(2), copy(3, 2), write(3), read(3)])]);
}

#[test]
fn a_read_before_the_copy_keeps_the_declaration() {
    // The earlier read is the temporal dead zone of v3: it must still throw.
    unchanged(vec![looped(vec![value(2), read(3), copy(3, 2), read(3)])]);
}

#[test]
fn a_source_declared_after_the_copy_stays() {
    unchanged(vec![looped(vec![copy(3, 2), value(2), read(3)])]);
}

#[test]
fn a_differently_typed_copy_stays() {
    unchanged(vec![looped(vec![
        value(2),
        bind(3, Type::String, Expr::LocalGet(2)),
        read(3),
    ])]);
}

#[test]
fn a_jump_into_a_later_switch_case_can_skip_the_source() {
    let case = |body| SwitchCase {
        test: Some(Expr::Integer(0)),
        body,
    };
    unchanged(vec![Stmt::Switch {
        discriminant: Expr::Integer(1),
        cases: vec![case(vec![value(2)]), case(vec![copy(3, 2), read(3)])],
    }]);
}

#[test]
fn a_captured_copy_stays() {
    let closure = Expr::Closure {
        func_id: 9,
        params: vec![],
        return_type: Type::Any,
        body: vec![read(3)],
        captures: vec![3],
        mutable_captures: vec![],
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: true,
    };
    unchanged(vec![looped(vec![
        value(2),
        copy(3, 2),
        Stmt::Expr(closure),
    ])]);
}

#[test]
fn a_copy_inside_a_closure_folds_only_against_the_closure_body() {
    let closure = |body| {
        Stmt::Expr(Expr::Closure {
            func_id: 9,
            params: vec![],
            return_type: Type::Any,
            body,
            captures: vec![2],
            mutable_captures: vec![],
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: true,
            is_async: false,
            is_generator: false,
            is_strict: true,
        })
    };
    // v2 lives outside: the closure may run before v2 is initialized.
    unchanged(vec![value(2), closure(vec![copy(3, 2), read(3)])]);
    let got = optimize(vec![closure(vec![value(5), copy(6, 5), read(6)])]);
    assert_eq!(text(&got), text(&[closure(vec![value(5), read(5)])]));
}

#[test]
fn top_level_module_init_declarations_stay_and_nested_ones_fold() {
    let mut module = Module::new("scoped");
    module.init = vec![
        value(2),
        copy(3, 2),
        read(3),
        looped(vec![value(4), copy(5, 4), read(5)]),
    ];
    run(&mut module);
    let want = vec![
        value(2),
        copy(3, 2),
        read(3),
        looped(vec![value(4), read(4)]),
    ];
    assert_eq!(text(&module.init), text(&want));
}

#[test]
fn a_copy_whose_type_may_be_a_native_record_stays() {
    for ty in [Type::Any, Type::Named("Packet".into())] {
        unchanged(vec![looped(vec![
            bind(2, ty.clone(), Expr::Integer(7)),
            bind(3, ty, Expr::LocalGet(2)),
            read(3),
        ])]);
    }
}
