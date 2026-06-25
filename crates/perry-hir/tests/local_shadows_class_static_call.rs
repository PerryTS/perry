use perry_diagnostics::SourceCache;
use perry_hir::lower_module;
use perry_parser::parse_typescript_with_cache;

fn lower_debug(src: &str) -> String {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(src, "local_shadows_class_static_call.ts", &mut cache)
        .expect("parse should succeed");
    let module = lower_module(&parsed.module, "test", "local_shadows_class_static_call.ts")
        .expect("lowering should succeed");
    format!("{module:#?}")
}

/// #5437 (Next.js dynamic/API routes 500): minified bundles reuse single-letter
/// names, so a function can have a LOCAL `let n = new Set()` while a same-named
/// `class n { static has(e, t) { ... } }` lives at module scope. JS lexical
/// scoping says the local shadows the class, so `n.has(x)` must be an instance
/// call on the Set local — NOT a static call on the class.
///
/// Before the fix, `try_static_method_and_instance` saw `ctx.lookup_class("n")`
/// succeed and lowered `n.has(x)` to `StaticMethodCall { class_name: "n",
/// method_name: "has" }`, calling the class's `static has(e, t)` with
/// `(x, undefined)` → `Reflect.has(x, undefined)` → "Reflect.has called on
/// non-object". (This is exactly Next.js's `app-route-turbo` implicit-tags
/// builder hitting its own `class n` proxy-handler.)
#[test]
fn local_set_shadows_same_named_class_static_has() {
    let debug = lower_debug(
        r#"
        class n {
            static has(e: any, t: any): any { return e; }
        }

        function build(): any {
            const n = new Set<string>();
            n.add("a");
            return n.has("a");
        }

        const out = build();
        const ref: any = n; // keep the class live so it is not pruned
        "#,
    );

    assert!(
        !debug.contains("StaticMethodCall {\n            class_name: \"n\""),
        "shadowed local `n.has()` must not lower to a StaticMethodCall on class n: {debug}"
    );
    // The local Set's `.has` should lower to the Set intrinsic.
    assert!(
        debug.contains("SetHas"),
        "the local Set's `.has` should lower to SetHas: {debug}"
    );
}

/// Companion: a genuine static-method call on a class that is NOT shadowed by a
/// local of the same name must still lower to `StaticMethodCall` — the fix must
/// not regress real static calls.
#[test]
fn unshadowed_class_static_call_stays_static() {
    let debug = lower_debug(
        r#"
        class Helper {
            static run(x: number): number { return x + 1; }
        }
        const out = Helper.run(41);
        "#,
    );

    assert!(
        debug.contains("StaticMethodCall"),
        "unshadowed `Helper.run()` must still lower to StaticMethodCall: {debug}"
    );
}
