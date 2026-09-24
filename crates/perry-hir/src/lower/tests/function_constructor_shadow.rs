//! Constructor lookup must follow the live lexical binding (#11160).

#[test]
fn function_constructor_restored_after_class_expression_block() {
    for binding in ["const", "let"] {
        let source = format!(
            r#"
            function C(this: any) {{ this.k = 1; }}
            {{ {binding} C: any = class {{ k = 2 }}; void C; }}
            new (C as any)();
        "#
        );
        let parsed = perry_parser::parse_typescript(&source, "t.ts").unwrap();
        let hir = super::lower_module(&parsed, "t", "t.ts").unwrap();
        let function = hir.functions.iter().find(|f| f.name == "C").unwrap();
        let init = format!("{:?}", hir.init);
        assert!(
            init.contains(&format!("NewDynamic {{ callee: FuncRef({})", function.id)),
            "the expired class binding must not replace the function: {init}"
        );
    }
}
