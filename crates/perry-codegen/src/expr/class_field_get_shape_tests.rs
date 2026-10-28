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
            byte_end: 0,
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

#[test]
fn memo_hits_share_a_managed_receiver_handle() {
    let mut m = probe_module(Type::Any);
    let get = Expr::PropertyGet {
        byte_end: 0,
        byte_offset: 0,
        object: Box::new(Expr::LocalGet(PARAM_ID)),
        property: "x".into(),
    };
    m.functions[0].body = vec![Stmt::Return(Some(Expr::Binary {
        op: perry_hir::BinaryOp::Add,
        left: Box::new(get.clone()),
        right: Box::new(get),
    }))];
    let ir =
        String::from_utf8(compile_module(&m, super::class_field_barrier_tests::ir_opts()).unwrap())
            .unwrap();
    let body = probe_bodies(&ir);
    assert!(
        body.contains("pget.class.memo_clear"),
        "fixture must reuse the proof: {body}"
    );
    if crate::codegen::helpers::native_stack_roots_enabled() {
        assert!(
            body.contains("getelementptr i8, ptr addrspace(1)"),
            "memo hits must address the managed handle: {body}"
        );
        assert!(body.contains("load ptr addrspace(1)"));
        #[cfg(feature = "llvm-inprocess")]
        crate::inprocess::statepoint_rewritten_ir(
            &ir,
            &crate::codegen::default_target_triple(),
            "class_memo_handle",
        )
        .expect("the managed handle must survive RS4GC verification");
    }
}
