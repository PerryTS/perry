//! A class-field read is a shape-site read (#12483).
//!
//! `p.x` on a receiver whose class the compiler names used to be lowered by its
//! own tower: an inline pre-check against the class's BIRTH ShapeId, then a
//! slot load, with every miss sent to `js_class_field_get_ic`. A receiver that
//! left the birth shape (a plain JavaScript class, whose keys are constructor
//! stores; a `: any` field whose lane generalized; a subclass the arm list
//! could not name) missed that pre-check on every read and paid the call: 150
//! instructions per `this.a` in an untyped `.js` class, 423-1,429 for a
//! `a: any = 1` field, against ~12 at the shape site.
//!
//! Now the read is the ordinary site: the receiver test, the receiver's
//! ShapeId compared against the site's, one slot load. Which ShapeId the site
//! compares is the only choice: the class's own when every instance is born on
//! it and keeps it and no subclass exists (`class_field_predicted_shape`, a
//! constant two reads of one receiver can share), else the site's learned
//! word. What must hold:
//!
//! 1. **The shape site is emitted.** A predicted read compares in
//!    `pget.class.token` and misses into the complete site operation
//!    (`js_object_get_field_ic`); any other read loads its `_packed_get` word.
//! 2. **No class guard is emitted for the read**: no `class_field_inline.*` or
//!    `class_field_get.*` block.
//! 3. **No class-field read helper is called**, in any representation: the
//!    boxed field, the raw-f64 field (value and number context) and a class
//!    with subclasses all take the same site.
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
        name: "Point".to_string(),
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

/// `function probe(p: Point) { return p.x }` — a bare `Named` parameter: the
/// compiler names the receiver's class, and proves nothing about its shape.
fn probe_module(field_ty: Type) -> Module {
    let mut m = Module::new("class_field_get_shape.ts");
    m.classes = vec![point_class(field_ty)];
    m.functions = vec![Function {
        id: 1,
        name: "probe".to_string(),
        type_params: Vec::new(),
        params: vec![Param {
            id: PARAM_ID,
            name: "p".to_string(),
            ty: Type::Named("Point".to_string()),
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(Expr::PropertyGet {
            byte_offset: 0,
            object: Box::new(Expr::LocalGet(PARAM_ID)),
            property: "x".to_string(),
        }))],
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

/// Every `probe` function body (the entry and its clones) in `ir`, and
/// nothing else: the class constructors and the module init in the same
/// module carry their own class guards and name the class ShapeId.
fn probe_bodies(ir: &str) -> String {
    let mut out = String::new();
    let mut from = 0;
    while let Some(rel) = ir[from..].find("\ndefine ") {
        let start = from + rel + 1;
        let end = ir[start..]
            .find("\n}\n")
            .map(|e| start + e + 3)
            .unwrap_or(ir.len());
        let header = &ir[start..start + ir[start..].find('\n').unwrap_or(0)];
        if header.contains("__probe") {
            out.push_str(&ir[start..end]);
            out.push('\n');
        }
        from = end;
    }
    assert!(!out.is_empty(), "no probe function in:\n{ir}");
    out
}

/// `probe(p: Point)` where `Point` has `subclasses` field-less subclasses.
fn hierarchy_ir(subclasses: u32) -> String {
    let mut m = probe_module(Type::Any);
    for i in 0..subclasses {
        let mut sub = point_class(Type::Any);
        sub.id = 200 + i;
        sub.name = format!("Sub{i}");
        sub.extends_name = Some("Point".to_string());
        sub.fields = Vec::new();
        m.classes.push(sub);
    }
    String::from_utf8(
        compile_module(&m, super::class_field_barrier_tests::ir_opts()).expect("module compiles"),
    )
    .expect("LLVM IR should be UTF-8")
}

/// The probe's IR for `field_ty`, with `x` born on an `F64` lane when `raw`.
fn probe_ir(field_ty: Type, raw: bool, number_context: bool) -> String {
    let mut m = probe_module(field_ty);
    if raw {
        m.classes[0].fields[0].init = Some(Expr::Number(0.0));
    }
    if number_context {
        // `return p.x + 1`: a numeric consumer asks for the field as a Number
        // first (`lower_raw_f64_class_field_get_for_number_context`).
        let Stmt::Return(Some(read)) = m.functions[0].body.pop().expect("probe body") else {
            unreachable!("probe returns its read")
        };
        m.functions[0].body.push(Stmt::Return(Some(Expr::Binary {
            op: perry_hir::BinaryOp::Add,
            left: Box::new(read),
            right: Box::new(Expr::Number(1.0)),
        })));
    }
    String::from_utf8(
        compile_module(&m, super::class_field_barrier_tests::ir_opts()).expect("module compiles"),
    )
    .expect("LLVM IR should be UTF-8")
}

fn assert_shape_site(probe: &str, what: &str, predicted: bool) {
    if predicted {
        assert!(
            probe.contains("pget.class.token")
                && probe.contains("call double @js_object_get_field_ic("),
            "{what}: the read must compare the class's ShapeId and miss into the site:\n{probe}"
        );
        assert!(
            !probe.contains("_packed_get"),
            "{what}: a predicted read must not load a learned word:\n{probe}"
        );
    } else {
        assert!(
            probe.contains("_packed_get") && !probe.contains("pget.class.token"),
            "{what}: the read must be served by the shape site's word:\n{probe}"
        );
        assert!(
            !probe.contains("@perry_class_shape_id_"),
            "{what}: the read compares against the class's birth ShapeId:\n{probe}"
        );
    }
    for retired in ["class_field_inline.", "class_field_get."] {
        assert!(
            !probe.contains(retired),
            "{what}: a class guard block (`{retired}`) is still emitted:\n{probe}"
        );
    }
    for call in [
        "@js_class_field_get_ic",
        "call i32 @js_typed_feedback_class_field_get_guard(",
        "call double @js_object_get_field_by_name_f64(",
    ] {
        assert!(
            !probe.contains(call),
            "{what}: the read calls `{call}`:\n{probe}"
        );
    }
}

#[test]
fn boxed_class_field_read_is_the_shape_site() {
    let ir = probe_ir(Type::Any, false, false);
    assert_shape_site(&probe_bodies(&ir), "any-typed field", true);
}

#[test]
fn raw_f64_class_field_read_is_the_shape_site() {
    let ir = probe_ir(Type::Number, true, false);
    assert_shape_site(&probe_bodies(&ir), "raw-f64 field", true);
}

#[test]
fn number_context_class_field_read_is_the_shape_site() {
    let ir = probe_ir(Type::Number, true, true);
    let probe = probe_bodies(&ir);
    assert_shape_site(&probe, "raw-f64 field in a number context", true);
    assert!(
        probe.contains("fadd double"),
        "the test must observe the numeric consumer:\n{probe}"
    );
}

/// One mechanism whatever the hierarchy: a class with one subclass (which the
/// old tower served with an extra arm) and one with nine (past its arm cap,
/// which it routed to the site) both read through the site, on its word: a
/// subclass instance carries another ShapeId.
#[test]
fn class_field_reads_take_the_site_whatever_the_hierarchy() {
    for subclasses in [0, 1, 9] {
        let ir = hierarchy_ir(subclasses);
        assert_shape_site(
            &probe_bodies(&ir),
            &format!("{subclasses} subclasses"),
            subclasses == 0,
        );
    }
}

/// A plain JavaScript class: its keys are constructor stores, so instances are
/// born without them and no constant ShapeId names them. The read takes the
/// site's learned word.
#[test]
fn constructor_store_class_field_read_takes_the_word() {
    let mut m = probe_module(Type::Any);
    m.classes[0].fields[0].origin = perry_hir::ClassFieldOrigin::ConstructorStore;
    let ir = String::from_utf8(
        compile_module(&m, super::class_field_barrier_tests::ir_opts()).expect("module compiles"),
    )
    .expect("LLVM IR should be UTF-8");
    assert_shape_site(&probe_bodies(&ir), "constructor-store field", false);
}
