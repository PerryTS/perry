//! #11298: every evaluation of a class expression creates a distinct
//! constructor and prototype, even when the class captures nothing. A
//! capture-free, heritage-free class expression inside a function used to
//! lower to the one shared template `ClassRef`, so `makeClass() ===
//! makeClass()` held and a prototype write on one evaluation leaked into
//! instances of another.

fn lower(source: &str) -> crate::ir::Module {
    let ast = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    super::lower_module(&ast, "t", "t.ts").expect("source lowers")
}

fn body_of(module: &crate::ir::Module, name: &str) -> String {
    let function = module
        .functions
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("fixture declares function {name}"));
    format!("{:?}", function.body)
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect()
}

#[test]
fn capture_free_class_expr_in_function_is_a_fresh_evaluation() {
    let module = lower("function makeClass() { return class { constructor() {} }; }");
    let body = body_of(&module, "makeClass");
    assert!(
        body.contains("ClassExprFresh{"),
        "each call must evaluate a distinct class object: {body}"
    );
    assert!(
        !body.contains("ClassRef("),
        "the shared template must not be returned as the class value: {body}"
    );
}

#[test]
fn capture_free_local_class_constructs_through_its_evaluation() {
    // The inferred `C` alias would make `new C()` a static `New` of the
    // template, whose instances link to the template's shared prototype
    // rather than this evaluation's `C.prototype`.
    let module = lower(
        "function local() { const C = class { m() { return 1; } }; const c = new C(); return c; }",
    );
    let body = body_of(&module, "local");
    assert!(
        body.contains("ClassExprFresh{"),
        "the local class must be a fresh evaluation: {body}"
    );
    assert!(
        !body.contains("New{class_name:"),
        "`new C()` must construct from the evaluated class value: {body}"
    );
}

#[test]
fn module_top_class_expr_keeps_the_shared_template() {
    // A module-top class expression evaluates exactly once.
    let module = lower("const K = class { m() { return 1; } }; new K();");
    let init = format!("{:?}", module.init);
    assert!(
        !init.contains("ClassExprFresh"),
        "a module-top class expression needs no per-evaluation object: {init}"
    );
}
