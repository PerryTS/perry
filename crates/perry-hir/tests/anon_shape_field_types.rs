//! #7544 — the field types a closed-shape object literal mints.
//!
//! HIR rewrites a closed-shape literal into `new __AnonShape_<hash>(…)`, and
//! the synthesized `ClassField::ty` of that class is a **collector-facing
//! layout input**: codegen's `typed_shape::class_layout_declarable_at_allocation`
//! refuses any class with a pointer-bearing field, and `Any` is pointer-bearing.
//! So a literal that mints `Any` fields is declared to the GC as N POINTER
//! slots and takes the mask-maintaining store path; one that mints `Number`
//! fields gets a raw-f64 mask, a `POINTER_FREE` layout, and the
//! allocation-site declaration #7532 added.
//!
//! The defect: `for (let i = 0; …)` hardcoded `Type::Any` for its head
//! binding while the statement-level `let i = 0;` inferred `Number`, so every
//! literal built from a loop counter — the single most common allocation shape
//! in the language — minted `Any` fields. `{ v: i, w: i + 1 }` was two pointer
//! slots holding two doubles.
//!
//! These tests pin **both directions**. The negative cases matter as much as
//! the positive one: Perry validates no declared type at runtime, so a field
//! typed `Number` that receives a pointer relies on the raw-f64 store guard
//! rejecting it and `layout_note_slot` downgrading the descriptor. That
//! self-healing is real (see `gc/tests/layout_trace/declared_at_allocation.rs`
//! and `test-files/test_gap_7544_anon_shape_numeric_fields.ts`), but it is a
//! recovery path, not a licence — a value we cannot type must still mint
//! `Any` so the collector's view is right the first time.

use perry_diagnostics::SourceCache;
use perry_hir::types::Type;
use perry_hir::{lower_module, Module};
use perry_parser::parse_typescript_with_cache;

/// Lowering is deeply recursive; the default 2 MB test thread SIGABRTs.
fn lower_src(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = parse_typescript_with_cache(&src, "test.ts", &mut cache)
                .expect("parse should succeed");
            lower_module(&parsed.module, "test", "test.ts").expect("lower should succeed")
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

/// The minted field types of the one anon-shape class carrying `field_name`,
/// as `(name, ty)` pairs in declaration order.
fn anon_shape_fields(module: &Module, field_name: &str) -> Vec<(String, Type)> {
    let matches: Vec<&perry_hir::Class> = module
        .classes
        .iter()
        .filter(|c| {
            c.name.starts_with("__AnonShape_") && c.fields.iter().any(|f| f.name == field_name)
        })
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "expected exactly one anon-shape class carrying field `{field_name}`, found {}",
        matches.len()
    );
    matches[0]
        .fields
        .iter()
        .map(|f| (f.name.clone(), f.ty.clone()))
        .collect()
}

fn tys(module: &Module, field_name: &str) -> Vec<Type> {
    anon_shape_fields(module, field_name)
        .into_iter()
        .map(|(_, ty)| ty)
        .collect()
}

/// The headline case: the `churn` shape. Both fields must be `Number`, or the
/// literal is two pointer slots holding two doubles.
#[test]
fn for_loop_counter_literal_mints_number_fields() {
    let m = lower_src(
        r#"
        const keep: any[] = [];
        for (let i = 0; i < 10; i++) {
          const o = { v: i, w: i + 1 };
          keep.push(o);
        }
        console.log(keep.length);
    "#,
    );
    assert_eq!(
        tys(&m, "v"),
        vec![Type::Number, Type::Number],
        "`{{ v: i, w: i + 1 }}` in a `for (let i = 0; …)` loop must mint two \
         Number fields — Any is pointer-bearing and declares two POINTER slots \
         to the collector (#7544)"
    );
}

/// Arithmetic over the counter, not just the counter itself.
#[test]
fn for_loop_counter_arithmetic_mints_number_fields() {
    let m = lower_src(
        r#"
        let acc = 0;
        for (let i = 0; i < 10; i++) {
          const o = { a: i * 2, b: i - 1, c: i };
          acc += o.a + o.b + o.c;
        }
        console.log(acc);
    "#,
    );
    assert_eq!(tys(&m, "a"), vec![Type::Number, Type::Number, Type::Number]);
}

/// A for-head binding whose initializer is a string types the field `String`,
/// not `Number` — the head's type is inferred from its initializer, and the
/// inference is not hardwired to numbers.
#[test]
fn for_loop_string_head_mints_string_field() {
    let m = lower_src(
        r#"
        for (let s = "x"; s.length < 4; s = s + "x") {
          const o = { t: s };
          console.log(o.t);
        }
    "#,
    );
    assert_eq!(tys(&m, "t"), vec![Type::String]);
}

/// The boundary that keeps this sound. A value the lowering cannot type stays
/// `Any`, so the class stays pointer-bearing and the collector scans the slot.
#[test]
fn unprovable_values_still_mint_any_fields() {
    // A bare (unannotated) parameter: its value comes from the caller.
    let bare_param = lower_src(
        r#"
        function g(p) { return { p1: p }; }
        console.log(g(1).p1);
    "#,
    );
    assert_eq!(tys(&bare_param, "p1"), vec![Type::Any]);

    // An explicitly `any` value.
    let any_value = lower_src(
        r#"
        declare const z: any;
        const o = { p2: z };
        console.log(o.p2);
    "#,
    );
    assert_eq!(tys(&any_value, "p2"), vec![Type::Any]);

    // A `for` head over a value of unknown type stays Any too: the head's
    // binding is only as good as its initializer.
    let opaque_head = lower_src(
        r#"
        declare const start: any;
        for (let k = start; k < 10; k++) {
          const o = { p3: k };
          console.log(o.p3);
        }
    "#,
    );
    assert_eq!(tys(&opaque_head, "p3"), vec![Type::Any]);
}

/// A `var` head keeps `Type::Any`. It is function-scoped and var-hoisted, so
/// its declarator is not the only writer of the name and the statement-level
/// parity argument does not carry over.
#[test]
fn var_for_head_still_mints_any() {
    let m = lower_src(
        r#"
        for (var i = 0; i < 10; i++) {
          const o = { q: i };
          console.log(o.q);
        }
    "#,
    );
    assert_eq!(tys(&m, "q"), vec![Type::Any]);
}

/// A mixed literal keeps each field's own type — the change propagates per
/// property, and a pointer-bearing property still makes the class
/// pointer-bearing (which is what `class_layout_declarable_at_allocation`
/// reads).
#[test]
fn mixed_literal_keeps_per_field_types() {
    let m = lower_src(
        r#"
        declare const z: any;
        for (let i = 0; i < 10; i++) {
          const o = { n: i, s: "k", u: z };
          console.log(o.n, o.s, o.u);
        }
    "#,
    );
    assert_eq!(
        tys(&m, "n"),
        vec![Type::Number, Type::String, Type::Any],
        "each property carries its own inferred type; one Any field is enough \
         to keep the class pointer-bearing"
    );
}

/// An inner binding that shadows the counter is a different local, so the
/// literal must not inherit the counter's type through the name.
#[test]
fn inner_shadow_does_not_inherit_the_counter_type() {
    let m = lower_src(
        r#"
        declare const z: any;
        for (let i = 0; i < 10; i++) {
          const i2 = z;
          const o = { r: i2 };
          console.log(o.r);
        }
    "#,
    );
    assert_eq!(tys(&m, "r"), vec![Type::Any]);
}
