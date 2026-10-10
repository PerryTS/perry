//! #12309: export visibility must not change a class-expression binding's identity.

#[test]
fn exported_class_declarator_matches_forward_constructor_identity() {
    for exported in [false, true] {
        for inner in ["", "Inner"] {
            let source = format!(
                r#"
                class Base {{ annotation; constructor(a) {{ this.annotation = a; }} }}
                function make() {{ return new Union(["a", "b"], "ann"); }}
                {} const Union = (class {} extends Base {{
                    tag = "Union";
                    types;
                    static label = "union";
                    constructor(types, annotation) {{ super(annotation); this.types = types; }}
                    getParser() {{ return this.types; }}
                }});
                "#,
                if exported { "export" } else { "" },
                inner,
            );
            let ast = perry_parser::parse_typescript(&source, "t.ts").unwrap();
            let hir = super::lower_module(&ast, "t", "t.ts").unwrap();
            let class = hir
                .classes
                .iter()
                .find(|class| class.name == "Union")
                .expect("the pre-scanned binding and its class must have the same identity");
            assert_eq!(class.extends_name.as_deref(), Some("Base"));
            assert!(class.fields.iter().any(|field| field.name == "types"));
            assert!(class.constructor.is_some());
            assert!(class
                .methods
                .iter()
                .any(|method| method.name == "getParser"));
            assert!(hir.init.iter().any(|stmt| matches!(stmt,
                crate::Stmt::Let { name, init: Some(crate::Expr::ClassRef(key)), .. }
                    if name == "Union" && key == &class.name
            )));
            let make = hir
                .functions
                .iter()
                .find(|function| function.name == "make")
                .unwrap();
            assert!(format!("{:?}", make.body).contains("class_name: \"Union\""));
        }
    }
}
