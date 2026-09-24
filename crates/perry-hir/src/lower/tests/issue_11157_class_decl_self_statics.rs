//! #11157: a function-body class DECLARATION lowered per evaluation
//! (`ClassExprFresh`) keeps its statics on each evaluated class object. Its
//! own members must reach them through that evaluation, never through the
//! template-keyed `StaticFieldGet`/`StaticFieldSet` or `ClassRef`.

fn lower(source: &str) -> crate::ir::Module {
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    super::lower_module(&module, "t", "t.ts").expect("source lowers")
}

fn static_method_debug(hir: &crate::ir::Module, class: &str, method: &str) -> String {
    let class = hir
        .classes
        .iter()
        .find(|c| c.name == class)
        .unwrap_or_else(|| panic!("{class} is lowered"));
    let method = class
        .static_methods
        .iter()
        .find(|m| m.name == method)
        .unwrap_or_else(|| panic!("{method} is lowered"));
    format!("{:?}", method.body)
}

#[test]
fn per_evaluation_class_decl_members_use_the_evaluation() {
    let hir = lower(
        r#"
        function mk() {
            const V = 7;
            class Base { get v() { return V; } }
            class C extends Base {
                static n = 0;
                static reset = () => { this.n = 40; };
                static bump() { return (C.n = (C.n + 1) % 0x1000000); }
            }
            return C;
        }
        "#,
    );
    let bump = static_method_debug(&hir, "C", "bump");
    assert!(!bump.contains("StaticFieldSet"), "template write: {bump}");
    assert!(!bump.contains("StaticFieldGet"), "template read: {bump}");
    let class = hir.classes.iter().find(|c| c.name == "C").unwrap();
    let reset = class
        .static_fields
        .iter()
        .find(|f| f.name == "reset")
        .and_then(|f| f.init.as_ref())
        .expect("reset has an initializer");
    let reset = format!("{reset:?}");
    assert!(
        !reset.contains("ClassRef(\"C\")"),
        "arrow this is the template: {reset}"
    );
}

#[test]
fn module_top_class_keeps_the_template_static_path() {
    let hir = lower(
        r#"
        class Base {}
        class C extends Base {
            static n = 0;
            static bump() { return (C.n = C.n + 1); }
        }
        "#,
    );
    let bump = static_method_debug(&hir, "C", "bump");
    assert!(
        bump.contains("StaticFieldSet"),
        "control lost its fast path: {bump}"
    );
}
