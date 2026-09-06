//! #6642: `perry/ui` widget lowering must preserve the Widget compatibility
//! of factory results and explicitly Widget-typed parameters.
//!
//! Split out of `lower/tests.rs` to keep it under the 2000-line size gate.

/// #6642: native lowering must preserve the Widget compatibility methods on
/// factory results and explicitly Widget-typed parameters.
#[test]
fn test_perry_ui_widget_add_child_uses_native_dispatch() {
    use crate::ir::{clear_current_module_source, Expr, Stmt};

    let source = r#"
        import { VStack, Text, type Widget } from "perry/ui";

        const parent = VStack(0, []);
        const child = Text("hello");
        parent.addChild(child);
        parent.removeAllChildren();

        function attach(target: Widget, item: Widget) {
            target.addChild(item);
        }
    "#;
    let module =
        perry_parser::parse_typescript(source, "widget_add_child.ts").expect("source should parse");
    let hir =
        super::lower_module(&module, "test", "widget_add_child.ts").expect("source should lower");
    clear_current_module_source();

    let call = hir.init.iter().find_map(|stmt| match stmt {
        Stmt::Expr(Expr::NativeMethodCall {
            module,
            class_name,
            object,
            method,
            args,
        }) if method == "addChild" => Some((module, class_name, object, args)),
        _ => None,
    });

    assert!(
        matches!(
            call,
            Some((module, Some(class_name), Some(_), args))
                if module == "perry/ui" && class_name == "VStack" && args.len() == 1
        ),
        "Widget.addChild must lower as a perry/ui instance call, got: {:#?}",
        hir.init
    );

    assert!(
        hir.init.iter().any(|stmt| matches!(
            stmt,
            Stmt::Expr(Expr::NativeMethodCall {
                module,
                class_name: Some(class_name),
                object: Some(_),
                method,
                args,
            }) if module == "perry/ui"
                && class_name == "VStack"
                && method == "removeAllChildren"
                && args.is_empty()
        )),
        "Widget.removeAllChildren must lower as a perry/ui instance call, got: {:#?}",
        hir.init
    );

    let attach = hir
        .functions
        .iter()
        .find(|function| function.name == "attach")
        .expect("attach should lower");
    assert!(
        attach.body.iter().any(|stmt| matches!(
            stmt,
            Stmt::Expr(Expr::NativeMethodCall {
                module,
                class_name: Some(class_name),
                object: Some(_),
                method,
                args,
            }) if module == "perry/ui"
                && class_name == "Widget"
                && method == "addChild"
                && args.len() == 1
        )),
        "Widget-typed parameters must use perry/ui instance dispatch, got: {:#?}",
        attach.body
    );
}
