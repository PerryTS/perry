use super::*;

#[test]
fn tracks_reads_but_keeps_identifiers_and_function_bodies_static() {
    for (expression, expected) in [
        ("value", false),
        ("1 + 2", false),
        ("() => value()", false),
        ("function() { return props.value }", false),
        ("value()", true),
        ("props.value", true),
        ("props?.value", true),
        ("value?.()", true),
        ("{ width: count() }", true),
        ("'width' in props", true),
        ("{ ...props }", true),
    ] {
        let module =
            perry_parser::parse_typescript(&format!("const x = {expression};"), "test.tsx")
                .unwrap();
        let ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Var(var))) = &module.body[0] else {
            panic!()
        };
        assert_eq!(
            is_dynamic(var.decls[0].init.as_ref().unwrap()),
            expected,
            "{expression}"
        );
    }
}

#[test]
fn universal_helpers_use_selected_module_and_do_not_collide_with_user_names() {
    let source = r#"
        const __perry_solid_0_createElement = 1;
        let ref;
        const count = () => 1;
        const focus = () => {};
        const x = <box width={count()} use:focus={count()} ref={ref}>Hello {count()}<text /></box>;
    "#;
    let module = perry_parser::parse_typescript(source, "test.tsx").unwrap();
    let expanded = lower_solid_jsx(&module, "@opentui/solid").unwrap();
    let ast::ModuleItem::ModuleDecl(ast::ModuleDecl::Import(import)) = &expanded.body[0] else {
        panic!()
    };
    assert_eq!(import.src.value, "@opentui/solid");
    assert!(import
        .specifiers
        .iter()
        .all(|s| s.local().sym.starts_with("__perry_solid_1_")));
    let hir = crate::lower_module(&expanded, "test", "test.tsx").unwrap();
    let hir = format!("{hir:?}");
    for helper in [
        "createElement",
        "createTextNode",
        "insertNode",
        "insert",
        "setProp",
        "effect",
        "use",
    ] {
        assert!(hir.contains(helper), "missing {helper}");
    }
    assert!(!hir.contains("JsxElement"));
    assert!(!hir.contains("jsx-runtime"));
}

#[test]
fn modules_without_jsx_do_not_gain_a_renderer_import() {
    let module = perry_parser::parse_typescript("export const x = 1", "test.ts").unwrap();
    assert!(lower_solid_jsx(&module, "@opentui/solid").is_none());
}

#[test]
fn callback_refs_do_not_assign_constants_but_mutable_shadowed_refs_still_assign() {
    let module = perry_parser::parse_typescript(
        r#"
        import { importedRef } from "./refs";
        const ref = node => {};
        const a = <text ref={ref} />;
        const b = <text ref={importedRef} />;
        function nested() { let ref; const c = <text ref={ref!} />; return ref; }
    "#,
        "refs.tsx",
    )
    .unwrap();
    let expanded = lower_solid_jsx(&module, "@opentui/solid").unwrap();
    struct Assignments(Vec<String>);
    impl Visit for Assignments {
        fn visit_assign_expr(&mut self, expr: &ast::AssignExpr) {
            if let ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(id)) = &expr.left {
                self.0.push(id.id.sym.to_string());
            }
            expr.visit_children_with(self);
        }
    }
    let mut assignments = Assignments(Vec::new());
    expanded.visit_with(&mut assignments);
    assert_eq!(
        assignments.0,
        vec!["ref"],
        "only the nested mutable ref is assigned"
    );
}
