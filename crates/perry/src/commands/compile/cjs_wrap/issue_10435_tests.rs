//! Regression tests for #10435: packages such as `abstract-logging` define
//! their complete CommonJS export with a descriptor on the `module` object.

use super::detect::is_commonjs;

#[test]
fn detects_module_exports_descriptor() {
    let single = r#"
'use strict'
function noop() {}
Object.defineProperty(module, 'exports', {
  get() { return { info: noop }; }
})
"#;
    let double = r#"Object . defineProperty ( module , "exports", { value: {} });"#;
    assert!(is_commonjs(single));
    assert!(is_commonjs(double));
}

#[test]
fn descriptor_signal_is_precise() {
    assert!(!is_commonjs(
        r#"const text = "Object.defineProperty(module, 'exports', {})";"#
    ));
    assert!(!is_commonjs(
        "// Object.defineProperty(module, 'exports', {})\nconst value = 1;"
    ));
    assert!(!is_commonjs(
        "const module = {}; Object.defineProperty(module, 'other', { value: 1 });"
    ));
    assert!(!is_commonjs(
        "const suffix = ''; Object.defineProperty(module, 'exports' + suffix, { value: 1 });"
    ));
    assert!(!is_commonjs(
        "import value from './value.js'; Object.defineProperty(module, 'exports', { value });"
    ));
}
