//! #12016: public method values use the ordinary shape read, including in
//! constructors whose live receiver may be a subclass with an own override.
use crate::compile_module;
use perry_hir::types::Type;
use perry_hir::{Class, Expr, Function, Module, Stmt};

fn function(id: u32, name: &str, body: Vec<Stmt>) -> Function {
    Function {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body,
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

fn module(constructor: bool) -> Module {
    let read = Expr::PropertyGet {
        byte_end: 0,
        object: Box::new(Expr::This),
        property: "m".to_string(),
        byte_offset: 0,
    };
    let probe = function(2, "read", vec![Stmt::Return(Some(read.clone()))]);
    let mut m = Module::new("method_value_shapes.ts");
    m.classes.push(Class {
        id: 101,
        name: "Base".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: Vec::new(),
        constructor: constructor
            .then(|| function(3, "constructor", vec![Stmt::Expr(read), Stmt::Return(None)])),
        methods: if constructor {
            vec![function(
                1,
                "m",
                vec![Stmt::Return(Some(Expr::Number(1.0)))],
            )]
        } else {
            vec![
                function(1, "m", vec![Stmt::Return(Some(Expr::Number(1.0)))]),
                probe,
            ]
        },
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
    });
    m
}

fn assert_shape_read(constructor: bool) {
    let ir = String::from_utf8(
        compile_module(
            &module(constructor),
            super::class_field_barrier_tests::ir_opts(),
        )
        .expect("public method value fixture compiles"),
    )
    .unwrap();
    assert!(
        !ir.contains("call double @js_class_method_bind_by_id("),
        "a public declared method read must not resolve by class/name: {ir}"
    );
    // The site probes the live receiver's shape with the leaf shape probe
    // (a sentinel result means "not served by the shape"), and only a miss
    // reaches the ordinary Get, which can run a getter.
    assert!(
        (ir.contains("call double @perry_probe_get(")
            || ir.contains("call double @perry_probe_get_memo("))
            && ir.contains("icmp ne i64"),
        "the site must probe the live receiver's shape and test for a miss: {ir}"
    );
    assert!(
        ir.contains("call double @js_object_get_field_value(")
            || ir.contains("call double @js_object_get_field_value_site("),
        "a shape miss must retain the ordinary Get, which can run a getter: {ir}"
    );
    let start = ir.find("\npget.recv_ok.").expect("probe block is emitted");
    let body = &ir[start + 1..];
    let body = &body[..body.find("\n\n").expect("end of probe block")];
    assert_eq!(
        body.matches("call ").count(),
        1,
        "the hit must be the shape probe alone, with no helper call: {body}"
    );
    let slow = ir.find("\npget.slow.").expect("miss block is emitted");
    let slow = &ir[slow + 1..];
    let slow = &slow[..slow.find("\n\n").expect("end of miss block")];
    assert!(
        slow.contains("@js_object_get_field_value(")
            || slow.contains("@js_object_get_field_value_site("),
        "the miss block must hold the ordinary Get: {slow}"
    );
}

#[test]
fn public_method_value_in_a_method_is_a_shape_read() {
    assert_shape_read(false);
}

#[test]
fn public_method_value_in_a_constructor_is_a_shape_read() {
    assert_shape_read(true);
}
