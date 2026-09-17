//! #10430 / #10431: the `node:stream` module value is the legacy `Stream`
//! constructor. The default binding's VALUE must read the `default` export
//! (the constructor) rather than evaluate to the namespace object, while a
//! namespace import stays the namespace; `new` of either constructor binding
//! must construct the export value instead of the by-name placeholder.

use crate::ir::{Expr, Stmt};

fn lower(source: &str) -> crate::Module {
    let ast = perry_parser::parse_typescript(source, "main.ts").unwrap();
    let hir = crate::lower::lower_module(&ast, "main", "main.ts").unwrap();
    crate::ir::clear_current_module_source();
    hir
}

fn let_init<'a>(hir: &'a crate::Module, binding: &str) -> &'a Expr {
    hir.init
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Let {
                name,
                init: Some(init),
                ..
            } if name == binding => Some(init),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no `let {binding}` with an initializer"))
}

fn is_stream_export_read(expr: &Expr, export: &str) -> bool {
    matches!(
        expr,
        Expr::PropertyGet { object, property, .. }
            if property == export
                && matches!(object.as_ref(), Expr::NativeModuleRef(module) if module == "stream")
    )
}

#[test]
fn default_import_value_is_the_stream_constructor() {
    let hir = lower(
        r#"
        import Stream from "node:stream";
        import Bare from "stream";
        import * as ns from "node:stream";
        const fromNodeSpecifier: any = Stream;
        const fromBareSpecifier: any = Bare;
        const namespace: any = ns;
        const check = ({} as any) instanceof Stream;
    "#,
    );
    assert!(is_stream_export_read(
        let_init(&hir, "fromNodeSpecifier"),
        "default"
    ));
    assert!(is_stream_export_read(
        let_init(&hir, "fromBareSpecifier"),
        "default"
    ));
    assert!(
        matches!(let_init(&hir, "namespace"), Expr::NativeModuleRef(module) if module == "stream"),
        "a namespace import must stay the namespace object"
    );
    let Expr::InstanceOf {
        ty_expr: Some(rhs), ..
    } = let_init(&hir, "check")
    else {
        panic!("expected a dynamic instanceof");
    };
    assert!(
        is_stream_export_read(rhs, "default"),
        "`x instanceof Stream` needs the callable constructor on the right-hand side"
    );
}

#[test]
fn new_of_a_stream_constructor_binding_constructs_the_export_value() {
    let hir = lower(
        r#"
        import Stream from "node:stream";
        import { Stream as Aliased } from "node:stream";
        const fromDefault = new Stream();
        const fromAlias = new Aliased();
    "#,
    );
    for binding in ["fromDefault", "fromAlias"] {
        let Expr::NewDynamic { callee, .. } = let_init(&hir, binding) else {
            panic!("`{binding}` must construct through NewDynamic");
        };
        assert!(is_stream_export_read(callee, "Stream"), "{binding}");
    }
}
