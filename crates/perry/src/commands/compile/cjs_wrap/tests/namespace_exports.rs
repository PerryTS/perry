//! A CommonJS module's ESM view exports exactly the keys the module writes to
//! `exports`/`module.exports`. Hoisting a top-level class out of the wrapper
//! (for class identity) must not turn the class into an export, and a key is
//! the class binding itself only when every write of the key stores it.
use super::super::extract_exports::export_key_is_own_binding;
use super::*;

#[test]
fn hoisted_private_class_is_not_exported() {
    let source = r#""use strict";
class Hidden { hi() { return 1; } }
class Base { base() { return 2; } }
class Pub extends Base { pub() { return new Hidden().hi(); } }
exports.Pub = Pub;
exports.Alias = Base;
"#;
    let wrapped = wrap_commonjs(source, &PathBuf::from("/tmp/protocols.cjs"));
    // All three classes are hoisted to module scope.
    assert!(wrapped.contains("class Hidden {"), "{wrapped}");
    assert!(wrapped.contains("export { Pub };"), "{wrapped}");
    assert!(!wrapped.contains("export { Hidden };"), "{wrapped}");
    assert!(!wrapped.contains("export { Base };"), "{wrapped}");
    // `Alias` is the `Base` class read back from the module's exports.
    assert!(wrapped.contains("export const Alias = _cjs.Alias;"), "{wrapped}");
}

#[test]
fn hoisted_class_key_holding_another_value_reads_the_export() {
    let source = r#"class Boxed { boxed() { return 1; } }
function box(C) { return { inner: C }; }
exports.Boxed = box(Boxed);
"#;
    let wrapped = wrap_commonjs(source, &PathBuf::from("/tmp/boxed.cjs"));
    assert!(wrapped.contains("class Boxed {"), "{wrapped}");
    assert!(!wrapped.contains("export { Boxed };"), "{wrapped}");
    // The key's value is the module's, under a name that does not redeclare
    // the module-scope class binding.
    assert!(wrapped.contains("const __cjsexp_Boxed = _cjs.Boxed;"), "{wrapped}");
    assert!(wrapped.contains("export { __cjsexp_Boxed as Boxed };"), "{wrapped}");
}

#[test]
fn export_key_is_own_binding_needs_every_write_to_store_the_binding() {
    for source in [
        "exports.X = X;",
        "module.exports.X = X\nexports.Y = Y\n",
        "exports['X'] = X;",
        "module.exports = { X };",
        "module.exports = { X: X, Y: 1 };",
        "__export(src_exports, {\n  X: () => X,\n  Y: () => other\n});",
    ] {
        assert!(export_key_is_own_binding(source, "X"), "{source}");
    }
    for source in [
        "",
        "exports.Y = X;",
        "exports.X = wrap(X);",
        "exports.X = X.inner;",
        "exports.X = X\n.inner;",
        "exports.X = X = other;",
        "exports.XY = XY;",
        "exports.X = X;\nexports.X = other;",
        "module.exports = { X: () => X };",
        "module.exports = { X: other };",
        "Object.defineProperty(exports, 'X', { get: () => X });",
        "exports.X = X;\nObject.defineProperty(exports, \"X\", { value: 1 });",
    ] {
        assert!(!export_key_is_own_binding(source, "X"), "{source}");
    }
}
