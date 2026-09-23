//! #11139: `class X extends ns.URL {}` extends a property of `ns`, not the
//! global `URL`. Codegen keys its built-in `super()` routes on the bare
//! `extends_name`, so a member heritage whose trailing name is a JS built-in
//! must not carry that name unless the object really is the global object or
//! a native module. Otherwise mongodb-connection-string-url's
//! `class URLWithoutHost extends whatwg_url_1.URL {}` constructs Perry's
//! native URL instead of running whatwg-url's constructor.

fn lower(source: &str) -> crate::ir::Module {
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    super::lower_module(&module, "t", "t.ts").expect("source lowers")
}

fn class<'a>(hir: &'a crate::ir::Module, name: &str) -> &'a crate::ir::Class {
    hir.classes
        .iter()
        .find(|class| class.name == name)
        .unwrap_or_else(|| panic!("{name} is lowered"))
}

#[test]
fn package_member_named_like_a_builtin_is_a_dynamic_parent() {
    let hir = lower(
        r#"
        import * as whatwg_url_1 from "whatwg-url";
        const lib = { Map: class {}, Label: class {} };
        class Decl extends whatwg_url_1.URL {}
        const Expr = class extends whatwg_url_1.URL {};
        class Paren extends (lib).Map {}
        class LabelSub extends lib.Label {}
        "#,
    );
    for name in ["Decl", "Expr", "Paren"] {
        let class = class(&hir, name);
        assert_eq!(
            class.extends_name, None,
            "{name}: a member heritage must not name the global built-in"
        );
        assert_eq!(class.extends, None, "{name}: no guessed static parent link");
        assert!(
            class.extends_expr.is_some(),
            "{name}: the parent resolves through the heritage value"
        );
    }
    // A trailing name codegen does not route as a built-in is unchanged.
    let label = class(&hir, "LabelSub");
    assert_eq!(label.extends_name.as_deref(), Some("Label"));
    assert!(label.extends_expr.is_some());
}

#[test]
fn global_object_and_native_module_members_keep_the_builtin_name() {
    let hir = lower(
        r#"
        import * as url from "url";
        class ViaGlobal extends globalThis.URL {}
        const ViaGlobalExpr = class extends globalThis.Date {};
        class ViaModule extends url.URL {}
        "#,
    );
    for (name, parent) in [
        ("ViaGlobal", "URL"),
        ("ViaGlobalExpr", "Date"),
        ("ViaModule", "URL"),
    ] {
        assert_eq!(
            class(&hir, name).extends_name.as_deref(),
            Some(parent),
            "{name}: the member really is the built-in"
        );
    }
}
