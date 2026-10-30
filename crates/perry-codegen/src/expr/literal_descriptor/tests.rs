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
                    byte_offset: 0,
                    byte_end: 0,
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
                origin: perry_hir::ClassFieldOrigin::Definition,
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
                    byte_end: 0,
                    class_name: class_name.into(),
                    args: (0..fields).map(|i| Expr::Number((r + i) as f64)).collect(),
                    type_args: vec![],
                    byte_offset: 0,
                    cap_args_appended: 0,
                })
                .collect(),
            None,
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
fn a_wide_shape_has_no_constructor_function() {
    // Below the descriptor node threshold: the births are ordinary `new`s of
    // a wide literal shape. Their constant number lanes are written by the
    // birth itself; nothing emits, calls or registers a constructor.
    let ir = ir(&fixture(1, 128));
    assert!(!ir.contains("call double @js_value_from_literal_descriptor("));
    assert!(
        !ir.contains("__AnonShape_descriptor_test_constructor"),
        "a wide literal shape has no constructor function"
    );
}

#[test]
fn a_narrow_shape_has_no_constructor_function() {
    // Two fields: the shape has no constructor function either, at any width.
    let ir = ir(&fixture(1, 2));
    assert!(
        !ir.contains("__AnonShape_descriptor_test_constructor"),
        "a literal shape has no constructor function: none is emitted, declared or registered"
    );
}

fn literal_new() -> Expr {
    Expr::New {
        byte_end: 0,
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
        byte_offset: 0,
        byte_end: 0,
    })
}

fn assert_literal_width(module: &Module, width: u32) {
    let text = ir(module);
    let allocations: Vec<_> = text
        .lines()
        .filter(|l| l.contains("call i64 @js_object_alloc_class_inline_keys_stamped("))
        .collect();
    if allocations.is_empty() {
        let total = crate::target_layout::inline_alloc_total_size_bytes(
            "aarch64-apple-darwin",
            crate::target_layout::HeaderForm::Meta,
            width,
        );
        let packed = crate::target_layout::inline_alloc_gc_packed(
            "aarch64-apple-darwin",
            crate::target_layout::HeaderForm::Meta,
            width,
        );
        assert!(
            text.contains("alloc.fast"),
            "fixture must emit a literal birth: {text}"
        );
        assert!(
            text.lines().any(|line| {
                line.contains(&format!("add i64 ")) && line.ends_with(&format!(", {total}"))
            }),
            "wrong emitted allocation width: {text}"
        );
        assert!(
            text.contains(&format!("<i64 {packed}, i64 ")),
            "emitted allocation and image size must agree: {text}"
        );
    }
    for line in allocations {
        assert!(
            line.contains(&format!(", i32 0, i32 {width}, i64 ")),
            "wrong allocation width: {line}"
        );
    }
    if width > 2 {
        let mints: Vec<_> = text
            .lines()
            .filter(|l| l.contains("call i32 @js_object_layout_for_class_keys_static("))
            .collect();
        assert_eq!(mints.len(), 1, "fixture must mint one wide birth image");
        assert!(
            mints[0].contains(&format!(", i32 2, i32 {width}, i32 ")),
            "wrong live bound: {}",
            mints[0]
        );
        let packed = crate::target_layout::inline_alloc_gc_packed(
            "aarch64-apple-darwin",
            crate::target_layout::HeaderForm::Meta,
            width,
        );
        assert!(
            text.contains(&format!("<i64 {packed}, i64 ")),
            "wide allocation and image size must agree"
        );
    } else {
        assert!(
            !text.lines().any(
                |l| l.contains("call i32 @js_object_layout_for_class_keys_static(")
                    && !l.contains(", i32 2, i32 2, i32 ")
            ),
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
            byte_end: 0,
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
    module.init = vec![Stmt::Expr(Expr::Array(vec![init], None))];
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
        .filter(|line| line.contains("call ptr @perry_alloc_layout("))
        .collect();
    assert_eq!(calls.len(), 1, "finalization must be emitted");
    assert!(
        text.lines()
            .any(|line| line.contains("perry_layout_literal_")
                && line.contains("section \"perry_layouts\"")
                && line.contains("i32 3, i32 ")),
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
        byte_offset: 0,
        byte_end: 0,
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
            byte_end: 0,
            callee: Box::new(Expr::LocalGet(2003)),
            args: vec![],
            type_args: vec![],
            byte_offset: 0,
        }),
        added_store(Expr::LocalGet(1001), "z"),
    ];
    assert_literal_width(&module, 3);
}

fn consumer_fixture(module: &mut Module, body: Vec<Stmt>) {
    let mut consumer = module.classes[0].constructor.as_ref().unwrap().clone();
    consumer.id = 2004;
    consumer.name = "consume".into();
    consumer.params.truncate(1);
    consumer.params[0].id = 3001;
    consumer.params[0].ty = Type::Any;
    consumer.body = body;
    module.functions.push(consumer);
}

fn consume_call(arg: Expr) -> Stmt {
    Stmt::Expr(Expr::Call {
        byte_end: 0,
        callee: Box::new(Expr::FuncRef(2004)),
        args: vec![arg],
        type_args: vec![],
        byte_offset: 0,
    })
}

/// #12484 (phase 2): a key the callee adds to its parameter reserves room in
/// every literal the module passes there, through a binding or directly.
#[test]
fn born_wide_parameter_evidence() {
    let mut module = fixture(1, 2);
    consumer_fixture(&mut module, vec![added_store(Expr::LocalGet(3001), "z")]);
    module.init = vec![
        literal_binding(literal_new()),
        consume_call(Expr::LocalGet(1001)),
    ];
    assert_literal_width(&module, 3);

    let mut module = fixture(1, 2);
    consumer_fixture(&mut module, vec![added_store(Expr::LocalGet(3001), "z")]);
    module.init = vec![consume_call(literal_new())];
    assert_literal_width(&module, 3);
}

/// A reassigned parameter names no single argument and fails closed.
#[test]
fn born_wide_reassigned_parameter_fails_closed() {
    let mut module = fixture(1, 2);
    consumer_fixture(
        &mut module,
        vec![
            Stmt::Expr(Expr::LocalSet(3001, Box::new(Expr::Undefined))),
            added_store(Expr::LocalGet(3001), "z"),
        ],
    );
    module.init = vec![consume_call(literal_new())];
    assert_literal_width(&module, 2);
}

/// #12484 (phase 2): an element read of an array the module fills with
/// literals carries their classes, so a key added through it reserves room.
#[test]
fn born_wide_array_element_evidence() {
    let mut module = fixture(1, 2);
    module.init = vec![
        Stmt::Let {
            id: 1002,
            name: "objs".into(),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::Array(vec![], None)),
        },
        Stmt::Expr(Expr::ArrayPush {
            array_id: 1002,
            value: Box::new(literal_new()),
            field_writeback: None,
        }),
        literal_binding(Expr::IndexGet {
            byte_offset: 0,
            byte_end: 0,
            object: Box::new(Expr::LocalGet(1002)),
            index: Box::new(Expr::Number(0.0)),
        }),
        added_store(Expr::LocalGet(1001), "z"),
    ];
    assert_literal_width(&module, 3);

    let mut module = fixture(1, 2);
    module.init = vec![
        Stmt::Let {
            id: 1002,
            name: "objs".into(),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::Array(vec![literal_new()], None)),
        },
        added_store(
            Expr::IndexGet {
                byte_offset: 0,
                byte_end: 0,
                object: Box::new(Expr::LocalGet(1002)),
                index: Box::new(Expr::Number(0.0)),
            },
            "z",
        ),
    ];
    assert_literal_width(&module, 3);
}

/// The runtime finds atoms and layout descriptors by walking their sections;
/// many have no reference from code. Mach-O's `-dead_strip` dropped them
/// (ELF keeps a section its `__start_`/`__stop_` bounds name), so on macOS the
/// program's key atoms were never seeded and a coloured read answered absent
/// (zod: "Bind must be called on a function"). Every Mach-O record section
/// must carry `no_dead_strip`; ELF spells the plain section.
#[test]
fn section_walked_records_survive_mach_o_dead_strip() {
    for (target, plain, kept) in [
        (
            "aarch64-apple-darwin",
            "\"__DATA,__perry_",
            ",regular,no_dead_strip\"",
        ),
        ("x86_64-unknown-linux-gnu", "\"perry_", "\""),
    ] {
        let mut options = super::super::class_field_barrier_tests::ir_opts();
        options.output_type = "executable".into();
        options.target = Some(target.into());
        let text =
            String::from_utf8(crate::compile_module(&method_fixture(), options).unwrap()).unwrap();
        let records: Vec<&str> = text
            .lines()
            .filter(|l| {
                ["atoms", "layouts", "shnames", "shadow_names"]
                    .iter()
                    .any(|s| l.contains(&format!("section {plain}{s}")))
            })
            .collect();
        assert!(
            records.iter().any(|l| l.contains("perry_atoms")),
            "{target}: the fixture must emit atom records"
        );
        assert!(
            records.iter().any(|l| l.contains("perry_layouts")),
            "{target}: the fixture must emit layout descriptors"
        );
        for line in records {
            let section = &line[line.find("section ").unwrap() + 8..];
            let section = &section[..section[1..].find('"').unwrap() + 2];
            assert!(section.ends_with(kept), "{target}: {line}");
            assert_eq!(
                section.contains("no_dead_strip"),
                target.contains("apple"),
                "{target}: {line}"
            );
        }
    }
}

/// #10399: a program that starts workers keeps its class keys and ShapeId
/// slots per agent (thread-local), so no rodata may name them. Its descriptor
/// builds the shape table in the frame from this agent's slot addresses.
pub(crate) fn worker_program_ir() -> String {
    let module = fixture(3200, 4);
    let mut opts = super::super::class_field_barrier_tests::ir_opts();
    opts.program_has_worker = true;
    String::from_utf8(crate::compile_module(&module, opts).expect("lowers")).unwrap()
}

#[test]
fn worker_program_descriptor_names_this_agents_slots() {
    let ir = worker_program_ir();
    assert_eq!(
        ir.matches("call double @js_value_from_literal_descriptor(")
            .count(),
        1,
        "the descriptor still serves a worker program"
    );
    assert!(
        !ir.lines()
            .any(|line| line.contains("_shapes = private constant")),
        "no link-time shape table in a worker program"
    );
    assert!(
        ir.contains("call ptr @llvm.threadlocal.address.p0(ptr @perry_class_keys_"),
        "the keys slot is this agent's address"
    );
}
