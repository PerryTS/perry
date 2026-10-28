//! A direct call must require both receiver and prototype shape authority.
use crate::compile_module;
use perry_hir::types::Type;
use perry_hir::{Class, ClassField, Expr, Function, Module, ModuleInitKind, Param, Stmt};
const PARAM_ID: u32 = 3;
fn field(name: &str, ty: Type) -> ClassField {
    ClassField {
        origin: perry_hir::ClassFieldOrigin::Definition,
        name: name.to_string(),
        key_expr: None,
        ty,
        init: None,
        is_private: false,
        is_readonly: false,
        decorators: Vec::new(),
    }
}

fn point_class(field_ty: Type) -> Class {
    Class {
        id: 101,
        name: "Meter".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![field("x", field_ty)],
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

/// `function probe(p: Meter) { return p.x }` — a bare `Named` parameter, which
/// is what routes to the generic `class_field_get.*` tower (#8033: the
/// numeric-specific `class_field_get_number.*` path needs a
/// `stable_local_type_proof` a parameter has not got).
fn probe_module(field_ty: Type) -> Module {
    let mut m = Module::new("accessor_guard.ts");
    let mut class = point_class(field_ty);
    let mut getter = Function {
        id: 200,
        name: "get_value".into(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(Expr::PropertyGet {
            byte_end: 0,
            object: Box::new(Expr::This),
            property: "x".into(),
            byte_offset: 0,
        }))],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    };
    class.getters.push(("value".into(), getter.clone()));
    getter.id = 201;
    getter.name = "set_value".into();
    getter.params.push(Param {
        id: 4,
        name: "value".into(),
        ty: Type::Any,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    });
    getter.body = vec![Stmt::Expr(Expr::PropertySet {
        object: Box::new(Expr::This),
        property: "x".into(),
        value: Box::new(Expr::LocalGet(4)),
    })];
    class.setters.push(("value".into(), getter));
    m.classes = vec![class];
    m.functions = vec![Function {
        id: 1,
        name: "probe".to_string(),
        type_params: Vec::new(),
        params: vec![Param {
            id: PARAM_ID,
            name: "p".to_string(),
            ty: Type::Named("Meter".to_string()),
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Any,
        body: vec![
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::LocalGet(PARAM_ID)),
                property: "value".into(),
                value: Box::new(Expr::Number(7.0)),
            }),
            Stmt::Return(Some(Expr::PropertyGet {
                byte_end: 0,
                byte_offset: 0,
                object: Box::new(Expr::LocalGet(PARAM_ID)),
                property: "value".to_string(),
            })),
        ],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }];
    m.init_kind = ModuleInitKind::Eager;
    m
}

fn emit(static_shapes: bool) -> String {
    let m = probe_module(Type::Any);
    let mut opts = super::ir_opts(false, None);
    opts.target = Some("x86_64-unknown-linux-gnu".into());
    if static_shapes {
        let births = crate::module_birth_shapes(&m, opts.clone()).unwrap();
        opts.static_shape_ids = crate::assign_static_shape_ids(births.iter().map(|b| &b.shape))
            .into_iter()
            .collect();
    }
    String::from_utf8(compile_module(&m, opts).unwrap()).unwrap()
}
fn probe(ir: &str) -> &str {
    ir.split("\ndefine ")
        .find(|f| f.starts_with("double @perry_fn_accessor_guard_ts__probe("))
        .unwrap_or_else(|| panic!("probe missing: {ir}"))
}
#[test]
fn compiled_accessors_need_live_receiver_and_holder_proofs() {
    let ir = emit(true);
    let p = probe(&ir);
    assert!(
        p.contains("call i32 @js_class_accessor_shape_guard("),
        "complete shared proof absent: {p}"
    );
    assert!(
        p.contains("label %named_accessor.hit"),
        "direct arm unreachable: {p}"
    );
    for half in ["get_get", "set_set"] {
        let target = format!("call double @perry_method_accessor_guard_ts__Meter____{half}_value");
        assert!(p.contains(&target), "compiled {half} body not called: {p}");
    }
    assert!(
        !p.contains("call double %"),
        "direct arm became generic invoke: {p}"
    );
    assert!(p.contains("call i32 @js_class_accessor_shape_guard("));
    assert!(p.contains("call double @js_get_property("));
    assert!(p.contains("call double @js_put_value_set("));
}
#[test]
fn absent_prototype_shape_is_a_negative_control() {
    let ir = emit(false);
    let p = probe(&ir);
    assert!(
        !p.contains("named_accessor.hit"),
        "missing authority still called body: {p}"
    );
    assert!(!p.contains("js_class_accessor_shape_guard"));
}

#[test]
fn inherited_accessor_emits_receiver_body_with_lexical_home() {
    let mut m = probe_module(Type::Any);
    let mut child = point_class(Type::Any);
    child.name = "Child".into();
    child.id = 102;
    child.fields.clear();
    child.extends = Some(101);
    child.extends_name = Some("Meter".into());
    m.classes.push(child);
    m.functions[0].params[0].ty = Type::Named("Child".into());
    let classes = m.classes.iter().map(|c| (c.name.clone(), c)).collect();
    let dispatch = crate::collectors::collect_module_dispatch_facts(&m);
    let fact = crate::collectors::inherited_accessor_proven_this(
        &m.classes[1],
        &m.classes[0],
        &m.classes[0].getters[0].1,
        &classes,
        &dispatch,
    );
    assert!(fact.is_some(), "inherited this flow rejected");
    let mut opts = super::ir_opts(false, None);
    opts.target = Some("x86_64-unknown-linux-gnu".into());
    let births = crate::module_birth_shapes(&m, opts.clone()).unwrap();
    opts.static_shape_ids = crate::assign_static_shape_ids(births.iter().map(|b| &b.shape))
        .into_iter()
        .collect();
    let ir = String::from_utf8(compile_module(&m, opts).unwrap()).unwrap();
    assert!(ir.contains("$receiver"), "inherited body absent: {ir}");
    assert!(
        probe(&ir).contains("$receiver"),
        "inherited body not selected: {}",
        probe(&ir)
    );
}

#[test]
fn getter_number_contract_requires_reachable_numeric_stores() {
    use std::collections::HashSet;
    let mut m = probe_module(Type::Number);
    m.classes[0].getters[0].1.return_type = Type::Number;
    let classes = m.classes.iter().map(|c| (c.name.clone(), c)).collect();
    let mut fact = crate::collectors::PtrShapeLocal {
        family: None,
        class_name: "Meter".into(),
        numeric_fields: HashSet::from(["x".into()]),
        report_name: None,
    };
    assert_eq!(
        crate::codegen::accessor_numeric::numeric_get_properties(&fact, &classes),
        HashSet::from(["value".into()])
    );
    fact.numeric_fields.clear();
    assert!(
        crate::codegen::accessor_numeric::numeric_get_properties(&fact, &classes).is_empty(),
        "a number annotation licensed a numeric result"
    );
}
