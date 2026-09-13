use super::{extract_exports_from_source, wrap_commonjs, PathBuf};

#[test]
fn detects_whitespace_separated_descriptor_exports() {
    let source = "Object . defineProperty (\n exports , 'answer', { value: 42 });";
    assert!(super::is_commonjs(source));
    assert_eq!(extract_exports_from_source(source), ["answer"]);
}

#[test]
fn descriptor_and_late_exports_are_values_not_declared_functions() {
    let source = r#"
Object.defineProperty(exports, "__esModule", { value: true });
Object.defineProperty(exports, "transform", {
  enumerable: true, get: function () { return impl.transform; }
});
Object.defineProperty(module.exports, 'value', { value: function () { return 42; } });
exports.update = function () { exports.late = function () { return 43; }; };
// Object.defineProperty(exports, "comment", { value: 0 });
var text = 'Object.defineProperty(exports, "text", { value: 0 });';
other.Object.defineProperty(exports, "other", { value: 0 });
Object.defineProperty(other.exports, "inner", { value: 0 });
"#;
    let names = extract_exports_from_source(source);
    assert_eq!(names, ["update", "late", "transform", "value"]);
    let wrapped = wrap_commonjs(source, &PathBuf::from("/tmp/descriptor/index.cjs"));
    for name in &names {
        assert!(wrapped.contains(&format!("export const {name} = _cjs.{name};")));
        assert!(!wrapped.contains(&format!("export function {name}")));
    }
    let ast = perry_parser::parse_typescript(&wrapped, "index.cjs").unwrap();
    let hir = perry_hir::lower_module(&ast, "index", "/tmp/descriptor/index.cjs").unwrap();
    for name in names {
        assert!(hir
            .exported_objects
            .iter()
            .any(|exported| exported == &name));
        assert!(!hir
            .functions
            .iter()
            .any(|function| function.is_exported && function.name == name));
        assert!(!hir
            .exported_functions
            .iter()
            .any(|(exported, _)| exported == &name));
    }
}

#[test]
fn extracts_esbuild_export_helper_keys() {
    let src = r#"
var __export = (target, all) => {
  for (var name in all) Object.defineProperty(target, name, { get: all[name] });
};
var src_exports = {};
__export(src_exports, {
  getContext: () => import_get_context.getContext,
  refreshToken: () => refreshToken,
  default: () => src_default
});
module.exports = __toCommonJS(src_exports);
0 && (module.exports = { getContext, refreshToken });
"#;
    let names = extract_exports_from_source(src);
    assert_eq!(
        names,
        vec!["getContext".to_string(), "refreshToken".to_string()]
    );
}

#[test]
fn wraps_esbuild_export_helper_as_named_esm_exports() {
    let src = r#"
var src_exports = {};
__export(src_exports, { getContext: () => getContext });
function getContext() { return {}; }
module.exports = __toCommonJS(src_exports);
"#;
    let wrapped = wrap_commonjs(src, &PathBuf::from("/tmp/vercel-oidc/index.js"));
    assert!(
        wrapped.contains("export const getContext = _cjs.getContext;"),
        "expected esbuild named export, got:\n{}",
        wrapped
    );
    assert!(perry_parser::parse_typescript(&wrapped, "index.js").is_ok());
}
