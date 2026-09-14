use perry_hir::types::Type;
use perry_hir::{Class, ClassField, Expr, Function, Module, Param, Stmt};

fn fixture(records: usize, fields: usize) -> Module {
    let class_name = "__AnonShape_descriptor_test";
    let params: Vec<_> = (0..fields)
        .map(|i| Param {
            id: i as u32 + 1,
            name: format!("k{i}"),
            ty: Type::Number,
            default: None,
            decorators: vec![],
            is_rest: false,
            arguments_object: None,
        })
        .collect();
    let constructor = Function {
        id: 1,
        name: "constructor".into(),
        type_params: vec![],
        body: params
            .iter()
            .map(|p| {
                Stmt::Expr(Expr::PropertySet {
                    object: Box::new(Expr::This),
                    property: p.name.clone(),
                    value: Box::new(Expr::LocalGet(p.id)),
                })
            })
            .collect(),
        params,
        return_type: Type::Void,
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    };
    let class = Class {
        id: 101,
        name: class_name.into(),
        type_params: vec![],
        extends: None,
        extends_name: None,
        extends_expr: None,
        native_extends: None,
        heritage_lexically_shadowed: false,
        fields: (0..fields)
            .map(|i| ClassField {
                name: format!("k{i}"),
                ty: Type::Number,
                key_expr: None,
                init: None,
                is_private: false,
                is_readonly: false,
                decorators: vec![],
            })
            .collect(),
        constructor: Some(constructor),
        methods: vec![],
        getters: vec![],
        setters: vec![],
        static_fields: vec![],
        static_methods: vec![],
        static_accessor_names: vec![],
        static_accessor_fn_ids: vec![],
        computed_members: vec![],
        decorators: vec![],
        is_exported: false,
        aliases: vec![],
        is_nested: false,
        alloc_width_hint: 0,
        specialized_from: None,
    };
    let mut module = Module::new("literal_descriptor_test");
    module.classes.push(class);
    module.init.push(Stmt::Let {
        id: fields as u32 + 100,
        name: "records".into(),
        ty: Type::Array(Box::new(Type::Named(class_name.into()))),
        mutable: false,
        init: Some(Expr::Array(
            (0..records)
                .map(|r| Expr::New {
                    class_name: class_name.into(),
                    args: (0..fields).map(|i| Expr::Number((r + i) as f64)).collect(),
                    type_args: vec![],
                    byte_offset: 0,
                    cap_args_appended: 0,
                })
                .collect(),
        )),
    });
    module
}

fn ir(module: &Module) -> String {
    String::from_utf8(
        crate::compile_module(module, super::super::class_field_barrier_tests::ir_opts())
            .expect("literal fixture lowers"),
    )
    .unwrap()
}

#[test]
fn cliff_literal_emits_one_materializer_and_bounded_function_bodies() {
    let ir = ir(&fixture(3200, 4));
    assert_eq!(
        ir.matches("call double @js_value_from_literal_descriptor(")
            .count(),
        1
    );
    // Count instructions inside functions, not descriptor bytes in globals.
    let mut in_function = false;
    let mut lines = 0;
    for line in ir.lines() {
        if line.starts_with("define ") {
            in_function = true;
            lines = 0;
        } else if line == "}" {
            assert!(
                lines < 2000,
                "a literal must not recreate a giant function: {lines} lines"
            );
            in_function = false;
        } else if in_function {
            lines += 1;
        }
    }
    assert!(ir.contains("perry_class_keys_"));
    assert!(ir.contains("perry_class_shape_id_"));
    assert!(ir.contains("perry_typed_shape_raw_f64_mask_"));
}

#[test]
fn small_literals_and_effectful_constructors_keep_normal_evaluation() {
    let small = ir(&fixture(2, 4));
    assert!(!small.contains("call double @js_value_from_literal_descriptor("));
    let mut effectful = fixture(80, 4);
    effectful.classes[0]
        .constructor
        .as_mut()
        .unwrap()
        .body
        .push(Stmt::Expr(Expr::Number(7.0)));
    let effectful = ir(&effectful);
    assert!(!effectful.contains("call double @js_value_from_literal_descriptor("));
}

#[test]
fn wide_shape_constructor_uses_the_shared_assignment_loop() {
    // Below the descriptor node threshold: exercise the actual constructor
    // call, not only an otherwise-unused constructor emitted beside a blob.
    let ir = ir(&fixture(1, 128));
    assert!(!ir.contains("call double @js_value_from_literal_descriptor("));
    assert_eq!(
        ir.matches("call void @js_literal_shape_initialize(")
            .count(),
        1
    );
    assert!(
        ir.contains("noinline"),
        "LLVM must not duplicate the marshalling body"
    );
}
