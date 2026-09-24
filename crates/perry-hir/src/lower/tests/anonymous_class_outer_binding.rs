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

#[test]
fn reassigned_anonymous_class_does_not_reuse_the_previous_class_key() {
    let ast = perry_parser::parse_typescript(
        "function make() { let C: any = class { static tag = 1; }; C = class { static tag = 2; who() { return C.tag; } }; return C; }",
        "anonymous-reassigned.ts",
    ).unwrap();
    let hir = super::lower_module(&ast, "anonymous_reassigned", "anonymous-reassigned.ts").unwrap();
    let method = hir
        .classes
        .iter()
        .flat_map(|class| &class.methods)
        .find(|method| method.name == "who")
        .unwrap();
    assert!(
        !format!("{:?}", method.body).contains("StaticFieldGet"),
        "the reassigned local must not retain its previous class key: {:?}",
        method.body
    );
}
