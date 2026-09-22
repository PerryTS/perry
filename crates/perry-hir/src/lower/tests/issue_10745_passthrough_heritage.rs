//! #10745: `PassThrough` is a classic `node:stream` native parent just like
//! `Transform`. Both class-lowering paths must retain that identity so codegen
//! can initialize the derived object in place and honor its `_transform`.

#[test]
fn passthrough_import_alias_is_a_native_parent_for_decls_and_expressions() {
    let source = r#"
        import { PassThrough as PT } from "node:stream";
        class Decl extends PT { _transform(chunk, enc, cb) { cb(null, chunk); } }
        const Expr = class extends PT { _transform(chunk, enc, cb) { cb(null, chunk); } };
    "#;
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");

    for name in ["Decl", "Expr"] {
        let class = hir
            .classes
            .iter()
            .find(|class| class.name == name)
            .unwrap_or_else(|| panic!("{name} is lowered"));
        assert_eq!(class.extends_name.as_deref(), Some("PassThrough"));
        assert_eq!(
            class.native_extends,
            Some(("node_stream".to_string(), "PassThrough".to_string()))
        );
        assert!(class.extends_expr.is_none());
    }
}
