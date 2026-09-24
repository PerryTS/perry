//! Emission pins for the hit-path audit's array, typed-array and class-field
//! access changes. Each test names the removed out-of-line work and the
//! fallback that must remain; the behavioural half of each change is a
//! `test-files/test_gap_*` fixture compared against node.

use super::class_field_barrier_tests::block_body;
use crate::testing::root_slots::function_slice;
use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Class, ClassField, Expr, Function, Module, Param, Stmt};

fn param(id: u32, ty: Type) -> Param {
    Param {
        id,
        name: format!("p{id}"),
        ty,
        default: None,
        decorators: vec![],
        is_rest: false,
        arguments_object: None,
    }
}

fn function(params: Vec<Param>, body: Vec<Stmt>) -> Function {
    Function {
        id: 1,
        name: "probe".into(),
        type_params: vec![],
        params,
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

/// The public entry plus every clone of `probe` — a typed parameter may route
/// the body through a specialized clone or the erased `$generic` fallback.
fn probe_ir(module: &Module) -> String {
    let ir = String::from_utf8(
        compile_module(
            module,
            CompileOptions {
                emit_ir_only: true,
                is_entry_module: false,
                ..CompileOptions::default()
            },
        )
        .expect("compile access probe"),
    )
    .unwrap();
    let prefix = format!("perry_fn_{}__probe", module.name.replace('.', "_"));
    let mut body = String::new();
    for line in ir.lines() {
        if line.starts_with("define ") && line.contains(&format!("@{prefix}")) {
            let name_start = line.find('@').unwrap() + 1;
            let name_end = line[name_start..].find('(').unwrap() + name_start;
            body.push_str(function_slice(&ir, &line[name_start..name_end]));
            body.push('\n');
        }
    }
    assert!(!body.is_empty(), "no probe function in:\n{ir}");
    body
}

fn module(name: &str, params: Vec<Param>, body: Vec<Stmt>) -> Module {
    let mut m = Module::new(name);
    m.functions.push(function(params, body));
    m
}

fn named(name: &str) -> Type {
    Type::Named(name.to_string())
}

/// `probe(a: Float64Array) { return a.length }` reads the typed array's header
/// inline. The name-keyed runtime lookup (which heap-copied "length") is only
/// the fallback.
#[test]
fn declared_typed_array_length_reads_the_header_inline() {
    let ir = probe_ir(&module(
        "ta_length",
        vec![param(1, named("Float64Array"))],
        vec![Stmt::Return(Some(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(1)),
            property: "length".into(),
            byte_offset: 0,
        }))],
    ));
    let arm = block_body(&ir, "plen.typed_array")
        .unwrap_or_else(|| panic!("no typed-array length arm:\n{ir}"));
    assert!(
        arm.contains("icmp eq i8") && arm.contains(", 11"),
        "the arm must test GC_TYPE_TYPED_ARRAY:\n{arm}"
    );
    assert!(
        arm.contains("@PERRY_TA_VIEW_GUARD") && arm.contains("@PERRY_TA_OWN_PROPS_PRESENT"),
        "a view or an own `length` property must keep the header read off:\n{arm}"
    );
}

/// `probe(a: number[], k: number, v: number) { a[k] = v }` — an index with no
/// static range proof — takes the guarded in-bounds store for a canonical
/// element index and calls the exact key helper only on a guard miss.
#[test]
fn unproven_numeric_index_store_has_an_inline_element_tier() {
    let ir = probe_ir(&module(
        "array_runtime_key",
        vec![
            param(1, Type::Array(Box::new(Type::Number))),
            param(2, Type::Number),
            param(3, Type::Number),
        ],
        vec![Stmt::Expr(Expr::IndexSet {
            object: Box::new(Expr::LocalGet(1)),
            index: Box::new(Expr::LocalGet(2)),
            value: Box::new(Expr::LocalGet(3)),
        })],
    ));
    assert!(
        ir.contains("idxset.runtime_key.fast"),
        "the canonical element index must reach the inline store:\n{ir}"
    );
    let slow = block_body(&ir, "idxset.runtime_key.slow")
        .unwrap_or_else(|| panic!("no helper arm:\n{ir}"));
    assert!(
        slow.contains("@js_typed_feedback_array_set_index_or_string("),
        "a declined key must still reach the exact helper:\n{slow}"
    );
}

/// A declared typed array with an unproven index takes the same guarded inline
/// arms as an erased receiver, not an unconditional runtime call per access.
#[test]
fn declared_typed_array_unproven_index_access_is_inline() {
    let get = probe_ir(&module(
        "ta_dynamic_get",
        vec![param(1, named("Float64Array")), param(2, Type::Number)],
        vec![Stmt::Return(Some(Expr::IndexGet {
            object: Box::new(Expr::LocalGet(1)),
            index: Box::new(Expr::LocalGet(2)),
        }))],
    ));
    assert!(
        get.contains("tav.brand") && !get.contains("@js_typed_array_index_get_dynamic("),
        "declared typed-array read must use the inline arm:\n{get}"
    );
    let set = probe_ir(&module(
        "ta_dynamic_set",
        vec![
            param(1, named("Float64Array")),
            param(2, Type::Number),
            param(3, Type::Number),
        ],
        vec![Stmt::Expr(Expr::IndexSet {
            object: Box::new(Expr::LocalGet(1)),
            index: Box::new(Expr::LocalGet(2)),
            value: Box::new(Expr::LocalGet(3)),
        })],
    ));
    // The erased `$generic` clone keeps the runtime dispatcher: it has no
    // typed-array receiver to route. What must change is the guarded body.
    assert!(
        set.contains("tav.set.fast"),
        "declared typed-array store must use the inline arm:\n{set}"
    );
}

/// The inline typed-array store admits only plain doubles (anything NaN-boxed
/// needs the runtime's ToNumber), and its integer kinds use the exact modular
/// ToInt32 — the unwrapped conversion is poison for |v| >= 2^63.
#[test]
fn inline_typed_array_store_rejects_boxed_values_and_wraps_exactly() {
    let ir = probe_ir(&module(
        "ta_erased_set",
        vec![
            param(1, Type::Any),
            param(2, Type::Any),
            param(3, Type::Any),
        ],
        vec![Stmt::Expr(Expr::IndexSet {
            object: Box::new(Expr::LocalGet(1)),
            index: Box::new(Expr::LocalGet(2)),
            value: Box::new(Expr::LocalGet(3)),
        })],
    ));
    assert!(
        ir.lines()
            .any(|line| line.contains("icmp slt i64") && line.contains("9221401712017801216")),
        "the entry guard must reject NaN-boxed values:\n{ir}"
    );
    let store = block_body(&ir, "tav.set.store").unwrap_or_else(|| panic!("no store block:\n{ir}"));
    assert!(
        !store.contains("fptosi"),
        "integer kinds must not use the |v| < 2^63-only conversion:\n{store}"
    );
}

/// `probe(a: number, b: number) { return [a, b, 3] }` — plain doubles are
/// stored raw under a header that already carries the raw-f64 flag; the noted
/// path and the marking walk remain only for a NaN-boxed element.
#[test]
fn plain_double_array_literal_skips_notes_and_marking() {
    let ir = probe_ir(&module(
        "numeric_literal",
        vec![param(1, Type::Number), param(2, Type::Number)],
        vec![Stmt::Return(Some(Expr::Array(vec![
            Expr::LocalGet(1),
            Expr::LocalGet(2),
            Expr::Number(3.0),
        ])))],
    ));
    let plain = block_body(&ir, "arrlit.plain_numbers")
        .unwrap_or_else(|| panic!("no plain-number arm:\n{ir}"));
    // `call i64 asm ""` is the RS4GC root-reload launder, not a runtime call.
    assert!(
        !plain
            .lines()
            .any(|line| line.contains("call ") && !line.contains("asm \"\"")),
        "the plain arm stores raw with no runtime calls:\n{plain}"
    );
    let noted = block_body(&ir, "arrlit.noted").unwrap_or_else(|| panic!("no noted arm:\n{ir}"));
    assert!(
        noted.contains("@js_array_mark_numeric_f64_layout("),
        "a boxed element must keep the marking walk:\n{noted}"
    );
}

fn point_class() -> Class {
    Class {
        id: 101,
        name: "Point".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![ClassField {
            name: "x".to_string(),
            key_expr: None,
            ty: Type::Number,
            init: None,
            is_private: false,
            is_readonly: false,
            decorators: Vec::new(),
        }],
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

/// `probe(p: Point) { return p.x }` (a raw-f64 field) — the class-field READ
/// guard loads the class's own ShapeId global, the (class id, ShapeId) word and
/// the `_reserved` half-word for the intact bit, and NOTHING else: the GcHeader word the write guard still tests (kind,
/// forwarded, descriptor and tombstone flags) is carried by the ShapeId on a
/// read (`emit_class_field_read_precheck`).
///
/// Neither the retired `@PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED` latch nor
/// the poisonable `@perry_class_guard_shape_*` twin that replaced it may come
/// back: the expectation is the class ShapeId itself (S6).
#[test]
fn class_field_read_guard_loads_identity_expectation_and_intact_bit_only() {
    let mut m = module(
        "class_field_fused",
        vec![param(1, named("Point"))],
        vec![Stmt::Return(Some(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(1)),
            property: "x".into(),
            byte_offset: 0,
        }))],
    );
    m.classes = vec![point_class()];
    let ir = probe_ir(&m);
    let deref = block_body(&ir, "class_field_inline.deref")
        .unwrap_or_else(|| panic!("no inline guard:\n{ir}"));
    let loads: Vec<&str> = deref.lines().filter(|l| l.contains(" = load ")).collect();
    assert_eq!(
        loads.len(),
        3,
        "the read guard must load the identity word, the live expectation and \
         the _reserved half-word, and nothing else (no GcHeader word):\n{deref}"
    );
    assert!(
        !ir.contains("@PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED"),
        "the per-access latch must be GONE from the class-field guard — its \
         authority moved onto the expectation this guard already loads:\n{ir}"
    );
    assert!(
        deref.contains("load volatile i32, ptr @perry_class_shape_id_")
            && !ir.contains("perry_class_guard_shape_"),
        "the expectation must be the class's own ShapeId global, never a \
         poisonable twin:\n{deref}"
    );
    assert!(
        loads.iter().any(|l| l.contains("load i64"))
            && loads.iter().any(|l| l.contains("load i16")),
        "one 64-bit (class id, ShapeId) identity word and the intact half-word:\n{deref}"
    );
}
