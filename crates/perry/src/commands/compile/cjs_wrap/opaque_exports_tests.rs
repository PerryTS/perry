use super::*;
use std::collections::BTreeSet;

#[test]
fn module_exports_opacity_follows_the_assigned_value() {
    // UMD, call results, identifiers and re-export wrappers hide the names.
    assert!(module_exports_is_opaque(
        "(function (g, f) {\n  if (typeof module === 'object' && module.exports)\n    module.exports = f(require('x'));\n})(this, function () {});"
    ));
    assert!(module_exports_is_opaque(
        "function pino() {}\nmodule.exports = pino;"
    ));
    assert!(module_exports_is_opaque(
        "module['exports'] = require('./impl');"
    ));
    // Property writes and object literals list them completely.
    assert!(!module_exports_is_opaque(
        "exports.a = 1;\nmodule.exports.b = 2;"
    ));
    assert!(!module_exports_is_opaque("module.exports = {\n  a: 1,\n};"));
    assert!(!module_exports_is_opaque(
        "if (module.exports === undefined) {}"
    ));
}

#[test]
fn demanded_names_are_exported_as_module_exports_snapshots() {
    let source = "module.exports = factory();\nfunction factory() { return { pg_query: 1 }; }";
    let demanded = BTreeSet::from([
        "pg_query".to_string(),
        "class".to_string(),
        "a-b".to_string(),
    ]);
    let (wrapped, _) = wrap_commonjs_with_addon_paths(
        source,
        Path::new("/fixture/proto.js"),
        None,
        false,
        None,
        None,
        Some(&demanded),
        &mut Vec::new(),
        None,
    );
    assert!(
        wrapped.contains("export const pg_query = _cjs.pg_query;"),
        "{wrapped}"
    );
    // Names no `export const` can bind stay unexported (the import still fails).
    assert!(!wrapped.contains("_cjs.class"), "{wrapped}");
    assert!(!wrapped.contains("a-b ="), "{wrapped}");
}
