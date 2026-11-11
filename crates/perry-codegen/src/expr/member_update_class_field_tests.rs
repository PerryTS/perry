//! `this.f++` / `--this.f` on a class-instance receiver takes the class-field
//! read and store routes (`member_update::class_field_update`).
//!
//! Before, an update the facts could not prove into a raw-f64 lane went by
//! name (`js_object_get_field_by_name_f64` + `js_put_value_set`), while
//! `this.f = this.f + 1` used the class-field shape sites. In the chess engine's
//! search, `this.nodes++` and `this.fullmove++` cost 10% of all instructions
//! that way.
//!
//! What must hold:
//!
//! 1. **No by-name read or write** for the update, prefix or postfix.
//! 2. **ToNumeric stays reachable**: a value that is not a plain double still
//!    reaches `js_to_numeric` + `js_numeric_step` (strings, BigInt, objects
//!    with `valueOf`), on the cold arm of a plain-double test.
use crate::compile_module;
use perry_hir::types::Type;
use perry_hir::{BinaryOp, Class, ClassField, Expr, Function, Module, ModuleInitKind, Stmt};

fn counter_class() -> Class {
    Class {
        id: 101,
        name: "Counter".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![ClassField {
            origin: perry_hir::ClassFieldOrigin::Definition,
            name: "n".to_string(),
            key_expr: None,
            ty: Type::Number,
            init: Some(Expr::Number(0.0)),
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

/// `class Counter { n = 0; probe() { return <update of this.n> } }`, with a
/// call `new Counter().probe()` so the method is reachable.
fn probe_ir(op: BinaryOp, prefix: bool) -> String {
    let mut m = Module::new("member_update_class_field.ts");
    let mut class = counter_class();
    class.methods = vec![Function {
        id: 1,
        name: "probe".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(Expr::PropertyUpdate {
            object: Box::new(Expr::This),
            property: "n".to_string(),
            op,
            prefix,
            strict: true,
            byte_offset: 0,
            byte_end: 0,
        }))],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }];
    m.classes = vec![class];
    m.init_kind = ModuleInitKind::Eager;
    let ir =
        String::from_utf8(compile_module(&m, super::class_field_barrier_tests::ir_opts()).unwrap())
            .unwrap();
    probe_bodies(&ir)
}

/// Every `probe` function body in `ir` (the entry and its clones).
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
fn class_field_update_takes_the_class_field_routes() {
    for (op, prefix) in [
        (BinaryOp::Add, false),
        (BinaryOp::Add, true),
        (BinaryOp::Sub, false),
        (BinaryOp::Sub, true),
    ] {
        let body = probe_ir(op, prefix);
        assert!(
            !body.contains("@js_object_get_field_by_name_f64("),
            "{op:?} prefix={prefix}: the update must not read by name:\n{body}"
        );
        assert!(
            !body.contains("@js_put_value_set("),
            "{op:?} prefix={prefix}: the update must not write by name:\n{body}"
        );
        assert!(
            body.contains("field_update.number") && body.contains("field_update.numeric"),
            "{op:?} prefix={prefix}: the plain-double test and its cold arm must be emitted:\n{body}"
        );
        assert!(
            body.contains("@js_to_numeric(") && body.contains("@js_numeric_step("),
            "{op:?} prefix={prefix}: a non-double value must still reach ToNumeric:\n{body}"
        );
    }
}
