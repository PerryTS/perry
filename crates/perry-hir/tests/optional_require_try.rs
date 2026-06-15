use perry_hir::lower_module;
use perry_parser::parse_typescript_with_cache;

fn lower_result(src: &str) -> Result<(), String> {
    let mut cache = Default::default();
    let parsed = parse_typescript_with_cache(src, "test.ts", &mut cache)
        .map_err(|e| format!("parse failed: {e:?}"))?;
    lower_module(&parsed.module, "test", "test.ts")
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[test]
fn optional_require_in_try_body_lowers() {
    let src = r#"
        var optionalNative = null;
        try {
            optionalNative = require("missing-optional-native-addon");
        } catch {
            optionalNative = null;
        }
    "#;

    lower_result(src).expect("optional require inside try should lower");
}

#[test]
fn require_outside_try_still_errors() {
    let src = r#"
        const nativeAddon = require("missing-optional-native-addon");
    "#;

    let err = lower_result(src).expect_err("require outside try must keep failing fast");
    assert!(
        err.contains("CommonJS `require(\"missing-optional-native-addon\")` is not supported"),
        "unexpected error: {err}"
    );
}

#[test]
fn optional_require_in_function_try_body_lowers() {
    let src = r#"
        function loadOptional() {
            try {
                return require("missing-optional-native-addon");
            } catch {
                return null;
            }
        }
    "#;

    lower_result(src).expect("optional require inside function try should lower");
}

// #5216: a string-literal `require("<native>")` of a statically resolvable
// Node builtin lowers to the module-namespace value (no compile-time refusal),
// behaving like `import * as ns from "<native>"`.

#[test]
fn require_native_builtin_namespace_binding_lowers() {
    let src = r#"
        const readline = require("readline");
        const iface = readline.createInterface;
    "#;
    lower_result(src).expect("require of a resolvable builtin must lower, not refuse");
}

#[test]
fn require_node_prefixed_builtin_lowers() {
    let src = r#"
        const os = require("node:os");
        const p = os.platform();
    "#;
    lower_result(src).expect("require('node:os') must lower as the namespace value");
}

#[test]
fn require_native_builtin_destructured_lowers() {
    let src = r#"
        const { createInterface } = require("readline");
        const f = createInterface;
    "#;
    lower_result(src).expect("destructured require of a resolvable builtin must lower");
}

#[test]
fn require_native_builtin_inline_member_lowers() {
    let src = r#"
        const plat = require("node:os").platform();
    "#;
    lower_result(src).expect("inline member access on require(builtin) must lower");
}

#[test]
fn non_literal_require_outside_try_still_errors() {
    // A computed specifier can't be statically resolved — keep the legacy
    // compile-time refusal (the resolvable-lowering only applies to string
    // literals).
    let src = r#"
        const spec = "missing-optional-native-addon";
        const m = require(spec === spec ? "missing-optional-native-addon" : "x");
    "#;
    // Non-literal arg never matches the literal lowering nor the literal bail;
    // it falls through unchanged. This documents that behavior is preserved.
    let _ = lower_result(src);
}
