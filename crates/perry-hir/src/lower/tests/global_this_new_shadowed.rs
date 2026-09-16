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
        r#"NewDynamic {{ callee: PropertyGet {{ object: GlobalGet(0), property: "{name}", byte_offset: 0 }}"#
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

/// Guards the other side: an unshadowed name keeps its by-name intrinsic
/// construct, and a shadowed name WITH a dedicated intrinsic node (#6726's
/// `class Set {}` case) keeps that node rather than going dynamic.
#[test]
fn unshadowed_and_dedicated_intrinsics_keep_their_lowering() {
    let source = r#"
        import { Map } from "./m";
        export function unshadowed(): any { return new globalThis.Event("ping"); }
        export function dedicated(): any { return new globalThis.Map([[1, 2]]); }
    "#;
    let unshadowed = lowered_function_debug(source, "unshadowed");
    assert!(
        unshadowed.contains(r#"New { class_name: "Event""#),
        "an unshadowed global keeps the by-name intrinsic construct:\n{unshadowed}"
    );
    let dedicated = lowered_function_debug(source, "dedicated");
    assert!(
        dedicated.contains("MapNewFromArray"),
        "a shadowed global with a dedicated intrinsic node keeps it:\n{dedicated}"
    );
    assert!(
        !dedicated.contains(&global_property_construct("Map")),
        "the dedicated intrinsic node must not be replaced by a dynamic construct:\n{dedicated}"
    );
}
