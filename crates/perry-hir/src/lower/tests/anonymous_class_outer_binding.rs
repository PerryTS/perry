//! #11153: an anonymous class has no inner self-binding; its inferred name
//! does not replace the enclosing variable that members close over.
#[test]
fn anonymous_class_members_capture_the_outer_binding() {
    let ast = perry_parser::parse_typescript(
        "function make(v: number) { const C = class { static tag = v; who() { return C.tag; } self() { return C; } }; return C; }",
        "anonymous-outer.ts",
    ).unwrap();
    let hir = super::lower_module(&ast, "anonymous_outer", "anonymous-outer.ts").unwrap();
    let class = hir
        .classes
        .iter()
        .find(|class| class.methods.iter().any(|m| m.name == "who"))
        .unwrap();
    let who = class.methods.iter().find(|m| m.name == "who").unwrap();
    let identity = class.methods.iter().find(|m| m.name == "self").unwrap();
    assert!(
        !format!("{:?}", who.body).contains("StaticFieldGet"),
        "outer binding must not read template statics: {:?}",
        who.body
    );
    assert!(
        !format!("{:?}", identity.body).contains("ClassRef"),
        "outer binding must be captured, not replaced by a template: {:?}",
        identity.body
    );
}
