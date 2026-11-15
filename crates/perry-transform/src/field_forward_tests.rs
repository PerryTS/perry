use super::*;
use perry_hir::{Class, ClassField, ClassFieldOrigin};

const B: u32 = 1;
const V: u32 = 2;

fn function(name: &str, body: Vec<Stmt>) -> Function {
    Function {
        id: 1,
        name: name.into(),
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

fn field(name: &str) -> ClassField {
    ClassField {
        origin: ClassFieldOrigin::Definition,
        name: name.to_string(),
        key_expr: None,
        ty: Type::Any,
        init: Some(Expr::Null),
        is_private: false,
        is_readonly: false,
        decorators: vec![],
    }
}

/// `class Box { p = null; }`
fn boxed() -> Class {
    Class {
        id: 1,
        name: "Box".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![field("p")],
        constructor: None,
        methods: Vec::new(),
        getters: Vec::new(),
        setters: Vec::new(),
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
        computed_members: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        aliases: Vec::new(),
        is_nested: false,
        alloc_width_hint: 0,
        specialized_from: None,
    }
}

fn bind(id: u32, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: format!("v{id}"),
        ty: Type::Number,
        mutable: false,
        init: Some(init),
    }
}

/// `let b = new Box()`
fn owner() -> Stmt {
    Stmt::Let {
        id: B,
        name: "b".into(),
        ty: Type::Named("Box".into()),
        mutable: false,
        init: Some(Expr::New {
            class_name: "Box".into(),
            args: vec![],
            type_args: vec![],
            byte_offset: 0,
            cap_args_appended: 0,
            byte_end: 0,
        }),
    }
}

/// `let v = 7`
fn value() -> Stmt {
    bind(V, Expr::Integer(7))
}

fn get(name: &str) -> Expr {
    Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(B)),
        property: name.into(),
        byte_offset: 0,
        byte_end: 0,
    }
}

/// `b.<name> = <v>` in the form an inlined `put(b, v)` leaves.
fn store_of(name: &str, v: Expr) -> Stmt {
    Stmt::Expr(Expr::PutValueSet {
        target: Box::new(Expr::LocalGet(B)),
        key: Box::new(Expr::String(name.into())),
        value: Box::new(v),
        receiver: Box::new(Expr::LocalGet(B)),
        strict: true,
        byte_offset: 0,
        byte_end: 0,
        element_offset: 0,
    })
}

fn store() -> Stmt {
    store_of("p", Expr::LocalGet(V))
}

fn read() -> Stmt {
    Stmt::Expr(Expr::Void(Box::new(get("p"))))
}

fn looped(body: Vec<Stmt>) -> Stmt {
    Stmt::While {
        condition: Expr::Bool(true),
        body,
    }
}

fn optimize_in(class: Class, body: Vec<Stmt>, extra: Vec<Function>) -> Vec<Stmt> {
    let mut module = Module::new("forward");
    module.classes.push(class);
    module.functions.push(function("forward", body));
    module.functions.extend(extra);
    run(&mut module);
    module.functions.remove(0).body
}

fn optimize(body: Vec<Stmt>) -> Vec<Stmt> {
    optimize_in(boxed(), body, vec![])
}

fn text(body: &[Stmt]) -> String {
    format!("{body:?}")
}

fn unchanged_in(class: Class, body: Vec<Stmt>, extra: Vec<Function>) {
    let before = text(&body);
    assert_eq!(text(&optimize_in(class, body, extra)), before);
}

fn unchanged(body: Vec<Stmt>) {
    unchanged_in(boxed(), body, vec![]);
}

#[test]
fn reads_after_the_store_become_the_value_and_the_store_goes() {
    let got = optimize(vec![looped(vec![
        owner(),
        value(),
        store(),
        read(),
        looped(vec![read()]),
    ])]);
    let forwarded = Stmt::Expr(Expr::Void(Box::new(Expr::LocalGet(V))));
    let want = vec![looped(vec![
        owner(),
        value(),
        forwarded.clone(),
        looped(vec![forwarded]),
    ])];
    assert_eq!(text(&got), text(&want));
}

#[test]
fn a_stored_literal_is_forwarded() {
    let got = optimize(vec![owner(), store_of("p", Expr::Integer(5)), read()]);
    let want = vec![owner(), Stmt::Expr(Expr::Void(Box::new(Expr::Integer(5))))];
    assert_eq!(text(&got), text(&want));
}

#[test]
fn a_use_of_the_owner_in_another_body_keeps_everything() {
    let other = function("other", vec![read()]);
    unchanged_in(
        boxed(),
        vec![owner(), value(), store(), read()],
        vec![other],
    );
}

#[test]
fn a_captured_owner_keeps_everything() {
    let closure = Expr::Closure {
        func_id: 9,
        params: vec![],
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(get("p")))],
        captures: vec![B],
        mutable_captures: vec![],
        captures_this: false,
        captures_new_target: false,
        is_strict: true,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
    };
    unchanged(vec![owner(), value(), store(), read(), Stmt::Expr(closure)]);
}

#[test]
fn a_store_inside_a_branch_is_not_forwarded() {
    unchanged(vec![
        owner(),
        value(),
        Stmt::If {
            condition: Expr::Bool(true),
            then_branch: vec![store()],
            else_branch: None,
        },
        read(),
    ]);
}

#[test]
fn a_field_stored_twice_is_not_forwarded() {
    unchanged(vec![
        owner(),
        value(),
        store(),
        read(),
        store_of("p", Expr::Integer(1)),
        read(),
    ]);
}

#[test]
fn a_read_before_the_store_keeps_it() {
    unchanged(vec![owner(), value(), read(), store(), read()]);
}

#[test]
fn a_written_value_local_is_not_forwarded() {
    unchanged(vec![
        owner(),
        value(),
        store(),
        Stmt::Expr(Expr::LocalSet(V, Box::new(Expr::Integer(8)))),
        read(),
    ]);
}

#[test]
fn a_value_declared_after_the_store_is_not_forwarded() {
    unchanged(vec![owner(), store(), value(), read()]);
}

#[test]
fn a_field_with_an_accessor_is_not_forwarded() {
    let mut class = boxed();
    class.getters.push(("p".into(), function("p", vec![])));
    unchanged_in(class, vec![owner(), value(), store(), read()], vec![]);
}

#[test]
fn an_open_construction_keeps_everything() {
    let mut class = boxed();
    class.constructor = Some(function(
        "constructor",
        vec![Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::LocalGet(9)),
            args: vec![],
            type_args: vec![],
            byte_offset: 0,
            byte_end: 0,
        })],
    ));
    unchanged_in(class, vec![owner(), value(), store(), read()], vec![]);
}

#[test]
fn a_field_the_class_does_not_declare_is_not_forwarded() {
    unchanged(vec![
        owner(),
        value(),
        store_of("q", Expr::LocalGet(V)),
        read(),
    ]);
}

#[test]
fn a_method_call_on_the_field_keeps_everything() {
    let call = Expr::Call {
        callee: Box::new(get("p")),
        args: vec![],
        type_args: vec![],
        byte_offset: 0,
        byte_end: 0,
    };
    unchanged(vec![owner(), value(), store(), read(), Stmt::Expr(call)]);
}

#[test]
fn top_level_init_is_left_alone_and_nested_lists_are_forwarded() {
    let mut module = Module::new("forward");
    module.classes.push(boxed());
    module.init = vec![owner(), value(), store(), read()];
    let top = text(&module.init);
    run(&mut module);
    assert_eq!(text(&module.init), top);

    module.init = vec![looped(vec![owner(), value(), store(), read()])];
    run(&mut module);
    let forwarded = Stmt::Expr(Expr::Void(Box::new(Expr::LocalGet(V))));
    assert_eq!(
        text(&module.init),
        text(&[looped(vec![owner(), value(), forwarded])])
    );
}
