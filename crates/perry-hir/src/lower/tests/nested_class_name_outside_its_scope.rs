//! A class expression bound to a function-local `var` that captures that
//! function's locals is only in scope inside that function. The module-wide
//! class table still knows its name, so `new <name>()` in another function —
//! with no binding of its own — used to construct that class statically and
//! forward the declaring function's locals as its environment, which are not
//! in scope there (every capture read `undefined`). Rolldown bundles hit this:
//! undici's `var Response = class {..}` in one factory, the app's
//! `new Response(..)` in another after `globalThis.Response` was replaced.

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

#[test]
fn new_outside_the_declaring_function_resolves_the_global_name() {
    let source = r#"
        export function factory(): any {
            const webidl = { util: { mark(_x: unknown) { return 1; } } };
            var Response = class { constructor() { webidl.util.mark(this); } };
            const inside = new Response();
            return { Response, inside };
        }
        export function serve(): any {
            // @ts-ignore
            return new Response("body");
        }
    "#;
    let serve = lowered_function_debug(source, "serve");
    assert!(
        serve.contains("js_global_get_or_throw_unresolved")
            && serve.contains(r#"String("Response")"#),
        "`new Response()` outside the factory must resolve the global name:\n{serve}"
    );
    assert!(
        !serve.contains(r#"New { class_name: "Response""#),
        "a static construct forwards the factory's locals out of scope:\n{serve}"
    );
    // Positive control: inside its own function the class keeps the static
    // construct with its captured environment.
    let factory = lowered_function_debug(source, "factory");
    assert!(
        factory.contains(r#"New { class_name: "Response""#)
            || factory.contains("ClassEnvStamp")
            || factory.contains("NewDynamic"),
        "the factory still constructs its own class:\n{factory}"
    );
    assert!(
        !factory.contains("js_global_get_or_throw_unresolved"),
        "inside its scope the class must not be resolved through the global:\n{factory}"
    );
}

#[test]
fn a_module_class_extending_a_factory_class_value_stays_in_scope() {
    // `C` at module scope is a value (the class `make` returns), not the
    // class `C` declared inside `make`. The name-keyed capture table must not
    // hand `Child` the factory's locals: `Child` is declared at module scope,
    // so `new Child()` there constructs it, never `globalThis.Child`.
    let source = r#"
        function make(n: number) {
            class C { get value() { return n; } }
            return C;
        }
        const C = make(1);
        class Child extends C {}
        export function build(): any {
            return new Child();
        }
    "#;
    let build = lowered_function_debug(source, "build");
    assert!(
        !build.contains("js_global_get_or_throw_unresolved"),
        "`new Child()` must not resolve `Child` on globalThis:\n{build}"
    );
}
