use super::{extract_exports_from_source, wrap_commonjs, PathBuf};

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
