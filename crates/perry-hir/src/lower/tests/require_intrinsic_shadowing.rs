//! `require` intrinsic vs. user/ambient shadowing tests (split out of
//! `lower/tests.rs` to keep it under the 2,000-line cap, #10750).

/// #8447: the ambient-typing idiom `declare function require(name: string): any`
/// names the global require intrinsic — it must NOT be registered as an
/// external FFI function. That registration made every require-shadowing guard
/// (`require_is_shadowed_by_local`, `try_require_literal`) treat the global as
/// shadowed since #8343, so `require("node:fs")` lowered to a call to a
/// `require` symbol no archive defines, and every consumer failed at link
/// (`Undefined symbols: "_require"`).
#[test]
fn test_ambient_require_declare_does_not_shadow_the_intrinsic() {
    let source = r#"
        declare function require(name: string): any;
        function probe(): string {
            const fs = require("node:fs");
            return typeof fs.constants.O_RDONLY;
        }
        console.log(probe());
    "#;
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");
    let dump = format!("{hir:?}");
    assert!(
        !dump.contains("ExternFuncRef { name: \"require\""),
        "an ambient `declare function require` must not lower calls to an \
         extern `require` symbol — nothing defines it, so linking fails: {dump}"
    );
    assert!(
        dump.contains("\"fs\""),
        "the require(\"node:fs\") call must resolve to the fs native module: {dump}"
    );
}

/// The counterpart (#8343's intent, unchanged): a `function require(...)` WITH
/// a body — e.g. the CJS wrap's synthetic require — is a real user binding and
/// must keep shadowing the intrinsic, so the call stays a plain user-function
/// call instead of a native-module namespace binding.
#[test]
fn test_user_require_function_with_body_still_shadows_the_intrinsic() {
    let source = r#"
        function require(name: string): string { return "shadowed:" + name; }
        const fs = require("node:fs");
        console.log(fs);
    "#;
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");
    let dump = format!("{hir:?}");
    assert!(
        !dump.contains("NativeModuleRef(\"fs\")"),
        "a user `function require` with a body shadows the intrinsic — the \
         call must not be rewritten into a native-module namespace: {dump}"
    );
}

/// #8465: `const require = createRequire(import.meta.url)` binds the REAL
/// module-scoped require — `const net = require("net")` must still take the
/// static native-namespace fast path (as it did before #8343's shadow guard),
/// not flow to the runtime createRequire surface, where `net.connect` reached
/// as a bound value dispatches through a null-by-default function pointer and
/// silently returns undefined.
#[test]
fn test_create_require_local_keeps_the_native_namespace_fast_path() {
    let source = r#"
        import { createRequire } from "node:module";
        const require = createRequire(import.meta.url);
        const net = require("net");
        console.log(typeof net.connect);
    "#;
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");
    let dump = format!("{hir:?}");
    assert!(
        dump.contains("NativeModuleRef(\"net\")"),
        "require(\"net\") under a createRequire-backed local must fold to the \
         static native namespace: {dump}"
    );
    assert!(
        !dump.contains("name: \"net\""),
        "the namespace binding must not leave a runtime `net` local behind: {dump}"
    );
}

/// Bun exposes a synchronous module loader as `import.meta.require`.  It must
/// share Perry's synchronous dynamic-require path; the generic import.meta
/// member lowering intentionally maps unknown properties to `undefined`.
#[test]
fn import_meta_require_lowers_to_synchronous_module_dispatch() {
    let source = r#"
        const direct = import.meta.require("/$bunfs/root/chunk-a.js");
        const computed = import.meta["require"]("./chunk-b.js");
        console.log(direct, computed);
    "#;
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");
    let dump = format!("{hir:#?}");
    assert_eq!(
        dump.matches("synchronous: true").count(),
        2,
        "both import.meta.require spellings must use synchronous module dispatch: {dump}"
    );
    assert!(
        dump.contains("/$bunfs/root/chunk-a.js") && dump.contains("./chunk-b.js"),
        "the original specifiers must reach the module collector: {dump}"
    );
}

/// The #8465 counterpart, complementary to
/// `test_user_require_function_with_body_still_shadows_the_intrinsic` above:
/// that one pins that a real `function require` body suppresses the fold; this
/// one additionally pins that the bound name survives as a runtime local, which
/// is what the CJS wrap's synthetic require depends on.
#[test]
fn test_function_require_with_body_still_shadows_the_namespace_fast_path() {
    let source = r#"
        function require(name: string): any { return { connect: 1 }; }
        const net = require("net");
        console.log(typeof net.connect);
    "#;
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");
    let dump = format!("{hir:?}");
    assert!(
        !dump.contains("NativeModuleRef(\"net\")"),
        "a real `function require` body must keep shadowing: {dump}"
    );
    assert!(
        dump.contains("name: \"net\""),
        "the `net` binding must stay a runtime local under a shadowing require: {dump}"
    );
}

// `test_cjs_wrapper_lru_cache_destructure_uses_static_constructor` removed
// here -- it asserted `const { LRUCache } = require("lru-cache"); new
// LRUCache(...)` lowers to the static native constructor
// (`cjs_wrapper_static_native_destructure` in `var_decl_sources.rs`), which
// no longer exists now that lru-cache's native binding is gone (#10685).
// The same CJS-destructure shape now goes through the ordinary
// resolvable-native-module path (any Node-builtin or well-known module,
// not lru-cache specifically), unaffected by this removal.
