use super::*;
use perry_hir::types::Type;
use perry_hir::{ClassField, ClassFieldOrigin, Param};

fn param(id: u32) -> Param {
    Param {
        id,
        name: format!("p{id}"),
        ty: Type::Number,
        default: None,
        decorators: vec![],
        is_rest: false,
        arguments_object: None,
    }
}

fn ctor(params: Vec<Param>, body: Vec<Stmt>) -> Function {
    Function {
        id: 1,
        name: "constructor".into(),
        type_params: vec![],
        params,
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    }
}

fn class(name: &str, constructor: Option<Function>) -> Class {
    Class {
        id: 1,
        name: name.to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: Vec::new(),
        constructor,
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

fn field(name: &str, init: Option<Expr>) -> ClassField {
    ClassField {
        origin: ClassFieldOrigin::Definition,
        name: name.to_string(),
        key_expr: None,
        ty: Type::Any,
        init,
        is_private: false,
        is_readonly: false,
        decorators: vec![],
    }
}

/// `this.<name> = <value>` in the form property assignments lower to.
fn store(name: &str, value: Expr) -> Stmt {
    Stmt::Expr(Expr::PutValueSet {
        target: Box::new(Expr::This),
        key: Box::new(Expr::String(name.into())),
        value: Box::new(value),
        receiver: Box::new(Expr::This),
        strict: true,
        byte_offset: 0,
        byte_end: 0,
        element_offset: 0,
    })
}

fn point() -> Class {
    class(
        "P",
        Some(ctor(
            vec![param(1), param(2), param(3)],
            vec![
                store("x", Expr::LocalGet(1)),
                store("y", Expr::LocalGet(3)),
                store("z", Expr::Integer(0)),
            ],
        )),
    )
}

fn construction(classes: Vec<Class>) -> Option<Closed> {
    let mut module = Module::new("effects");
    module.classes = classes;
    Effects::of_module(&module).construction("P").cloned()
}

#[test]
fn stores_of_parameters_and_literals_close_the_construction() {
    let closed = construction(vec![point()]).expect("closed");
    assert_eq!(
        closed.kept,
        [Fate::KeptByReceiver, Fate::Dropped, Fate::KeptByReceiver]
    );
}

#[test]
fn a_class_without_a_constructor_is_closed() {
    let mut c = class("P", None);
    c.fields.push(field("x", Some(Expr::Integer(1))));
    c.fields.push(field("y", None));
    assert_eq!(construction(vec![c]), Some(Closed { kept: vec![] }));
}

#[test]
fn an_operator_on_a_parameter_may_run_code() {
    let sum = Expr::Binary {
        op: perry_hir::BinaryOp::Add,
        left: Box::new(Expr::LocalGet(1)),
        right: Box::new(Expr::Integer(1)),
    };
    let c = class("P", Some(ctor(vec![param(1)], vec![store("x", sum)])));
    assert_eq!(construction(vec![c]), None);
}

#[test]
fn a_store_into_another_object_or_a_call_opens_it() {
    let other = Stmt::Expr(Expr::PutValueSet {
        target: Box::new(Expr::LocalGet(1)),
        key: Box::new(Expr::String("x".into())),
        value: Box::new(Expr::Integer(1)),
        receiver: Box::new(Expr::LocalGet(1)),
        strict: true,
        byte_offset: 0,
        byte_end: 0,
        element_offset: 0,
    });
    let c = class("P", Some(ctor(vec![param(1)], vec![other])));
    assert_eq!(construction(vec![c]), None);

    let call = Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::LocalGet(1)),
        args: vec![],
        type_args: vec![],
        byte_offset: 0,
        byte_end: 0,
    });
    let c = class("P", Some(ctor(vec![param(1)], vec![call])));
    assert_eq!(construction(vec![c]), None);
}

#[test]
fn an_accessor_for_a_stored_name_opens_it() {
    let mut c = point();
    c.setters.push(("y".into(), ctor(vec![param(9)], vec![])));
    assert_eq!(construction(vec![c]), None);
}

#[test]
fn a_parent_a_default_or_a_field_initializer_that_reads_opens_it() {
    let mut c = point();
    c.extends_name = Some("Base".into());
    assert_eq!(construction(vec![c]), None);

    let mut c = point();
    c.constructor.as_mut().unwrap().params[0].default = Some(Expr::Integer(1));
    assert_eq!(construction(vec![c]), None);

    let mut c = point();
    c.fields.push(field("w", Some(Expr::LocalGet(7))));
    assert_eq!(construction(vec![c]), None);
}

#[test]
fn two_classes_of_one_name_are_both_open() {
    assert_eq!(construction(vec![point(), point()]), None);
}

#[test]
fn without_a_session_every_construction_is_open() {
    assert!(!construction_is_closed("P"));
    let mut module = Module::new("effects");
    module.classes = vec![point()];
    {
        let _session = Session::start(&module);
        assert!(construction_is_closed("P"));
        assert!(!construction_is_closed("Q"));
    }
    assert!(!construction_is_closed("P"));
}
