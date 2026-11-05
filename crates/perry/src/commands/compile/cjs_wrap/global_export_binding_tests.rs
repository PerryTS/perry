use super::wrap::wrap_commonjs_for_target;
use std::path::Path;

/// A rolldown bundle's factories (`__commonJSMin((exports) => { ... })`) assign
/// `exports.Buffer` and declare `const Buffer` in nested scopes, while another
/// factory reads the global `Buffer` freely. The named export of `Buffer` must
/// not become a module-scope `export const Buffer = _cjs.Buffer`: the free
/// `typeof Buffer` would resolve to that binding in its dead zone
/// ("Cannot access 'Buffer' before initialization").
#[test]
fn global_named_export_never_shadows_free_global_with_nested_declaration() {
    let source = r#"var __commonJSMin = (cb, mod) => () => (mod || cb((mod = { exports: {} }).exports, mod), mod.exports);
var require_safe = __commonJSMin(((exports) => { exports.Buffer = 7; }));
var require_align = __commonJSMin(((exports) => { const Buffer = require_safe().Buffer; exports.n = Buffer; }));
var require_other = __commonJSMin(((exports) => { exports.ok = typeof Buffer; }));
exports.Buffer = require_align().n;
module.exports.ok = require_other().ok;
"#;
    let wrapped =
        wrap_commonjs_for_target(source, Path::new("/project/bundle.cjs"), None, true, None);
    assert!(
        !wrapped.contains("export const Buffer"),
        "a module-scope `Buffer` binding would shadow the global:\n{wrapped}"
    );
    assert!(
        wrapped.contains("export { __cjsexp_Buffer as Buffer };"),
        "the export must surface through the mangled binding:\n{wrapped}"
    );
}

/// The same for a Web global the runtime provides: a bundle exporting undici's
/// `Response` must not shadow the global `Response` its other modules construct.
#[test]
fn web_global_named_export_never_shadows_free_global() {
    let source = r#"var __commonJSMin = (cb, mod) => () => (mod || cb((mod = { exports: {} }).exports, mod), mod.exports);
var require_response = __commonJSMin(((exports) => { class Response { } exports.Response = Response; }));
var handler = () => new Response("ok");
exports.Response = require_response().Response;
exports.handler = handler;
"#;
    let wrapped =
        wrap_commonjs_for_target(source, Path::new("/project/bundle.cjs"), None, true, None);
    assert!(
        !wrapped.contains("export const Response"),
        "a module-scope `Response` binding would shadow the global:
{wrapped}"
    );
    assert!(
        wrapped.contains("export { __cjsexp_Response as Response };"),
        "the export must surface through the mangled binding:
{wrapped}"
    );
}

/// Every global binding the runtime defines (its realm declarations): a CJS
/// `exports.<global> = ...` / `module.exports.<global> = ...` never becomes a
/// module-scope binding that would shadow the global for the body's free
/// references; the export surfaces through the mangled binding.
#[test]
fn no_runtime_global_is_shadowed_by_a_cjs_named_export() {
    let globals: Vec<&str> = perry_codegen::realm_manifest::REALM
        .iter()
        .map(|entry| entry.name)
        .filter(|name| super::wrap::is_global_binding_name(name))
        .chain(["globalThis", "undefined", "NaN", "Infinity"])
        .collect();
    assert!(
        globals.len() > 100,
        "the realm should declare the runtime's globals: {}",
        globals.len()
    );
    for (index, global) in globals.iter().enumerate() {
        assert!(
            super::wrap::is_runtime_global_name(global),
            "{global} is not classified as global"
        );
        // Alternate the two assignment spellings.
        let assign = if index % 2 == 0 {
            "exports"
        } else {
            "module.exports"
        };
        let source = format!(
            "var __commonJSMin = (cb, mod) => () => (mod || cb((mod = {{ exports: {{}} }}).exports, mod), mod.exports);\n\
             var require_inner = __commonJSMin(((exports) => {{ exports.value = 1; }}));\n\
             var reads = () => typeof {global};\n\
             {assign}.{global} = require_inner().value;\n\
             exports.reads = reads;\n"
        );
        let wrapped =
            wrap_commonjs_for_target(&source, Path::new("/project/bundle.cjs"), None, true, None);
        assert!(
            !wrapped.contains(&format!("export const {global} ")),
            "`export const {global}` would shadow the global `{global}`:\n{wrapped}"
        );
        assert!(
            wrapped.contains(&format!("export {{ __cjsexp_{global} as {global} }};")),
            "`{global}` must surface through the mangled binding:\n{wrapped}"
        );
    }
}
