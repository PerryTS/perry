//! Tests for Phase 1 (object-literal shape inference) and Phase 4
//! (body-based return-type inference). Parses real TypeScript source,
//! lowers to HIR, and inspects the inferred types on locals/functions.
//!
//! These tests drive the step-1+4 work of the Static-Hermes-parity plan —
//! see the design notes in the team response. The invariant we're establishing:
//! typed object literals, typed classes (including inheritance chains), and
//! unannotated user functions all carry enough compile-time type info for
//! downstream codegen to route property access through the direct-GEP path.

use perry_diagnostics::SourceCache;
use perry_hir::{lower_module, Module, Stmt};
use perry_parser::parse_typescript_with_cache;
use perry_types::Type;

/// Run parsing + lowering on a thread with a larger stack than cargo's default
/// 2 MB. The perry HIR lowering passes are deeply recursive — typical compiler
/// workloads just fine on production runs (perry spawns its own compile threads)
/// but the default test harness thread is too small and SIGABRTs on moderately
/// nested programs.
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

/// Find the declared type of a top-level `let`/`const` binding by name.
/// Module-level bindings live in `module.init` as `Stmt::Let`. The Type is
/// carried on the statement itself.
fn find_local_type<'m>(module: &'m Module, name: &str) -> &'m Type {
    fn walk<'m>(stmts: &'m [Stmt], target: &str) -> Option<&'m Type> {
        for s in stmts {
            if let Stmt::Let { name: n, ty, .. } = s {
                if n == target {
                    return Some(ty);
                }
            }
            // Descend through control-flow wrappers in case the binding is nested.
            match s {
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    if let Some(t) = walk(then_branch, target) {
                        return Some(t);
                    }
                    if let Some(eb) = else_branch {
                        if let Some(t) = walk(eb, target) {
                            return Some(t);
                        }
                    }
                }
                Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
                    if let Some(t) = walk(body, target) {
                        return Some(t);
                    }
                }
                _ => {}
            }
        }
        None
    }
    if let Some(ty) = walk(&module.init, name) {
        return ty;
    }
    // Fall back to globals.
    for g in &module.globals {
        if g.name == name {
            return &g.ty;
        }
    }
    panic!(
        "local `{}` not found in module (init statements: {})",
        name,
        module.init.len()
    );
}

fn find_fn<'m>(module: &'m Module, name: &str) -> &'m perry_hir::Function {
    module
        .functions
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("function `{}` not found", name))
}

fn assert_obj_shape(ty: &Type, expected: &[(&str, Type)]) {
    let obj = match ty {
        Type::Object(o) => o,
        other => panic!("expected Type::Object, got {:?}", other),
    };
    assert_eq!(
        obj.properties.len(),
        expected.len(),
        "property count mismatch: got {:?}, expected {:?}",
        obj.properties.keys().collect::<Vec<_>>(),
        expected.iter().map(|(k, _)| *k).collect::<Vec<_>>(),
    );
    for (key, expected_ty) in expected {
        let got = obj
            .properties
            .get(*key)
            .unwrap_or_else(|| panic!("missing property `{}`", key));
        assert_eq!(&got.ty, expected_ty, "property `{}` type mismatch", key);
    }
}

// ---------- Phase 1: object literal shape inference ----------

#[test]
fn literal_with_number_fields_infers_shape() {
    let module = lower_src("const p = { x: 1, y: 2 };");
    assert_obj_shape(
        find_local_type(&module, "p"),
        &[("x", Type::Number), ("y", Type::Number)],
    );
}

#[test]
fn literal_with_mixed_primitive_fields_infers_shape() {
    let module = lower_src(r#"const u = { id: 42, name: "a", active: true };"#);
    assert_obj_shape(
        find_local_type(&module, "u"),
        &[
            ("id", Type::Number),
            ("name", Type::String),
            ("active", Type::Boolean),
        ],
    );
}

#[test]
fn shorthand_properties_pick_up_local_types() {
    let module = lower_src(
        r#"
        const x = 1;
        const name = "hello";
        const p = { x, name };
        "#,
    );
    assert_obj_shape(
        find_local_type(&module, "p"),
        &[("x", Type::Number), ("name", Type::String)],
    );
}

#[test]
fn nested_object_literal_infers_nested_shape() {
    let module = lower_src("const p = { origin: { x: 0, y: 0 }, scale: 1 };");
    let outer = match find_local_type(&module, "p") {
        Type::Object(o) => o,
        other => panic!("expected Type::Object, got {:?}", other),
    };
    assert!(outer.properties.contains_key("origin"));
    assert!(outer.properties.contains_key("scale"));
    assert_eq!(outer.properties["scale"].ty, Type::Number);
    assert_obj_shape(
        &outer.properties["origin"].ty,
        &[("x", Type::Number), ("y", Type::Number)],
    );
}

#[test]
fn spread_makes_shape_open() {
    let module = lower_src(
        r#"
        const base = { x: 1 };
        const q = { ...base, y: 2 };
        "#,
    );
    assert_eq!(*find_local_type(&module, "q"), Type::Any);
}

#[test]
fn computed_key_makes_shape_open() {
    let module = lower_src(
        r#"
        const k = "foo";
        const q = { [k]: 1 };
        "#,
    );
    assert_eq!(*find_local_type(&module, "q"), Type::Any);
}

#[test]
fn methods_make_shape_open() {
    let module = lower_src(
        r#"
        const q = {
            greet() { return "hi"; }
        };
        "#,
    );
    assert_eq!(*find_local_type(&module, "q"), Type::Any);
}

#[test]
fn getter_makes_shape_open() {
    let module = lower_src(
        r#"
        const q = {
            get x() { return 1; }
        };
        "#,
    );
    assert_eq!(*find_local_type(&module, "q"), Type::Any);
}

#[test]
fn empty_literal_is_closed_empty_shape() {
    let module = lower_src("const q = {};");
    assert_obj_shape(find_local_type(&module, "q"), &[]);
}

// ---------- Phase 4: return-type inference from body ----------

#[test]
fn unannotated_function_returning_number_literal_infers_number() {
    let module = lower_src("function id() { return 42; }");
    assert_eq!(find_fn(&module, "id").return_type, Type::Number);
}

#[test]
fn unannotated_function_returning_string_literal_infers_string() {
    let module = lower_src(r#"function greet() { return "hello"; }"#);
    assert_eq!(find_fn(&module, "greet").return_type, Type::String);
}

#[test]
fn unannotated_function_returning_object_infers_shape() {
    let module = lower_src(
        r#"
        function makePoint() {
            return { x: 0, y: 0 };
        }
        "#,
    );
    assert_obj_shape(
        &find_fn(&module, "makePoint").return_type,
        &[("x", Type::Number), ("y", Type::Number)],
    );
}

#[test]
fn function_with_multiple_consistent_returns_infers_type() {
    let module = lower_src(
        r#"
        function sign(n: number) {
            if (n < 0) { return -1; }
            if (n > 0) { return 1; }
            return 0;
        }
        "#,
    );
    assert_eq!(find_fn(&module, "sign").return_type, Type::Number);
}

#[test]
fn function_with_mixed_returns_bails_to_any() {
    let module = lower_src(
        r#"
        function mixed(b: boolean) {
            if (b) { return 1; }
            return "zero";
        }
        "#,
    );
    assert_eq!(find_fn(&module, "mixed").return_type, Type::Any);
}

#[test]
fn function_with_no_returns_infers_void() {
    let module = lower_src(
        r#"
        function noop() {
            const x = 1;
        }
        "#,
    );
    assert_eq!(find_fn(&module, "noop").return_type, Type::Void);
}

#[test]
fn async_unannotated_function_wraps_in_promise() {
    let module = lower_src(
        r#"
        async function getNum() {
            return 7;
        }
        "#,
    );
    match &find_fn(&module, "getNum").return_type {
        Type::Promise(inner) => assert_eq!(**inner, Type::Number),
        other => panic!("expected Promise, got {:?}", other),
    }
}

#[test]
fn annotated_return_type_takes_precedence_over_inference() {
    // Annotation wins even if body would infer something different.
    let module = lower_src(
        r#"
        function f(): any {
            return 42;
        }
        "#,
    );
    assert_eq!(find_fn(&module, "f").return_type, Type::Any);
}

#[test]
fn generator_return_type_is_not_inferred() {
    // Generators have complex return semantics (Generator<T>); we skip them.
    let module = lower_src(
        r#"
        function* gen() {
            yield 1;
            yield 2;
        }
        "#,
    );
    assert_eq!(find_fn(&module, "gen").return_type, Type::Any);
}

#[test]
fn nested_functions_dont_leak_returns_to_outer() {
    let module = lower_src(
        r#"
        function outer() {
            function inner() { return "nested"; }
            return 42;
        }
        "#,
    );
    // outer returns a number, not a string — the inner return statement is
    // scoped to inner and collect_return_types skips nested function decls.
    // (Nested fn decls don't appear in module.functions — they're hoisted into
    // the enclosing scope — so we only assert on the outer's inferred type.)
    assert_eq!(find_fn(&module, "outer").return_type, Type::Number);
}

#[test]
fn return_type_inferred_across_call_site() {
    // With Phase 4 the unannotated `makePoint` gets `Type::Object({x,y})` as its
    // return type, and the call-site inference in infer_call_return_type pulls it
    // back out, so `p` should carry the inferred shape.
    let module = lower_src(
        r#"
        function makePoint() { return { x: 0, y: 0 }; }
        const p = makePoint();
        "#,
    );
    assert_obj_shape(
        find_local_type(&module, "p"),
        &[("x", Type::Number), ("y", Type::Number)],
    );
}

// ---------- Classes and inheritance ----------
//
// Classes already populated fields into `Class.fields` before this work; what
// we're establishing here is that the fields are still correctly represented
// as the inheritance chain deepens. The direct-GEP codegen path (expr.rs:2399-2434)
// consumes these via class_field_global_index(), so any silent drop-off here
// would regress typed property access for inherited fields.

fn find_class<'m>(module: &'m Module, name: &str) -> &'m perry_hir::Class {
    module
        .classes
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("class `{}` not found", name))
}

#[test]
fn class_with_declared_fields_populates_field_list() {
    let module = lower_src(
        r#"
        class Point {
            x: number;
            y: number;
            constructor(x: number, y: number) { this.x = x; this.y = y; }
        }
        "#,
    );
    let c = find_class(&module, "Point");
    let field_names: Vec<_> = c.fields.iter().map(|f| f.name.as_str()).collect();
    assert!(field_names.contains(&"x"));
    assert!(field_names.contains(&"y"));
}

#[test]
fn class_single_inheritance_preserves_parent_link() {
    let module = lower_src(
        r#"
        class Animal {
            species: string;
            constructor(s: string) { this.species = s; }
        }
        class Dog extends Animal {
            breed: string;
            constructor(s: string, b: string) { super(s); this.breed = b; }
        }
        "#,
    );
    let dog = find_class(&module, "Dog");
    let dog_fields: Vec<_> = dog.fields.iter().map(|f| f.name.as_str()).collect();
    assert!(dog_fields.contains(&"breed"));
    // The extends link is what class_field_global_index walks at codegen time;
    // without it the inherited `species` slot would collide with a local one.
    assert!(
        dog.extends.is_some(),
        "Dog should record its parent class in the extends link"
    );
}

#[test]
fn class_multi_level_inheritance_each_level_has_its_fields() {
    let module = lower_src(
        r#"
        class A {
            a: number;
            constructor(a: number) { this.a = a; }
        }
        class B extends A {
            b: number;
            constructor(a: number, b: number) { super(a); this.b = b; }
        }
        class C extends B {
            c: number;
            constructor(a: number, b: number, c: number) { super(a, b); this.c = c; }
        }
        "#,
    );
    let a = find_class(&module, "A");
    let b = find_class(&module, "B");
    let c = find_class(&module, "C");
    assert!(a.fields.iter().any(|f| f.name == "a"));
    assert!(b.fields.iter().any(|f| f.name == "b"));
    assert!(c.fields.iter().any(|f| f.name == "c"));
    assert!(b.extends.is_some());
    assert!(c.extends.is_some());
}

#[test]
fn class_method_without_annotation_infers_return_from_body() {
    // v0.5.168+: class method return types are inferred from the body when
    // no annotation is present (Phase 4 expansion). `this.n` currently
    // infers as Type::Any (no per-field receiver type flow yet), so a
    // bare `return this.n` reports Any. But a method returning a literal
    // or an already-typed local does propagate through.
    let module = lower_src(
        r#"
        class Counter {
            n: number;
            constructor() { this.n = 0; }
            zero() { return 0; }
            label() { return "counter"; }
            flag() { return true; }
        }
        "#,
    );
    let c = find_class(&module, "Counter");
    let find_method = |name: &str| {
        c.methods
            .iter()
            .find(|m| m.name == name)
            .unwrap_or_else(|| panic!("method `{}` should exist", name))
    };
    assert_eq!(find_method("zero").return_type, Type::Number);
    assert_eq!(find_method("label").return_type, Type::String);
    assert_eq!(find_method("flag").return_type, Type::Boolean);
}

#[test]
fn class_method_annotation_wins_over_inference() {
    let module = lower_src(
        r#"
        class C {
            n: number;
            constructor() { this.n = 0; }
            get(): any { return 42; }
        }
        "#,
    );
    let c = find_class(&module, "C");
    let get = c.methods.iter().find(|m| m.name == "get").unwrap();
    // Explicit `: any` annotation is respected over inference.
    assert_eq!(get.return_type, Type::Any);
}

#[test]
fn async_class_method_wraps_inferred_return_in_promise() {
    let module = lower_src(
        r#"
        class C {
            n: number;
            constructor() { this.n = 0; }
            async fetch() { return 7; }
        }
        "#,
    );
    let c = find_class(&module, "C");
    let fetch = c.methods.iter().find(|m| m.name == "fetch").unwrap();
    match &fetch.return_type {
        Type::Promise(inner) => assert_eq!(**inner, Type::Number),
        other => panic!("expected Promise<Number>, got {:?}", other),
    }
}

#[test]
fn generator_class_method_return_type_not_inferred() {
    let module = lower_src(
        r#"
        class C {
            n: number;
            constructor() { this.n = 0; }
            *seq() { yield 1; yield 2; }
        }
        "#,
    );
    let c = find_class(&module, "C");
    let seq = c.methods.iter().find(|m| m.name == "seq").unwrap();
    // Generators skip inference (Generator<T> shape is out of scope).
    assert_eq!(seq.return_type, Type::Any);
}

#[test]
fn getter_infers_return_from_body() {
    let module = lower_src(
        r#"
        class C {
            n: number;
            constructor() { this.n = 42; }
            get zero() { return 0; }
            get label() { return "x"; }
        }
        "#,
    );
    let c = find_class(&module, "C");
    let zero = c.getters.iter().find(|(n, _)| n == "zero").unwrap();
    let label = c.getters.iter().find(|(n, _)| n == "label").unwrap();
    assert_eq!(zero.1.return_type, Type::Number);
    assert_eq!(label.1.return_type, Type::String);
}

#[test]
fn arrow_expression_body_infers_return() {
    // Arrows with no param reference in the body: Phase 4 expansion pulls
    // the body type directly via infer_type_from_expr. Arrows whose body
    // references params require the params to be in ctx at inference time
    // — a separate extension that'd need infer_type_from_expr to accept
    // a scope override (tracked as follow-up).
    let module = lower_src(
        r#"
        const g = () => "hello";
        const h = () => true;
        const k = () => 42;
        "#,
    );
    match find_local_type(&module, "g") {
        Type::Function(ft) => assert_eq!(*ft.return_type, Type::String),
        other => panic!("expected Function, got {:?}", other),
    }
    match find_local_type(&module, "h") {
        Type::Function(ft) => assert_eq!(*ft.return_type, Type::Boolean),
        other => panic!("expected Function, got {:?}", other),
    }
    match find_local_type(&module, "k") {
        Type::Function(ft) => assert_eq!(*ft.return_type, Type::Number),
        other => panic!("expected Function, got {:?}", other),
    }
}

#[test]
fn arrow_expression_body_using_param_falls_back_to_any() {
    // Documents the current limitation: the body references `x`, but
    // infer_type_from_expr runs with ctx that hasn't seen the arrow's
    // params yet (they'd be defined during actual lowering, not
    // inference). Flips to the inferred type when we extend inference
    // to accept a scope override.
    let module = lower_src(
        r#"
        const f = (x: number) => x + 1;
        "#,
    );
    match find_local_type(&module, "f") {
        Type::Function(ft) => assert_eq!(*ft.return_type, Type::Any),
        other => panic!("expected Function, got {:?}", other),
    }
}

#[test]
fn arrow_block_body_infers_return() {
    let module = lower_src(
        r#"
        const f = (n: number) => {
            if (n < 0) return -1;
            return 1;
        };
        "#,
    );
    match find_local_type(&module, "f") {
        Type::Function(ft) => assert_eq!(*ft.return_type, Type::Number),
        other => panic!("expected Function, got {:?}", other),
    }
}

#[test]
fn async_arrow_wraps_inferred_return_in_promise() {
    let module = lower_src(
        r#"
        const f = async () => 42;
        "#,
    );
    match find_local_type(&module, "f") {
        Type::Function(ft) => match ft.return_type.as_ref() {
            Type::Promise(inner) => assert_eq!(**inner, Type::Number),
            other => panic!("expected Promise, got {:?}", other),
        },
        other => panic!("expected Function, got {:?}", other),
    }
}

#[test]
fn method_call_return_type_flows_to_binding() {
    // Phase 4.1: `new C().method()` where method has an inferred or
    // annotated return type → the binding picks it up.
    let module = lower_src(
        r#"
        class C {
            n: number;
            constructor() { this.n = 0; }
            label() { return "hi"; }
            zero() { return 0; }
        }
        const s = new C().label();
        const z = new C().zero();
        "#,
    );
    assert_eq!(*find_local_type(&module, "s"), Type::String);
    assert_eq!(*find_local_type(&module, "z"), Type::Number);
}

#[test]
fn method_call_on_typed_local_receiver() {
    // Receiver is a local with Type::Named — method-call inference
    // should resolve through the class registry.
    let module = lower_src(
        r#"
        class C {
            n: number;
            constructor() { this.n = 0; }
            label(): string { return "x"; }
        }
        const c = new C();
        const s = c.label();
        "#,
    );
    assert_eq!(*find_local_type(&module, "s"), Type::String);
}

#[test]
fn method_call_annotated_return_type_flows() {
    let module = lower_src(
        r#"
        class C {
            n: number;
            constructor() { this.n = 0; }
            parse(raw: string): number { return 42; }
        }
        const x = new C().parse("x");
        "#,
    );
    assert_eq!(*find_local_type(&module, "x"), Type::Number);
}

#[test]
fn arrow_annotation_wins_over_inference() {
    let module = lower_src(
        r#"
        const f = (): any => 42;
        "#,
    );
    match find_local_type(&module, "f") {
        Type::Function(ft) => assert_eq!(*ft.return_type, Type::Any),
        other => panic!("expected Function, got {:?}", other),
    }
}

#[test]
fn class_with_interface_implements_preserves_structural_shape() {
    // Interface shapes are stored separately from the class; what we want here
    // is the class itself — still the authoritative layout source — to carry
    // all the implemented fields.
    let module = lower_src(
        r#"
        interface HasLabel { label: string; }
        interface HasSize { size: number; }
        class Button implements HasLabel, HasSize {
            label: string;
            size: number;
            constructor(l: string, s: number) { this.label = l; this.size = s; }
        }
        "#,
    );
    let b = find_class(&module, "Button");
    let names: Vec<_> = b.fields.iter().map(|f| f.name.as_str()).collect();
    assert!(names.contains(&"label"));
    assert!(names.contains(&"size"));
}

#[test]
fn literal_matching_interface_shape_infers_same_field_set() {
    // The literal doesn't reference the interface, but Phase 1's inference
    // gives it the same structural shape an explicit `: Point` annotation
    // would — which is what makes typed-TS-through-the-typed-path work.
    let module = lower_src(
        r#"
        interface Point { x: number; y: number; }
        const p = { x: 1, y: 2 };
        "#,
    );
    assert_obj_shape(
        find_local_type(&module, "p"),
        &[("x", Type::Number), ("y", Type::Number)],
    );
}

// ---------- Sanity: used-together integration ----------

#[test]
fn factory_returning_nested_literal_survives_roundtrip() {
    // Phase 4 infers makeCfg's return type from its body; Phase 1's inference
    // of the nested literal survives the round-trip to the call site.
    //
    // The combination of `class X extends Y` with a top-level factory-call
    // binding currently hits a pre-existing compiler overflow in this tree
    // (see tests/repro_test.rs), unrelated to this work. This test exercises
    // the factory + nested-literal round-trip without the extends-chain trigger.
    let module = lower_src(
        r#"
        function makeCfg() { return { scale: 1, origin: { x: 0, y: 0 } }; }
        const cfg = makeCfg();
        "#,
    );
    let outer = match find_local_type(&module, "cfg") {
        Type::Object(o) => o,
        other => panic!("expected Type::Object, got {:?}", other),
    };
    assert!(outer.properties.contains_key("scale"));
    assert!(outer.properties.contains_key("origin"));
    assert_obj_shape(
        &outer.properties["origin"].ty,
        &[("x", Type::Number), ("y", Type::Number)],
    );
}

#[test]
fn standalone_inheritance_preserves_chain() {
    // Separate test covers the inheritance chain without the overflow trigger.
    let module = lower_src(
        r#"
        class Base { b: number; constructor(b: number) { this.b = b; } }
        class Derived extends Base {
            d: number;
            constructor(b: number, d: number) { super(b); this.d = d; }
        }
        "#,
    );
    let derived = find_class(&module, "Derived");
    assert!(derived.extends.is_some());
    assert!(derived.fields.iter().any(|f| f.name == "d"));
}
