use super::*;

#[test]
fn flat_default_class_export_does_not_redeclare_sibling_bindings() {
    let source = r#"
const shared = 7, marker = 8;
const { other } = { other: 9 };
class First { value() { return shared; } }
class Second { value() { return shared; } }
function helper() { return shared; }
module.exports = First;
module.exports.Second = Second;
module.exports.shared = 99;
module.exports.helper = helper;
module.exports.extra = 123;
module.exports.marker = marker;
module.exports.other = other;
const path = 1;
module.exports.path = require("node:path");
"#;
    let wrapped = wrap_commonjs(source, &PathBuf::from("/tmp/classes.cjs"));
    assert!(wrapped.contains("export default First;"), "{wrapped}");
    assert!(!wrapped.contains("const _cjs = (function()"));
    for name in [
        "Second", "shared", "helper", "extra", "marker", "other", "path",
    ] {
        assert!(
            wrapped.contains(&format!("const __cjsexp_{name} = _cjs.{name};")),
            "flat exports must not redeclare {name}:\n{wrapped}"
        );
        assert!(wrapped.contains(&format!("export {{ __cjsexp_{name} as {name} }};")));
        assert!(!wrapped.contains(&format!("export const {name} =")));
    }
    let ast = perry_parser::parse_typescript(&wrapped, "classes.cjs").unwrap();
    perry_hir::lower_module(&ast, "classes", "/tmp/classes.cjs").unwrap();
}

#[test]
fn iife_export_can_keep_the_name_of_its_inner_binding() {
    let source = "const value = 7; module.exports.value = value;";
    let wrapped = wrap_commonjs(source, &PathBuf::from("/tmp/value.cjs"));
    assert!(wrapped.contains("const _cjs = (function()"));
    assert!(wrapped.contains("export const value = _cjs.value;"));
}

#[test]
fn flat_class_keeps_export_star_and_top_level_return_guard() {
    // #4872 export-star routing shares the wrapper with #4933 flat classes.
    let source = "const shared = 7;\nclass Default { value() { return shared; } }\nmodule.exports = Default;\n__exportStar(require('./star'), exports);";
    let wrapped = wrap_commonjs(source, &PathBuf::from("/tmp/default.cjs"));
    assert!(wrapped.contains("export default Default;"));
    assert!(wrapped.contains("export * from './star';"));
    let with_return = format!("{source}\nreturn;");
    let wrapped = wrap_commonjs(&with_return, &PathBuf::from("/tmp/default.cjs"));
    assert!(wrapped.contains("const _cjs = (function()"));
    assert!(wrapped.contains("export * from './star';"));
}

#[test]
fn flat_class_explicit_self_export_has_only_one_named_export() {
    let source = "const shared = 7;\nclass First { value() { return shared; } }\nmodule.exports = First;\nmodule.exports.First = First;";
    let wrapped = wrap_commonjs(source, &PathBuf::from("/tmp/first.cjs"));
    assert!(wrapped.contains("export default First;"));
    assert!(wrapped.contains("export { __cjsexp_First as First };"));
    assert!(!wrapped.contains("export { First };"), "{wrapped}");
    let ast = perry_parser::parse_typescript(&wrapped, "first.cjs").unwrap();
    perry_hir::lower_module(&ast, "first", "/tmp/first.cjs").unwrap();
}
