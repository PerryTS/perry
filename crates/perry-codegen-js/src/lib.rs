//! JavaScript code generation backend for Perry
//!
//! Compiles HIR modules to JavaScript for `--target web`.
//! Produces a self-contained HTML file with embedded JS runtime.

pub mod emit;
pub mod minify;

pub use emit::AppMetadata;

use anyhow::Result;
use perry_hir::ir::{Module, Stmt};
use std::collections::BTreeSet;

/// Embedded web runtime JavaScript
const WEB_RUNTIME_JS: &str = include_str!("web_runtime.js");

/// Compile a single HIR module to JavaScript source code.
/// Returns (js_source, exported_names).
pub fn compile_module_to_js(module: &Module, minify_names: bool) -> (String, BTreeSet<String>) {
    compile_module_to_js_with_metadata(module, minify_names, AppMetadata::default())
}

/// Compile a single HIR module to JavaScript source code, baking the supplied
/// app metadata into compile-time `perry/system` introspection APIs.
pub fn compile_module_to_js_with_metadata(
    module: &Module,
    minify_names: bool,
    app_metadata: AppMetadata,
) -> (String, BTreeSet<String>) {
    let emitter = emit::JsEmitter::with_metadata(&module.name, minify_names, app_metadata);

    // Collect names that have runtime values (not type-only exports)
    let mut runtime_names = BTreeSet::new();
    for func in &module.functions {
        runtime_names.insert(func.name.clone());
    }
    for class in &module.classes {
        runtime_names.insert(class.name.clone());
    }
    for en in &module.enums {
        runtime_names.insert(en.name.clone());
    }
    for global in &module.globals {
        runtime_names.insert(global.name.clone());
    }
    for stmt in &module.init {
        if let Stmt::Let { name, .. } = stmt {
            runtime_names.insert(name.clone());
        }
    }

    // Collect exported names, filtering out type-only exports
    let mut exported_names = BTreeSet::new();
    for export in &module.exports {
        if let perry_hir::ir::Export::Named { exported, .. } = export {
            if runtime_names.contains(exported) {
                exported_names.insert(exported.clone());
            }
        }
    }

    let js = emitter.emit_module(module);
    (js, exported_names)
}

/// Compile multiple HIR modules into a self-contained HTML file.
///
/// Modules are emitted in topological order (dependency order).
/// The entry module is the last one in the list.
/// When `minify` is true, applies name mangling and whitespace stripping.
pub fn compile_modules_to_html(
    modules: &[(String, Module)], // (module_name, hir_module)
    title: &str,
    minify: bool,
) -> Result<String> {
    compile_modules_to_html_with_metadata(modules, title, minify, AppMetadata::default())
}

/// Same as `compile_modules_to_html` but bakes the supplied app metadata into
/// compile-time `perry/system` introspection APIs.
pub fn compile_modules_to_html_with_metadata(
    modules: &[(String, Module)],
    title: &str,
    minify: bool,
    app_metadata: AppMetadata,
) -> Result<String> {
    let mut all_js = String::with_capacity(32768);
    let mut declared_names = BTreeSet::new();

    // Emit non-entry modules as IIFE-wrapped sections that export their values
    let entry_idx = modules.len().saturating_sub(1);

    for (i, (mod_name, module)) in modules.iter().enumerate() {
        let is_entry = i == entry_idx;

        let (js, exported_names) =
            compile_module_to_js_with_metadata(module, minify, app_metadata.clone());

        if is_entry {
            // Entry module: emit directly (no IIFE wrapper)
            if !minify {
                all_js.push_str("// --- Entry module ---\n");
            }
            all_js.push_str(&js);
        } else if !exported_names.is_empty() {
            // Non-entry module with exports: wrap in IIFE
            let safe_name = sanitize_module_name(mod_name);
            let _ = std::fmt::Write::write_fmt(
                &mut all_js,
                format_args!("const __mod_{} = (() => {{\n", safe_name),
            );
            all_js.push_str(&js);
            all_js.push_str("  return {");
            for (j, name) in exported_names.iter().enumerate() {
                if j > 0 {
                    all_js.push_str(", ");
                }
                all_js.push_str(name);
            }
            all_js.push_str("};\n})();\n");

            // Destructure exports into local scope (skip already-declared names)
            let new_names: Vec<&String> = exported_names
                .iter()
                .filter(|n| !declared_names.contains(n.as_str()))
                .collect();
            if !new_names.is_empty() {
                all_js.push_str("const {");
                for (j, name) in new_names.iter().enumerate() {
                    if j > 0 {
                        all_js.push_str(", ");
                    }
                    all_js.push_str(name);
                    declared_names.insert(name.to_string());
                }
                let _ = std::fmt::Write::write_fmt(
                    &mut all_js,
                    format_args!("}} = __mod_{};\n", safe_name),
                );
            }
        } else {
            // Non-entry module without exports: still wrap in IIFE for scope isolation
            all_js.push_str("(() => {\n");
            all_js.push_str(&js);
            all_js.push_str("})();\n");
        }

        all_js.push('\n');
    }

    // Apply whitespace minification to both user code and web runtime
    let runtime_js = if minify {
        minify::minify_js(WEB_RUNTIME_JS)
    } else {
        WEB_RUNTIME_JS.to_string()
    };
    let final_js = if minify {
        minify::minify_js(&all_js)
    } else {
        all_js
    };

    // Build HTML
    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>{title}</title>
  <style>
    * {{ margin: 0; padding: 0; box-sizing: border-box; }}
    html, body {{ width: 100vw; height: 100vh; overflow: hidden; }}
    #perry-root {{ width: 100%; flex: 1 1 0%; min-height: 0; display: flex; flex-direction: column; overflow: hidden; }}
  </style>
</head>
<body>
  <div id="perry-root"></div>
  <script>
{runtime_js}
  </script>
  <script>
{final_js}
  </script>
</body>
</html>"#,
        title = html_escape(title),
        runtime_js = runtime_js,
        final_js = final_js,
    );

    Ok(html)
}

/// Sanitize a module name for use as a JavaScript identifier
fn sanitize_module_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Basic HTML escaping for title
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
