//! #11142: a per-evaluation class declaration's own name inside its body is
//! that evaluation's class object, not the shared template `ClassRef`.
//!
//! `class RC { #s; static make() { return attachConfig(RC) } }` declared in a
//! function takes the per-evaluation `ClassExprFresh` path, so the outer `RC`
//! is a fresh heap class object. Its body used to read `ClassRef("RC")`, and a
//! subclass built from that in a static factory (redis's
//! `RedisClient.factory`) pinned the template as its parent: `instanceof RC`
//! and `#s in` were then false for the caller's `RC`.

use crate::ir::{Class, Module};

fn lower(source: &str) -> Module {
    let module = perry_parser::parse_typescript(source, "self-binding.ts").expect("source parses");
    crate::lower::lower_module(&module, "self-binding", "self-binding.ts").expect("source lowers")
}

fn class<'a>(hir: &'a Module, name: &str) -> &'a Class {
    hir.classes
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("class `{name}` must be lowered: {:?}", hir.classes))
}

fn static_method_body(hir: &Module, class_name: &str, method: &str) -> String {
    let body = &class(hir, class_name)
        .static_methods
        .iter()
        .find(|m| m.name == method)
        .unwrap_or_else(|| panic!("static `{method}` must be lowered"))
        .body;
    format!("{body:?}")
}

fn factory_body(hir: &Module) -> String {
    let body = &hir
        .functions
        .iter()
        .find(|f| f.name == "factory")
        .expect("`factory` must be lowered")
        .body;
    format!("{body:?}")
}

#[test]
fn private_class_decl_self_reference_reads_its_evaluation() {
    let hir = lower(
        r#"
            function attach(Base: any) { return class extends Base {}; }
            export function factory(tag: string) {
                class RC {
                    #s = tag;
                    static make() { return new (attach(RC))(); }
                    static create() { return new RC(); }
                    static is(o: any) { return o instanceof RC; }
                }
                return RC;
            }
        "#,
    );
    let make = static_method_body(&hir, "RC", "make");
    assert!(
        !make.contains("ClassRef(\"RC\")"),
        "the self-reference passed to a factory must not be the template: {make}"
    );
    let create = static_method_body(&hir, "RC", "create");
    assert!(
        create.contains("NewDynamic"),
        "`new RC()` in the body must construct this evaluation: {create}"
    );
    let is = static_method_body(&hir, "RC", "is");
    assert!(
        is.contains("ty_expr: Some("),
        "`instanceof RC` in the body must test against this evaluation: {is}"
    );
    let factory = factory_body(&hir);
    assert!(
        factory.contains("evaluation_owner: Some("),
        "the fresh class object must initialize the self-binding: {factory}"
    );
}

#[test]
fn function_nested_class_decl_is_per_evaluation() {
    // Every call of `factory` evaluates the declaration again and must create
    // a distinct class, so even a class with no private elements and no
    // heritage takes the per-evaluation path, and its members' own name reads
    // this evaluation's class object instead of the shared template.
    let hir = lower(
        r#"
            export function factory() {
                class Plain {
                    static make() { return new Plain(); }
                    static self() { return Plain; }
                }
                return Plain;
            }
        "#,
    );
    let self_body = static_method_body(&hir, "Plain", "self");
    assert!(
        !self_body.contains("ClassRef(\"Plain\")") && self_body.contains("ClassCaptureValue"),
        "a per-evaluation declaration reads its own evaluation: {self_body}"
    );
    let factory = factory_body(&hir);
    assert!(
        factory.contains("ClassExprFresh"),
        "a declaration in a function body is evaluated per call: {factory}"
    );
}
