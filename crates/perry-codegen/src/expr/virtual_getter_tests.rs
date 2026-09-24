//! A getter read on `this` must consult the live receiver, including overrides.
use crate::compile_module;
use perry_hir::types::Type;
use perry_hir::{Class, Expr, Function, Module, Stmt};

fn function(id: u32, name: &str, value: Expr) -> Function {
    Function {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(value))],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }
}

#[test]
fn getter_read_uses_live_receiver_instead_of_enclosing_class() {
    let read = Expr::PropertyGet {
        object: Box::new(Expr::This),
        property: "value".to_string(),
        byte_offset: 0,
    };
    let class = Class {
        id: 101,
        name: "Base".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: Vec::new(),
        constructor: None,
        methods: vec![function(2, "read", read)],
        getters: vec![(
            "value".to_string(),
            function(1, "get_value", Expr::Number(1.0)),
        )],
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
    };
    let mut module = Module::new("virtual_getter.ts");
    module.classes.push(class);
    let ir = String::from_utf8(
        compile_module(&module, super::class_field_barrier_tests::ir_opts())
            .expect("class compiles"),
    )
    .unwrap();
    let getter = ir
        .lines()
        .find(|line| line.starts_with("define ") && line.contains("__get_get_value("))
        .expect("getter body must be emitted");
    let symbol = getter.split('@').nth(1).unwrap().split('(').next().unwrap();
    assert!(
        !ir.contains(&format!("call double @{symbol}(")),
        "a base member must not directly call its base getter: {ir}"
    );
    assert!(
        ir.contains("call double @js_object_get_field_ic_slow("),
        "the property read must use runtime lookup: {ir}"
    );
}
