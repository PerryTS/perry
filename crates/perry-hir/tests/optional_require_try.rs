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
