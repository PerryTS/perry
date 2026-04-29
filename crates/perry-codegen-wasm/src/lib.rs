//! WebAssembly code generation backend for Perry
//!
//! Compiles HIR modules to WebAssembly binary format for `--target wasm`.
//! Produces a self-contained HTML file with embedded WASM (base64) and JS runtime bridge.
//!
//! All JSValues use NaN-boxing (f64) consistent with perry-runtime.
//! Runtime operations (strings, console, objects) are imported from JavaScript.

pub mod emit;

use anyhow::Result;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use perry_hir::ir::Module;

/// Embedded WASM runtime JavaScript (bridge between WASM and browser APIs)
const WASM_RUNTIME_JS: &str = include_str!("wasm_runtime.js");

/// Compile multiple HIR modules into a self-contained HTML file with embedded WASM.
pub fn compile_modules_to_wasm_html(
    modules: &[(String, Module)],
    title: &str,
    minify: bool,
) -> Result<String> {
    let output = emit::compile_to_wasm_with_async(modules);
    let wasm_b64 = BASE64.encode(&output.wasm_bytes);

    let runtime_js = if minify {
        perry_codegen_js::minify::minify_js(WASM_RUNTIME_JS)
    } else {
        WASM_RUNTIME_JS.to_string()
    };

    // If there are async functions, inject them into the runtime
    let async_inject = if output.async_js.is_empty() {
        String::new()
    } else {
        format!("\n// === Generated async function implementations ===\nconst __asyncFuncImpls = {{\n{}\n}};\n", output.async_js)
    };

    // If there are FFI imports, generate a comment listing them for the host to provide
    let ffi_comment = if output.ffi_imports.is_empty() {
        String::new()
    } else {
        format!(
            "\n// === FFI imports required (provide via __ffiImports or bootPerryWasm 2nd arg) ===\n// {}\n",
            output.ffi_imports.join(", ")
        )
    };

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
{runtime_js}{async_inject}{ffi_comment}
  </script>
  <script>
window.__perryWasmB64 = "{wasm_b64}";
bootPerryWasm("{wasm_b64}").catch(e => {{
  document.getElementById("perry-root").textContent = "WASM Error: " + e.message;
  console.error("Boot error:", e);
}});
  </script>
</body>
</html>"#,
        title = html_escape(title),
        runtime_js = runtime_js,
        async_inject = async_inject,
        ffi_comment = ffi_comment,
        wasm_b64 = wasm_b64,
    );

    Ok(html)
}

/// Get the list of FFI function names that a WASM module requires as imports.
pub fn get_ffi_imports(modules: &[(String, Module)]) -> Vec<String> {
    let output = emit::compile_to_wasm_with_async(modules);
    output.ffi_imports
}

/// Get the raw WASM binary (for non-HTML output)
pub fn compile_modules_to_wasm(modules: &[(String, Module)]) -> Result<Vec<u8>> {
    Ok(emit::compile_to_wasm(modules))
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
