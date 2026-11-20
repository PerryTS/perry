//! A named class expression evaluated in a function is a fresh class object
//! per evaluation (#11298), so its own name inside its body must be that
//! evaluation for `new` and `instanceof` too, as for a per-evaluation class
//! declaration (#11142).
//!
//! prosemirror-model's Fragment, bundled into a lazy `__esm` init:
//! `F = class Fragment { static from(v) { if (v instanceof Fragment) return v;
//! ... } append(o) { return new Fragment(..) } }`. `new Fragment()` built an
//! instance of the shared template, whose prototype is not the evaluated
//! class's, and `instanceof Fragment` (the evaluation) rejected it:
//! editor threw "Can not convert <paragraph> to a Fragment".

fn lower(source: &str) -> crate::ir::Module {
    let ast = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    super::lower_module(&ast, "t", "t.ts").expect("source lowers")
}

fn compact(text: String) -> String {
    text.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn class_named<'a>(module: &'a crate::ir::Module, prefix: &str) -> &'a crate::ir::Class {
    module
        .classes
        .iter()
        .find(|c| c.name.starts_with(prefix))
        .unwrap_or_else(|| panic!("class `{prefix}*` must be lowered: {:?}", module.classes))
}

fn static_body(module: &crate::ir::Module, prefix: &str, method: &str) -> String {
    let class = class_named(module, prefix);
    let m = class
        .static_methods
        .iter()
        .find(|m| m.name == method)
        .unwrap_or_else(|| panic!("static `{method}` must be lowered"));
    compact(format!("{:?}", m.body))
}

fn method_body(module: &crate::ir::Module, prefix: &str, method: &str) -> String {
    let class = class_named(module, prefix);
    let m = class
        .methods
        .iter()
        .find(|m| m.name == method)
        .unwrap_or_else(|| panic!("method `{method}` must be lowered"));
    compact(format!("{:?}", m.body))
}

const FRAGMENT: &str = r#"
    var F: any;
    function init() {
        F = class Fragment {
            content: any;
            constructor(content: any) { this.content = content; }
            append(other: any) { return new Fragment(this.content.concat(other.content)); }
            static from(v: any) {
                if (v instanceof Fragment) return v;
                return new Fragment([v]);
            }
        };
    }
    init();
"#;

#[test]
fn new_of_own_name_in_a_fresh_class_expression_constructs_its_evaluation() {
    let module = lower(FRAGMENT);
    for body in [
        static_body(&module, "Fragment", "from"),
        method_body(&module, "Fragment", "append"),
    ] {
        assert!(
            body.contains("NewDynamic{"),
            "`new Fragment()` must construct this evaluation: {body}"
        );
        assert!(
            !body.contains("New{class_name:"),
            "`new Fragment()` must not construct the shared template: {body}"
        );
    }
}

#[test]
fn instanceof_own_name_in_a_fresh_class_expression_tests_its_evaluation() {
    let module = lower(FRAGMENT);
    let body = static_body(&module, "Fragment", "from");
    assert!(
        body.contains("ty_expr:Some("),
        "`instanceof Fragment` must test against this evaluation: {body}"
    );
}

#[test]
fn fresh_class_expression_initializes_its_self_binding() {
    let module = lower(FRAGMENT);
    let init = compact(format!(
        "{:?}",
        module
            .functions
            .iter()
            .find(|f| f.name == "init")
            .expect("`init` must be lowered")
            .body
    ));
    assert!(
        init.contains("evaluation_owner:Some("),
        "the evaluated class object must initialize the self-binding: {init}"
    );
}
