//! Module loader for V8 runtime
//!
//! Handles loading JavaScript modules from node_modules and local paths.

use anyhow::{anyhow, Result};
use deno_core::error::ModuleLoaderError;
use deno_core::{
    ModuleLoadOptions, ModuleLoadReferrer, ModuleLoadResponse, ModuleLoader, ModuleSource,
    ModuleSourceCode, ModuleSpecifier, ModuleType, ResolutionKind,
};
use deno_error::JsErrorBox;
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;
use std::ffi::{c_char, CStr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

// Issue #818 follow-up: embedded module map for self-contained V8-fallback
// binaries. The compile pipeline emits a generated `.c` file (one entry per
// JS module pulled into the bundle by `collect_js_module_imports`) whose
// `__attribute__((constructor))` calls `js_register_embedded_module` for
// each `(canonical_path, source)` pair plus `js_register_embedded_alias`
// for each `(bare_specifier, canonical_path)` import edge. At runtime the
// `NodeModuleLoader` consults these maps BEFORE touching `node_modules/`,
// so the resulting binary boots correctly even when shipped without the
// source tree's `node_modules/` directory.
//
// Keys are kept as build-time canonical path strings — they don't need to
// exist on the runtime filesystem. The loader uses them as opaque
// identifiers; only the source string and the import-edge alias map are
// consulted on the load hot path.
static EMBEDDED_MODULES: Lazy<RwLock<HashMap<String, Arc<String>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));
static EMBEDDED_ALIASES: Lazy<RwLock<HashMap<String, String>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

/// Register a JS module source against its build-time canonical path.
/// Called by `js_register_embedded_module` (the C FFI) at startup from the
/// generated bundle constructor; also usable directly from Rust for tests.
pub fn register_embedded_module(path: &str, source: String) {
    if let Ok(mut map) = EMBEDDED_MODULES.write() {
        map.insert(path.to_string(), Arc::new(source));
    }
}

/// Register a bare specifier → build-time canonical path alias. Lets
/// `resolve()` redirect `import "hono"` to the embedded source without
/// walking `node_modules/`.
pub fn register_embedded_alias(specifier: &str, path: &str) {
    if let Ok(mut map) = EMBEDDED_ALIASES.write() {
        map.insert(specifier.to_string(), path.to_string());
    }
}

/// Look up an embedded source by build-time canonical path. Returns
/// `None` when nothing's registered (the normal dev-build case).
pub fn lookup_embedded_module(path: &str) -> Option<Arc<String>> {
    EMBEDDED_MODULES
        .read()
        .ok()
        .and_then(|map| map.get(path).cloned())
}

/// Look up the build-time canonical path that a bare specifier maps to.
pub fn lookup_embedded_alias(specifier: &str) -> Option<String> {
    EMBEDDED_ALIASES
        .read()
        .ok()
        .and_then(|map| map.get(specifier).cloned())
}

/// C FFI: register an embedded JS module's source. Called from the
/// compile-emitted bundle constructor. Pointers are not retained — the
/// source string is copied into the global map. UTF-8 is assumed.
///
/// # Safety
///
/// `path_ptr` / `source_ptr` must point to valid `len`-byte regions of
/// UTF-8 text. The map takes ownership of an internal copy.
#[no_mangle]
pub unsafe extern "C" fn js_register_embedded_module(
    path_ptr: *const c_char,
    path_len: usize,
    source_ptr: *const c_char,
    source_len: usize,
) {
    if path_ptr.is_null() || source_ptr.is_null() {
        return;
    }
    let path_bytes = std::slice::from_raw_parts(path_ptr as *const u8, path_len);
    let source_bytes = std::slice::from_raw_parts(source_ptr as *const u8, source_len);
    let path = match std::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => return,
    };
    let source = match std::str::from_utf8(source_bytes) {
        Ok(s) => s.to_string(),
        Err(_) => return,
    };
    register_embedded_module(path, source);
}

/// C FFI: register a bare specifier → embedded-path alias. Pointers are
/// not retained.
///
/// # Safety
///
/// Both pointers must reference valid UTF-8 of the given lengths.
#[no_mangle]
pub unsafe extern "C" fn js_register_embedded_alias(
    specifier_ptr: *const c_char,
    specifier_len: usize,
    path_ptr: *const c_char,
    path_len: usize,
) {
    if specifier_ptr.is_null() || path_ptr.is_null() {
        return;
    }
    let spec_bytes = std::slice::from_raw_parts(specifier_ptr as *const u8, specifier_len);
    let path_bytes = std::slice::from_raw_parts(path_ptr as *const u8, path_len);
    let specifier = match std::str::from_utf8(spec_bytes) {
        Ok(s) => s,
        Err(_) => return,
    };
    let path = match std::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => return,
    };
    register_embedded_alias(specifier, path);
}

// Allow C-style null-terminated registration too — slightly nicer codegen
// from the bundle constructor (no manual `strlen`) and matches the
// convention used elsewhere in `perry-jsruntime` FFIs.
#[allow(dead_code)]
unsafe fn cstr_to_str<'a>(ptr: *const c_char) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }
    CStr::from_ptr(ptr).to_str().ok()
}

/// Probe the embedded map with the same extension/index candidates used
/// by `resolve_with_extensions` against the filesystem. Returns the
/// matching build-time canonical path on hit. Used when the file isn't on
/// disk because the binary's been shipped without its `node_modules/`.
pub fn lookup_embedded_path_with_extensions(base: &Path) -> Option<PathBuf> {
    let key = base.to_string_lossy().to_string();
    if lookup_embedded_module(&key).is_some() {
        return Some(PathBuf::from(&key));
    }
    let extensions = [".js", ".mjs", ".cjs", ".json"];
    for ext in extensions {
        let candidate = format!("{}{}", key, ext);
        if lookup_embedded_module(&candidate).is_some() {
            return Some(PathBuf::from(candidate));
        }
    }
    // Try as a directory containing an index file.
    for ext in extensions {
        let candidate = if key.ends_with('/') {
            format!("{}index{}", key, ext)
        } else {
            format!("{}/index{}", key, ext)
        };
        if lookup_embedded_module(&candidate).is_some() {
            return Some(PathBuf::from(candidate));
        }
    }
    None
}

// CJS heuristics regex set. These are tight, hot path on every loaded JS
// module (called once per import); compiling them once amortizes the cost.
pub static EXPORTS_WORD_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\bexports\b").unwrap());
pub static REQUIRE_CALL_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"require\s*\(\s*['"]([^'"]+)['"]\s*\)"#).unwrap());
pub static EXPORTS_ASSIGN_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"exports\.(\w+)\s*=").unwrap());
pub static EXPORT_STAR_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"__exportStar\s*\(\s*require\s*\(\s*['"]([^'"]+)['"]\s*\)\s*,\s*exports\s*\)"#)
        .unwrap()
});
pub static BLOCK_COMMENT_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?s)/\*.*?\*/").unwrap());
pub static LINE_COMMENT_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)//.*$").unwrap());

// Topical sub-modules (extracted from the original modules.rs).
mod builtin_stubs;
mod cjs;
mod loader;

// Explicit named re-exports — preserve the public/inner surface that the
// rest of perry-jsruntime (and external consumers) used when this lived in
// a single file. `#[allow(unused_imports)]` keeps cjs internals (used only
// by the `#[cfg(test)]` block below or within their own sibling) re-exported
// at the parent path without warning in non-test builds.
#[allow(unused_imports)]
pub use builtin_stubs::get_builtin_stub;
#[allow(unused_imports)]
pub use cjs::{
    classify_require_specs, is_commonjs, is_safe_js_binding_name, looks_like_esm,
    parse_package_specifier, resolve_exports, scan_template_literal, strip_js_comments,
    wrap_commonjs,
};
#[allow(unused_imports)]
pub use loader::NodeModuleLoader;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_package_specifier() {
        assert_eq!(
            parse_package_specifier("lodash"),
            ("lodash".to_string(), None)
        );
        assert_eq!(
            parse_package_specifier("lodash/map"),
            ("lodash".to_string(), Some("map".to_string()))
        );
        assert_eq!(
            parse_package_specifier("@types/node"),
            ("@types/node".to_string(), None)
        );
        assert_eq!(
            parse_package_specifier("@babel/core/lib/parser"),
            ("@babel/core".to_string(), Some("lib/parser".to_string()))
        );
    }

    #[test]
    fn test_is_commonjs() {
        assert!(is_commonjs("module.exports = {};"));
        assert!(is_commonjs("exports.foo = 'bar';"));
        assert!(is_commonjs("var base64 = exports;"));
        assert!(is_commonjs(
            "Object.defineProperty(exports, \"__esModule\", { value: true });"
        ));
        assert!(!is_commonjs("export default {};"));
        assert!(!is_commonjs("import foo from 'bar';"));
    }

    #[test]
    fn test_is_commonjs_does_not_wrap_esm_with_exports_text() {
        let code =
            "import fs from 'node:fs';\n/** docs mention exports.foo */\nexport const value = 1;";

        assert!(!is_commonjs(code));
    }

    #[test]
    fn test_wrap_commonjs_skips_default_named_export() {
        let wrapped = wrap_commonjs("exports.default = 1;\nexports.iterate = 2;", None);

        assert!(!wrapped.contains("export const default"));
        assert!(wrapped.contains("export default _cjs;"));
        assert!(wrapped.contains("export const iterate = _cjs.iterate;"));
    }

    #[test]
    fn test_wrap_commonjs_requires_namespace_imports() {
        let wrapped = wrap_commonjs(
            "const uid = require('uid');\nexports.value = uid.uid();",
            None,
        );

        assert!(wrapped.contains("import * as _req_0 from 'uid';"));
        assert!(wrapped.contains("specifier === 'uid'"));
        assert!(wrapped.contains("return __perry_require_namespace(_req_0, _req_0_url);"));
        assert!(wrapped.contains(
            "if (ns.__perry_commonjs === true && ns.default !== undefined) return ns.default;"
        ));
        assert!(wrapped.contains("catch (_)"));
        assert!(wrapped.contains("export const __perry_commonjs = true;"));
    }

    #[test]
    fn test_wrap_commonjs_ignores_require_in_comments() {
        let wrapped = wrap_commonjs(
            "module.exports = roots;\n/** Example only: require('./compiled.js'); */",
            None,
        );

        assert!(!wrapped.contains("import * as _req_0 from './compiled.js';"));
        assert!(!wrapped.contains("specifier === './compiled.js'"));
    }

    #[test]
    fn test_wrap_commonjs_imports_json_with_attribute() {
        let wrapped = wrap_commonjs(
            "exports.version = require('../package.json').version;",
            None,
        );

        assert!(wrapped.contains("import _req_0 from '../package.json' with { type: 'json' };"));
        assert!(wrapped.contains("if (specifier === '../package.json') return _req_0;"));
    }

    #[test]
    fn test_wrap_commonjs_emits_export_star_barrels() {
        let wrapped = wrap_commonjs(
            "const tslib_1 = require('tslib');\ntslib_1.__exportStar(require('./decorators'), exports);",
            None,
        );

        assert!(wrapped.contains("export * from './decorators';"));
    }

    #[test]
    fn test_wrap_commonjs_aliases_reserved_export_names() {
        let wrapped = wrap_commonjs("exports.static = require('serve-static');", None);

        assert!(wrapped.contains("const _cjs_export_static = _cjs.static;"));
        assert!(wrapped.contains("export { _cjs_export_static as static };"));
        assert!(!wrapped.contains("export const static"));
    }

    #[test]
    fn test_classify_top_level_in_array_literal() {
        // Busboy's lib/index.js pattern: `require()` inside a top-level array
        // literal followed by `.filter(function(...) { ... })`. The `function`
        // keyword comes AFTER the requires; the requires themselves are at
        // function-depth 0 and must be classified as top-level. Regression
        // for: `require('./types/multipart')` falling through to
        // __perry_require_lazy with `Cannot find module './types/multipart'`.
        let src = r#"
'use strict';
const { parseContentType } = require('./utils.js');
const TYPES = [
  require('./types/multipart'),
  require('./types/urlencoded'),
].filter(function(typemod) { return typeof typemod.detect === 'function'; });
module.exports = (cfg) => cfg;
"#;
        let stripped = strip_js_comments(src);
        let specs = classify_require_specs(&stripped);
        for (spec, top_level) in &specs {
            assert!(
                *top_level,
                "expected `{}` to be classified top-level, got lazy",
                spec
            );
        }
        assert!(
            specs.iter().any(|(s, _)| s == "./types/multipart"),
            "expected `./types/multipart` to be classified"
        );
    }

    #[test]
    fn test_wrap_template_literal_doesnt_eat_following_requires() {
        // Regression for busboy/lib/index.js shape: a template literal
        // containing `${...}` interpolation that's followed by another
        // top-level `require()`. Pre-fix, the template-literal scanner
        // saw `${` -> jumped out of template mode at the `$`, but then
        // the trailing backtick after the closing `}` re-entered template
        // mode and consumed the rest of the source, swallowing all
        // subsequent requires.
        let src = r#"
'use strict';
const { parseContentType } = require('./utils.js');
function getInstance(cfg) {
  throw new Error(`Unsupported content type: ${cfg.headers['content-type']}`);
}
const TYPES = [
  require('./types/multipart'),
  require('./types/urlencoded'),
];
module.exports = getInstance;
"#;
        let wrapped = wrap_commonjs(src, None);
        for spec in ["./utils.js", "./types/multipart", "./types/urlencoded"] {
            assert!(
                wrapped.contains(&format!("specifier === '{}'", spec)),
                "expected require shim case for `{}`. Wrapped contains:\n{}",
                spec,
                wrapped
                    .lines()
                    .filter(|l| l.contains("specifier ==="))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
    }

    #[test]
    fn test_classify_lazy_inside_method_body() {
        // Readable-stream's _stream_readable.js pattern: lazy require inside
        // a class method / function body. Must be classified as lazy so we
        // don't hoist into a static ESM import (which creates the duplex<->
        // readable cycle that nestjs trips on).
        let src = r#"
'use strict';
function Readable(options) {
  if (!(this instanceof Readable)) return new Readable(options);
}
Readable.prototype._read = function (n) {
  var Duplex = Duplex || require('./_stream_duplex');
};
module.exports = Readable;
"#;
        let stripped = strip_js_comments(src);
        let specs = classify_require_specs(&stripped);
        let entry = specs
            .iter()
            .find(|(s, _)| s == "./_stream_duplex")
            .expect("entry");
        assert!(
            !entry.1,
            "expected `./_stream_duplex` lazy when inside function body"
        );
    }

    #[test]
    fn test_file_url_directory_resolves_to_index() {
        let root = std::env::temp_dir().join(format!(
            "perry-jsruntime-module-test-{}",
            std::process::id()
        ));
        let module_dir = root.join("pkg");
        std::fs::create_dir_all(&module_dir).unwrap();
        let index = module_dir.join("index.js");
        std::fs::write(&index, "export const value = 1;").unwrap();

        let loader = NodeModuleLoader::with_base_dir(root.clone());
        let specifier = format!("file://{}", module_dir.display());
        let resolved = loader
            .resolve_module_path(&specifier, &root.join("entry.js"))
            .unwrap();

        assert_eq!(resolved, index);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_package_main_resolves_to_file() {
        let root = std::env::temp_dir().join(format!(
            "perry-jsruntime-package-test-{}",
            std::process::id()
        ));
        let package_dir = root.join("node_modules").join("pkg");
        std::fs::create_dir_all(&package_dir).unwrap();
        let index = package_dir.join("index.js");
        std::fs::write(&index, "module.exports = {};").unwrap();
        std::fs::write(package_dir.join("package.json"), r#"{"main":"index.js"}"#).unwrap();

        let loader = NodeModuleLoader::with_base_dir(root.clone());
        let resolved = loader
            .resolve_module_path("pkg", &root.join("entry.js"))
            .unwrap();

        assert_eq!(resolved, index);
        let _ = std::fs::remove_dir_all(root);
    }

    /// Issue #755: `fs/promises` and the other Node-builtin subpath aliases
    /// must be recognized by the resolver so they don't fall through to
    /// the node_modules lookup ("Cannot find module 'fs/promises' in
    /// node_modules"). This guards the explicit-match list in
    /// `is_node_builtin` so a future edit can't silently drop them.
    #[test]
    fn test_is_node_builtin_promise_subpaths() {
        for spec in &[
            "fs",
            "fs/promises",
            "node:fs/promises",
            "stream/promises",
            "node:stream/promises",
            "stream/consumers",
            "stream/web",
            "dns/promises",
            "node:dns/promises",
            "timers",
            "timers/promises",
            "node:timers/promises",
            "readline/promises",
            "node:readline/promises",
            "util/types",
            "node:util/types",
            "assert/strict",
            "node:assert/strict",
            "process",
            "node:process",
        ] {
            assert!(
                NodeModuleLoader::is_node_builtin(spec),
                "expected `{}` to be recognized as a Node built-in",
                spec
            );
        }
    }

    /// Stub generator must return a real (non-empty-fallback) module body
    /// for the promise-subpath builtins added in #755. The empty-fallback
    /// branch only `export default {}`, which trips `Cannot read properties
    /// of undefined` at the import site once colyseus reaches for, e.g.,
    /// `fsp.readFile`.
    #[test]
    fn test_get_builtin_stub_promise_subpaths() {
        for name in &[
            "fs/promises",
            "stream/promises",
            "stream/consumers",
            "stream/web",
            "dns/promises",
            "timers/promises",
            "readline/promises",
            "util/types",
            "assert/strict",
        ] {
            let stub = get_builtin_stub(name);
            assert!(
                !stub.contains("Empty stub for unsupported"),
                "expected real stub for `{}`, got empty-fallback body",
                name
            );
            assert!(
                stub.contains("export default"),
                "stub for `{}` missing default export",
                name
            );
        }
    }

    /// Issue #789: the async_hooks builtin must not regress to the old
    /// structural no-op stub. Tracing libraries need lifecycle callbacks and
    /// non-zero AsyncResource ids even when Perry is executing JS through the
    /// embedded V8 runtime.
    #[test]
    fn test_async_hooks_stub_exposes_lifecycle_polyfill() {
        let stub = get_builtin_stub("async_hooks");

        assert!(
            stub.contains("function __perryEmit"),
            "async_hooks stub should emit createHook lifecycle callbacks"
        );
        assert!(
            stub.contains("let __perryNextAsyncId = 1"),
            "async_hooks stub should allocate monotonically increasing ids"
        );
        assert!(
            stub.contains("globalThis.Promise = PerryAsyncHookPromise"),
            "async_hooks stub should hook Promise settlement for promiseResolve"
        );
        assert!(
            !stub.contains("executionAsyncId() { return 0; }")
                && !stub.contains("executionAsyncId() {return 0;}"),
            "async_hooks executionAsyncId must not be the old constant-zero stub"
        );
    }

    /// Regression for the pino smoke `[js_get_export] failed to get namespace`
    /// failure downstream of #903. `thread-stream/index.js` reads
    /// `const MAX_STRING = buffer.constants.MAX_STRING_LENGTH` at top-level
    /// module init, so the V8-fallback `node:buffer` stub MUST expose
    /// `constants.MAX_STRING_LENGTH` (and `MAX_LENGTH`). When it didn't, the
    /// module-init evaluation threw `TypeError: Cannot read properties of
    /// undefined (reading 'MAX_STRING_LENGTH')`, V8 marked the module as
    /// failed-to-eval, and `state.runtime.get_module_namespace(module_id)`
    /// bubbled that error through `js_get_export` for any downstream import
    /// reaching into thread-stream. Values mirror Node 20+'s
    /// `buffer.constants` to keep parity with the real Node module.
    #[test]
    fn test_buffer_stub_exposes_constants() {
        let stub = get_builtin_stub("buffer");
        assert!(
            stub.contains("export const constants"),
            "buffer stub must export `constants` (named) for `buffer.constants.X` reads"
        );
        assert!(
            stub.contains("MAX_STRING_LENGTH: 536870888"),
            "buffer.constants.MAX_STRING_LENGTH must match Node's value (2^29 - 24)"
        );
        assert!(
            stub.contains("MAX_LENGTH: 9007199254740991"),
            "buffer.constants.MAX_LENGTH must match Node's value (Number.MAX_SAFE_INTEGER)"
        );
        // default export must also carry constants so `require('buffer')`
        // unwrap-via-default and the named-namespace path both work.
        assert!(
            stub.contains("export default { Buffer, constants"),
            "buffer stub default export must carry `constants` for CJS-wrap consumers"
        );
    }
}
