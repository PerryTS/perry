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

fn literal_new() -> Expr {
    Expr::New {
        class_name: "__AnonShape_descriptor_test".into(),
        args: vec![Expr::Number(1.0), Expr::Number(2.0)],
        type_args: vec![],
        byte_offset: 0,
        cap_args_appended: 0,
    }
}

fn literal_binding(init: Expr) -> Stmt {
    Stmt::Let {
        id: 1001,
        name: "record".into(),
        ty: Type::Any,
        mutable: false,
        init: Some(init),
    }
}

fn added_store(receiver: Expr, key: &str) -> Stmt {
    Stmt::Expr(Expr::PropertySet {
        object: Box::new(receiver),
        property: key.into(),
        value: Box::new(Expr::Number(3.0)),
    })
}

fn assert_literal_width(module: &Module, width: u32) {
    let text = ir(module);
    let allocations: Vec<_> = text
        .lines()
        .filter(|l| l.contains("call i64 @js_object_alloc_class_inline_keys_stamped("))
        .collect();
    assert!(
        !allocations.is_empty(),
        "fixture must emit a literal birth: {text}"
    );
    for line in allocations {
        assert!(
            line.contains(&format!(", i32 0, i32 {width}, i64 ")),
            "wrong allocation width: {line}"
        );
    }
    if width > 2 {
        let mints: Vec<_> = text
            .lines()
            .filter(|l| l.contains("call i32 @js_object_shape_id_for_class_keys_live("))
            .collect();
        assert_eq!(mints.len(), 1, "fixture must mint one wide birth image");
        assert!(
            mints[0].contains(&format!(", i32 2, i32 {width}, i32 ")),
            "wrong live bound: {}",
            mints[0]
        );
        let packed = crate::target_layout::inline_alloc_gc_packed("aarch64-apple-darwin", width);
        assert!(
            text.contains(&format!("<i64 {packed}, i64 0>")),
            "wide allocation and image size must agree"
        );
    } else {
        assert!(
            !text.contains("call i32 @js_object_shape_id_for_class_keys_live("),
            "exact literal must not be widened"
        );
    }
}

#[test]
fn born_wide_literal_local_and_module_binding() {
    let mut module = fixture(1, 2);
    module.init = vec![
        literal_binding(literal_new()),
        added_store(Expr::LocalGet(1001), "z"),
    ];
    assert_literal_width(&module, 3);
}

#[test]
fn born_wide_factory_uses_return_shape_fact() {
    let mut module = fixture(1, 2);
    let mut producer = module.classes[0].constructor.as_ref().unwrap().clone();
    producer.id = 2001;
    producer.name = "make".into();
    producer.params.clear();
    producer.return_type = Type::Any;
    producer.body = vec![Stmt::Return(Some(literal_new()))];
    module.functions.push(producer);
    module.init = vec![
        literal_binding(Expr::Call {
            callee: Box::new(Expr::FuncRef(2001)),
            args: vec![],
            type_args: vec![],
            byte_offset: 0,
        }),
        added_store(Expr::LocalGet(1001), "z"),
    ];
    assert_literal_width(&module, 3);
}

fn literal_closure(body: Vec<Stmt>, captures: Vec<u32>) -> Expr {
    Expr::Closure {
        func_id: 2002,
        params: vec![],
        return_type: Type::Void,
        body,
        captures,
        mutable_captures: vec![],
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: false,
        is_async: false,
        is_generator: false,
        is_strict: true,
    }
}

#[test]
fn born_wide_captured_literal() {
    let mut module = fixture(1, 2);
    module.init = vec![
        literal_binding(literal_new()),
        Stmt::Expr(literal_closure(
            vec![added_store(Expr::LocalGet(1001), "z")],
            vec![1001],
        )),
    ];
    assert_literal_width(&module, 3);
}

fn method_fixture() -> Module {
    let mut module = fixture(1, 2);
    // The method is a dynamic-this closure in a literal constructor argument.
    let mut init = literal_new();
    if let Expr::New { args, .. } = &mut init {
        args[1] = literal_closure(vec![added_store(Expr::This, "z")], vec![]);
    }
    module.classes[0].fields[1].ty = Type::Any;
    module.classes[0].constructor.as_mut().unwrap().params[1].ty = Type::Any;
    // An array element escapes scalar replacement; only method `this`
    // supplies capacity evidence for this literal.
    module.init = vec![Stmt::Expr(Expr::Array(vec![init]))];
    module
}

#[test]
fn born_wide_literal_method_this() {
    assert_literal_width(&method_fixture(), 3);
}

#[test]
fn born_wide_constfn_final_preserves_reserved_capacity() {
    let module = method_fixture();
    let mut options = super::super::class_field_barrier_tests::ir_opts();
    options.output_type = "executable".into();
    let births = crate::module_birth_shapes(&module, options.clone()).unwrap();
    let finals: Vec<_> = births
        .iter()
        .filter(|birth| !birth.shape.constfn.is_empty())
        .collect();
    assert_eq!(
        finals.len(),
        1,
        "method fixture must produce a ConstFn final shape"
    );
    assert_eq!(finals[0].shape.key_count, 2);
    assert_eq!(finals[0].shape.live, 3);
    options.static_shape_ids =
        crate::assign_static_shape_ids(births.iter().map(|birth| &birth.shape))
            .into_iter()
            .collect();
    let text = String::from_utf8(crate::compile_module(&module, options).unwrap()).unwrap();
    let calls: Vec<_> = text
        .lines()
        .filter(|line| line.contains("call i64 @js_object_finalize_constfn_static("))
        .collect();
    assert_eq!(calls.len(), 1, "finalization must be emitted");
    assert!(
        calls[0].contains(", i32 2, i32 3, i32 0, i64 "),
        "finalization must preserve live width: {}",
        calls[0]
    );
}

#[test]
fn born_wide_precision_filters_and_cap() {
    let mut module = fixture(1, 2);
    module.init = vec![
        literal_binding(literal_new()),
        added_store(Expr::LocalGet(9999), "z"),
        added_store(Expr::LocalGet(1001), "k0"),
        added_store(Expr::LocalGet(1001), "#private"),
        added_store(Expr::LocalGet(1001), "0"),
    ];
    assert_literal_width(&module, 2);
    module.init = vec![literal_binding(literal_new())];
    for i in 0..12 {
        module
            .init
            .push(added_store(Expr::LocalGet(1001), &format!("added{i}")));
    }
    assert_literal_width(&module, 10);
    module
        .init
        .push(Stmt::Expr(Expr::LocalSet(1001, Box::new(literal_new()))));
    assert_literal_width(&module, 2);
}

#[test]
fn born_wide_descriptor_keeps_payload_count_and_allocation_width_distinct() {
    let mut module = fixture(3200, 2);
    module.init.insert(0, literal_binding(literal_new()));
    module.init.push(added_store(Expr::LocalGet(1001), "z"));
    let text = ir(&module);
    assert!(
        text.contains("call double @js_value_from_literal_descriptor("),
        "descriptor fixture must cross the materializer threshold"
    );
    let shape = text
        .lines()
        .find(|l| l.contains("_shapes = private constant"))
        .unwrap();
    assert!(
        shape.contains("i32 2, ptr"),
        "payload arity remains the key count"
    );
    assert!(
        shape.contains(", i32 3, i64 "),
        "descriptor must carry the shared birth width: {shape}"
    );
    assert_literal_width(&module, 3);
}

#[test]
fn born_wide_alias_shares_one_image_at_the_maximum_width() {
    let mut module = fixture(1, 2);
    module.classes[0].aliases.push("literal_alias".into());
    let mut aliased = literal_new();
    if let Expr::New { class_name, .. } = &mut aliased {
        *class_name = "literal_alias".into();
    }
    module.init = vec![
        literal_binding(aliased),
        added_store(Expr::LocalGet(1001), "z"),
        Stmt::Expr(literal_new()),
    ];
    assert_literal_width(&module, 3);
}

#[test]
fn born_wide_global_binding_and_static_putvalue_key() {
    let mut module = fixture(1, 2);
    module.globals.push(perry_hir::Global {
        id: 1001,
        name: "global_record".into(),
        ty: Type::Any,
        mutable: false,
        init: Some(literal_new()),
    });
    module.init = vec![Stmt::Expr(Expr::PutValueSet {
        target: Box::new(Expr::GlobalGet(1001)),
        receiver: Box::new(Expr::GlobalGet(1001)),
        key: Box::new(Expr::String("z".into())),
        value: Box::new(Expr::Number(3.0)),
        strict: false,
    })];
    // HIR globals may be materialized by an earlier pass; assert the capacity
    // evidence directly as well as emitting an ordinary birth of the class.
    let classes = module.classes.iter().map(|c| (c.name.clone(), c)).collect();
    let facts = crate::collectors::collect_module_dispatch_facts(&module);
    let adds =
        crate::collectors::anon_key_adds::anon_receiver_added_keys(&module, &classes, &facts);
    assert_eq!(adds["__AnonShape_descriptor_test"].len(), 1);
    module.init.push(Stmt::Expr(literal_new()));
    assert_literal_width(&module, 3);
}

#[test]
fn born_wide_closure_factory_uses_the_same_return_facts() {
    let mut module = fixture(1, 2);
    let mut producer = literal_closure(vec![Stmt::Return(Some(literal_new()))], vec![]);
    if let Expr::Closure { return_type, .. } = &mut producer {
        *return_type = Type::Any;
    }
    module.init = vec![
        Stmt::Let {
            id: 2003,
            name: "make".into(),
            ty: Type::Any,
            mutable: false,
            init: Some(producer),
        },
        literal_binding(Expr::Call {
            callee: Box::new(Expr::LocalGet(2003)),
            args: vec![],
            type_args: vec![],
            byte_offset: 0,
        }),
        added_store(Expr::LocalGet(1001), "z"),
    ];
    assert_literal_width(&module, 3);
}

#[test]
fn born_wide_parameter_evidence_is_deferred() {
    let mut module = fixture(1, 2);
    let mut consumer = module.classes[0].constructor.as_ref().unwrap().clone();
    consumer.id = 2004;
    consumer.name = "consume".into();
    consumer.params.truncate(1);
    consumer.params[0].id = 3001;
    consumer.params[0].ty = Type::Any;
    consumer.body = vec![added_store(Expr::LocalGet(3001), "z")];
    module.functions.push(consumer);
    module.init = vec![
        literal_binding(literal_new()),
        Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::FuncRef(2004)),
            args: vec![Expr::LocalGet(1001)],
            type_args: vec![],
            byte_offset: 0,
        }),
    ];
    assert_literal_width(&module, 2);
}
