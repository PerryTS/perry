//! #10359: `new globalThis.<name>(…)` constructs the global property even when
//! a module binding shares the name. Split from `tests.rs` for the 2000-line
//! cap.

fn lowered_function_debug(source: &str, name: &str) -> String {
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");
    let function = hir
        .functions
        .iter()
        .find(|function| function.name == name)
        .unwrap_or_else(|| panic!("{name} is lowered"));
    format!("{function:?}")
}

fn global_property_construct(name: &str) -> String {
    format!(
        r#"NewDynamic {{ callee: PropertyGet {{ object: GlobalGet(0), property: "{name}", byte_offset: 0, byte_end: 0 }}"#
    )
}

/// The issue's shape: an import, a class, a function and a local each shadow a
/// global constructor with no dedicated intrinsic HIR node. Each used to lower
/// to a by-name construct (`New { class_name }` / `FuncRef` / `LocalGet`) that
/// bound to the shadowing binding; each must construct the global property,
/// exactly like the aliased `const E = globalThis.Event; new E()` form.
#[test]
fn shadowed_global_constructor_reads_the_global_property() {
    let source = r#"
        import { Event } from "./ev";
        class Headers { tag = 1 }
        function Request(this: any) { this.tag = 2; }
        export function viaImport(): any { return new globalThis.Event("ping"); }
        export function viaClass(): any { return new globalThis.Headers({ a: "1" }); }
        export function viaFunction(): any { return new globalThis.Request("http://x.test/"); }
        export function viaLocal(): any {
            const MessageChannel = function () {};
            return new globalThis.MessageChannel();
        }
        export function viaGlobalAlias(): any {
            const g = globalThis;
            return new g.Headers();
        }
    "#;
    for (function, name) in [
        ("viaImport", "Event"),
        ("viaClass", "Headers"),
        ("viaFunction", "Request"),
        ("viaLocal", "MessageChannel"),
        ("viaGlobalAlias", "Headers"),
    ] {
        let debug = lowered_function_debug(source, function);
        assert!(
            debug.contains(&global_property_construct(name)),
            "{function}: `new globalThis.{name}()` must construct the global property:\n{debug}"
        );
        assert!(
            !debug.contains(&format!(r#"New {{ class_name: "{name}""#)),
            "{function}: a by-name construct binds to the shadowing `{name}`:\n{debug}"
        );
    }
}

/// Guards the other side: unrelated builtins keep their existing intrinsic
/// route, while Map reads the global constructor value for both spellings.
#[test]
fn map_constructor_forms_read_the_global_value_once() {
    let source = r#"
        export function unshadowed(): any { return new globalThis.Event("ping"); }
        export function bareMap(): any { return new Map([[1, 2]]); }
        export function dedicated(): any { return new globalThis.Map([[1, 2]]); }
    "#;
    let unshadowed = lowered_function_debug(source, "unshadowed");
    assert!(
        unshadowed.contains(r#"New { class_name: "Event""#),
        "an unshadowed global keeps the by-name intrinsic construct:\n{unshadowed}"
    );
    let bare_map = lowered_function_debug(source, "bareMap");
    assert!(
        bare_map.contains(&global_property_construct("Map")),
        "an unshadowed `new Map` must evaluate the global constructor:\n{bare_map}"
    );
    assert!(
        !bare_map.contains("MapNewFromArray") && !bare_map.contains("MapNew)"),
        "bare Map construction must not skip the observable global Get:\n{bare_map}"
    );
    let dedicated = lowered_function_debug(source, "dedicated");
    assert!(
        dedicated.contains(&global_property_construct("Map")),
        "`new globalThis.Map` must evaluate the global constructor:\n{dedicated}"
    );

    let shadowed_global = lowered_function_debug(
        r#"
            import { Map } from "./m";
            export function dedicated(): any { return new globalThis.Map([[1, 2]]); }
        "#,
        "dedicated",
    );
    assert!(
        shadowed_global.contains(&global_property_construct("Map")),
        "globalThis.Map must remain the global value even when bare Map is imported:\n{shadowed_global}"
    );
}

#[test]
fn resolved_map_types_do_not_brand_live_global_constructions() {
    use crate::types::Type;
    use std::collections::HashMap;
    use swc_ecma_ast::{Decl, ModuleItem, Pat, Stmt};

    let source = r#"
        const bare = new Map<number, number>();
        const member = new globalThis.Map<number, number>();
    "#;
    let ast = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let span_for = |index: usize| {
        let ModuleItem::Stmt(Stmt::Decl(Decl::Var(decl))) = &ast.body[index] else {
            panic!("top-level item {index} is a variable declaration");
        };
        let Pat::Ident(binding) = &decl.decls[0].name else {
            panic!("declaration {index} is an identifier");
        };
        binding.id.span.lo.0
    };
    let resolved_map = Type::Generic {
        base: "Map".to_string(),
        type_args: vec![Type::Number, Type::Number],
    };
    let resolved_types = HashMap::from([
        (span_for(0), resolved_map.clone()),
        (span_for(1), resolved_map),
    ]);
    let hir =
        super::lower_module_with_class_id_and_types(&ast, "t", "t.ts", 1, Some(resolved_types))
            .expect("source lowers")
            .0;
    let debug = format!("{hir:?}");
    assert!(
        debug.contains(r#"name: "bare", ty: Any"#),
        "tsgo's generic result must not restore a brand for bare Map:\n{debug}"
    );
    assert!(
        debug.contains(r#"name: "member", ty: Any"#),
        "tsgo's generic result must not restore a brand for globalThis.Map:\n{debug}"
    );
}
