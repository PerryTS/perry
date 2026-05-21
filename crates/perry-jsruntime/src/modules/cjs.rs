//! CommonJS detection and wrap-to-ESM transformation.

use super::*;

/// Parse a package specifier into (package_name, subpath)
pub fn parse_package_specifier(specifier: &str) -> (String, Option<String>) {
    if specifier.starts_with('@') {
        // Scoped package: @scope/package or @scope/package/subpath
        let parts: Vec<&str> = specifier.splitn(3, '/').collect();
        if parts.len() >= 2 {
            let package_name = format!("{}/{}", parts[0], parts[1]);
            let subpath = if parts.len() > 2 {
                Some(parts[2].to_string())
            } else {
                None
            };
            return (package_name, subpath);
        }
    } else {
        // Regular package: package or package/subpath
        let parts: Vec<&str> = specifier.splitn(2, '/').collect();
        let package_name = parts[0].to_string();
        let subpath = if parts.len() > 1 {
            Some(parts[1].to_string())
        } else {
            None
        };
        return (package_name, subpath);
    }

    (specifier.to_string(), None)
}

/// Resolve exports field from package.json
pub fn resolve_exports(exports: &serde_json::Value, subpath: &str) -> Option<String> {
    match exports {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Object(map) => {
            // Determine if this is a subpath map (keys start with '.') or conditions map
            let has_subpaths = map.keys().any(|k| k.starts_with('.'));
            if has_subpaths {
                // Subpath map - try matching the subpath
                if let Some(entry) = map.get(subpath) {
                    return resolve_exports(entry, subpath);
                }
                None
            } else {
                // Conditions map - try conditions in priority order
                for condition in ["import", "module", "default", "require", "node"] {
                    if let Some(entry) = map.get(condition) {
                        return resolve_exports(entry, subpath);
                    }
                }
                None
            }
        }
        _ => None,
    }
}

/// Check if code appears to be CommonJS
pub fn is_commonjs(code: &str) -> bool {
    if looks_like_esm(code) {
        return false;
    }

    let code = strip_js_comments(code);

    // Quick heuristics for CommonJS detection
    code.contains("module.exports")
        || code.contains("exports.")
        || EXPORTS_WORD_RE.is_match(&code)
        || code.contains("Object.defineProperty(exports,")
        || (code.contains("require(") && !code.contains("import "))
}

pub fn looks_like_esm(code: &str) -> bool {
    code.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("import ")
            || trimmed.starts_with("export ")
            || trimmed.starts_with("export{")
    })
}

/// Wrap CommonJS code as ESM
pub fn wrap_commonjs(code: &str, self_path: Option<&Path>) -> String {
    // Extract all require() specifiers so we can convert them to ESM imports.
    // We further classify each specifier as "top-level" (appears at function
    // depth 0 in the original source) or "lazy" (inside a function body).
    //
    // Lazy specifiers are normally hoisted into static ESM imports just like
    // top-level ones — most of them resolve to modules that don't form
    // cycles with the current one. The exception is the
    // readable-stream-style pattern where two relative-path siblings both
    // require each other (one statically, one lazily); hoisting the lazy
    // edge would convert it into a full ESM static cycle, and V8 evaluates
    // one peer with the other's bindings still in TDZ, breaking top-level
    // `inherits(...)` calls. We detect that 1-hop cycle by reading the
    // peer's source from disk (we have the importer's path available here)
    // and grep'ing for a require of the importer file back. When detected
    // we drop the static import for that specifier and resolve at call
    // time via the global `__perry_cjs_partial` registry, which is
    // populated as each wrapped module finishes its IIFE.
    let code_without_comments = strip_js_comments(code);
    let require_specs_classified = classify_require_specs(&code_without_comments);
    let is_relative = |s: &str| s.starts_with("./") || s.starts_with("../");
    let creates_cycle = |spec: &str| -> bool {
        if !is_relative(spec) {
            return false;
        }
        let Some(self_path) = self_path else {
            return false;
        };
        let Some(parent) = self_path.parent() else {
            return false;
        };
        let mut peer_path = parent.join(spec);
        // Resolve `.js` / `index.js` similarly to Node's resolver — best
        // effort, since we only need to detect a back-require text match.
        if !peer_path.is_file() {
            let with_js = peer_path.with_extension("js");
            if with_js.is_file() {
                peer_path = with_js;
            } else {
                let as_index = peer_path.join("index.js");
                if as_index.is_file() {
                    peer_path = as_index;
                }
            }
        }
        let peer_src = match std::fs::read_to_string(&peer_path) {
            Ok(s) => s,
            Err(_) => return false,
        };
        // Build a set of relative-path tokens that would refer back to
        // self_path from peer_path. We compute a single canonical filename
        // form: the importer's stem (e.g. `_stream_readable`) since real
        // siblings reference each other by `./_stream_readable` or
        // `./_stream_readable.js`. Looking at the filename stem covers
        // ambiguity around `.js` extension and sibling directories.
        let stem = match self_path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => return false,
        };
        // Search the peer's source (comment-stripped) for any
        // `require(...stem...)` form. We use the stripped form so commented
        // requires don't falsely indicate a cycle.
        let peer_stripped = strip_js_comments(&peer_src);
        for cap in REQUIRE_CALL_RE.captures_iter(&peer_stripped) {
            if let Some(spec_match) = cap.get(1) {
                let s = spec_match.as_str();
                if !is_relative(s) {
                    continue;
                }
                // Compare basename of s against stem.
                let s_stem = std::path::Path::new(s)
                    .file_stem()
                    .and_then(|x| x.to_str())
                    .unwrap_or("");
                if s_stem == stem {
                    return true;
                }
            }
        }
        false
    };
    let require_specs: Vec<String> = require_specs_classified
        .iter()
        .filter(|(spec, top_level)| *top_level || !creates_cycle(spec))
        .map(|(s, _)| s.clone())
        .collect();
    let lazy_only_specs: Vec<String> = require_specs_classified
        .iter()
        .filter(|(spec, top_level)| {
            !*top_level && creates_cycle(spec) && !require_specs.contains(spec)
        })
        .map(|(s, _)| s.clone())
        .collect();

    // Generate ESM namespace imports for each require() specifier. `require()`
    // unwraps wrapped CJS default exports when safe, but falls back to the
    // namespace if a circular module's default binding is still in TDZ.
    let imports = require_specs
        .iter()
        .enumerate()
        .map(|(i, spec)| {
            if spec.ends_with(".json") {
                format!("import _req_{} from '{}' with {{ type: 'json' }};", i, spec)
            } else {
                format!("import * as _req_{} from '{}';", i, spec)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Per-specifier URL constants. We resolve each non-JSON spec against the
    // current module's URL at wrap-time-emitted top-level. The require shim
    // uses these as keys into globalThis.__perry_cjs_partial — a registry of
    // partially-loaded CJS module.exports objects. When a static import cycle
    // hands us a still-in-TDZ namespace (V8 picked an evaluation order where
    // the importee hadn't completed yet), we fall back to the importee's live
    // `module.exports` from this registry. This mirrors Node's CJS partial-
    // cycle semantics: `var X = require('./cycle-peer')` returns whatever the
    // peer has assigned to `module.exports` so far. Closes the readable-stream
    // _stream_duplex.js -> _stream_readable.js cycle that nestjs triggers via
    // express's `send` dependency.
    let req_url_decls = require_specs
        .iter()
        .enumerate()
        .filter_map(|(i, spec)| {
            if spec.ends_with(".json") {
                None
            } else {
                let escaped_spec = spec.replace('\\', "\\\\").replace('\'', "\\'");
                Some(format!(
                    "var _req_{idx}_url; try {{ _req_{idx}_url = new URL('{spec}', import.meta.url).href; }} catch (_) {{ _req_{idx}_url = '{spec}'; }}",
                    idx = i,
                    spec = escaped_spec,
                ))
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Generate require() lookup cases. Each non-JSON case first checks
    // whether the imported namespace carries the `perry-missing:` stub
    // marker — if so we throw a Node-compatible MODULE_NOT_FOUND error
    // INSIDE the require() function body so a wrapping try/catch (the
    // optional-dependency pattern used by debug, etc.) can catch it.
    let mut require_cases_vec: Vec<String> = require_specs
        .iter()
        .enumerate()
        .map(|(i, spec)| {
            let escaped_spec = spec.replace('\\', "\\\\").replace('\'', "\\'");
            if spec.ends_with(".json") {
                format!(
                    "        if (specifier === '{}') return _req_{};",
                    escaped_spec, i
                )
            } else {
                format!(
                    "        if (specifier === '{spec}') {{\n\
                     \x20           if (_req_{idx} && _req_{idx}.__perry_missing === true) {{\n\
                     \x20               var __err = new Error(\"Cannot find module '\" + _req_{idx}.__perry_specifier + \"'\");\n\
                     \x20               __err.code = 'MODULE_NOT_FOUND';\n\
                     \x20               throw __err;\n\
                     \x20           }}\n\
                     \x20           return __perry_require_namespace(_req_{idx}, _req_{idx}_url);\n\
                     \x20       }}",
                    spec = escaped_spec,
                    idx = i
                )
            }
        })
        .collect();

    // Lazy-only require cases. These specs only appear inside function
    // bodies — i.e., they're called at runtime (after the module graph has
    // finished loading), never at IIFE top-level. We skip the static ESM
    // import to avoid creating eager cycles, and instead resolve at call
    // time via the global `__perry_cjs_partial` registry. By the time a
    // lazy `require()` fires, some statically-imported peer has already
    // loaded the target module and registered it.
    for spec in &lazy_only_specs {
        let escaped_spec = spec.replace('\\', "\\\\").replace('\'', "\\'");
        require_cases_vec.push(format!(
            "        if (specifier === '{spec}') return __perry_require_lazy('{spec}');",
            spec = escaped_spec,
        ));
    }
    let require_cases = require_cases_vec.join("\n");

    // Extract exported names from CommonJS code to properly re-export them
    let mut named_exports = Vec::new();
    let mut export_star_specs = Vec::new();

    // Find exports.X = assignments
    for cap in EXPORTS_ASSIGN_RE.captures_iter(code) {
        if let Some(name) = cap.get(1) {
            let name = name.as_str();
            if name != "__esModule"
                && name != "default"
                && !named_exports.contains(&name.to_string())
            {
                named_exports.push(name.to_string());
            }
        }
    }

    // Find tslib __exportStar(require("..."), exports) barrel re-exports.
    for cap in EXPORT_STAR_RE.captures_iter(code) {
        if let Some(spec) = cap.get(1) {
            let spec = spec.as_str().to_string();
            if !export_star_specs.contains(&spec) {
                export_star_specs.push(spec);
            }
        }
    }

    // Use a more sophisticated approach: wrap the code in an IIFE and then export
    // the results using dynamic re-exports
    let named_export_decls = if named_exports.is_empty() {
        String::new()
    } else {
        // Create individual export statements that reference the _cjs object
        named_exports
            .iter()
            .map(|n| {
                if is_safe_js_binding_name(n) {
                    format!("export const {} = _cjs.{};", n, n)
                } else {
                    let alias = format!("_cjs_export_{}", n);
                    format!(
                        "const {} = _cjs.{};\nexport {{ {} as {} }};",
                        alias, n, alias, n
                    )
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let export_star_decls = if export_star_specs.is_empty() {
        String::new()
    } else {
        export_star_specs
            .iter()
            .map(|spec| format!("export * from '{}';", spec))
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        r#"{imports}
{req_url_decls}
var __perry_self_url; try {{ __perry_self_url = import.meta.url; }} catch (_) {{ __perry_self_url = ''; }}
var __perry_cjs_partial = (globalThis.__perry_cjs_partial = globalThis.__perry_cjs_partial || new Map());
var __perry_self_module = {{ exports: {{}} }};
if (__perry_self_url) {{
    __perry_cjs_partial.set(__perry_self_url, __perry_self_module);
    // Also register the module under its package name (e.g.
    // `@nestjs/platform-express`) if the URL is under node_modules. NestJS
    // and other frameworks call `require(packageName)` with a runtime
    // string argument; we can't hoist a static import for that, so we
    // resolve it via this auxiliary index in `__perry_require_lazy`.
    try {{
        // Use the last occurrence of `/node_modules/` so nested packages
        // (foo/node_modules/bar) get attributed to the inner one.
        var __nm = __perry_self_url.lastIndexOf('/node_modules/');
        if (__nm !== -1) {{
            var __after = __perry_self_url.slice(__nm + '/node_modules/'.length);
            var __segs = __after.split('/');
            var __pkg, __restStart;
            if (__segs.length > 0 && __segs[0].charAt(0) === '@' && __segs.length > 1) {{
                __pkg = __segs[0] + '/' + __segs[1];
                __restStart = 2;
            }} else if (__segs.length > 0) {{
                __pkg = __segs[0];
                __restStart = 1;
            }} else {{
                __pkg = '';
                __restStart = 0;
            }}
            var __rest = __segs.slice(__restStart).join('/');
            // Register under the package name only for typical package-entry
            // file shapes (index.js / dist/index.js / lib/index.js). This
            // avoids deep-internal modules clobbering the package-root entry.
            // We deliberately use `set()` unconditionally so the last entry
            // wins; in practice only one of these candidate files exists per
            // package, so collisions are rare.
            if (__pkg !== '' && (__rest === '' || __rest === 'index.js' || __rest === 'dist/index.js' || __rest === 'lib/index.js' || __rest === 'src/index.js')) {{
                __perry_cjs_partial.set(__pkg, __perry_self_module);
            }}
        }}
    }} catch (_) {{}}
}}
const _cjs = (function() {{
    var module = __perry_self_module;
    var exports = module.exports;
    function __perry_require_namespace(ns, nsUrl) {{
        // Detect ESM-cycle TDZ: a still-loading namespace will throw
        // "Cannot access '<binding>' before initialization" on any property
        // read. In that case, fall back to the importee's partial
        // `module.exports` from the global CJS registry — mirroring Node's
        // CJS partial-cycle semantics where `require()` mid-load returns
        // whatever `module.exports` has been assigned so far. Without this,
        // readable-stream's `_stream_duplex.js`/`_stream_readable.js` cycle
        // (entered transitively via nestjs -> @nestjs/platform-express ->
        // express -> send) sees `Readable.prototype === undefined` and the
        // whole module graph dies at top-level `inherits(Duplex, Readable)`.
        var __tdz = false;
        try {{
            if (ns.__perry_commonjs === true && ns.default !== undefined) return ns.default;
        }} catch (e) {{
            // TDZ surfaces as a ReferenceError with a "before initialization"
            // message. Other shapes (Proxy traps, etc) we treat as opaque.
            if (e && typeof e.message === 'string' && e.message.indexOf('before initialization') !== -1) {{
                __tdz = true;
            }}
        }}
        if (__tdz && nsUrl && __perry_cjs_partial.has(nsUrl)) {{
            var __peer = __perry_cjs_partial.get(nsUrl);
            if (__peer && __peer.exports !== undefined) return __peer.exports;
        }}
        // ESM module-namespace objects (from `import * as ns`) have a null
        // prototype, so `ns.hasOwnProperty(...)` throws "is not a function".
        // safer-buffer's `for (key in buffer) if (!buffer.hasOwnProperty(key))`
        // probe (loaded indirectly by express via body-parser) and similar
        // legacy CommonJS code expects the value returned from `require()` to
        // inherit from Object.prototype. Copy enumerable own props into a
        // plain object so Object.prototype.* (hasOwnProperty,
        // propertyIsEnumerable, toString, valueOf) is reachable.
        try {{
            if (ns && typeof ns === 'object' && Object.getPrototypeOf(ns) === null) {{
                var __o = {{}};
                for (var __k in ns) __o[__k] = ns[__k];
                return __o;
            }}
        }} catch (_) {{
        }}
        return ns;
    }}
    // Resolve a lazy-only require: the specifier was found inside a function
    // body in the source so we deliberately skipped its static ESM import to
    // avoid forcing an eager cycle. By the time this runs the target module
    // has been loaded by some other statically-imported caller and has
    // registered its `module.exports` in `__perry_cjs_partial`. We look up
    // by absolute URL (resolved against the current module's URL).
    function __perry_require_lazy(specifier) {{
        // 1) Bare-specifier package lookup (e.g. `@nestjs/platform-express`).
        //    Registered side-channel keyed by the package name.
        if (__perry_cjs_partial.has(specifier)) {{
            var __pkgPeer = __perry_cjs_partial.get(specifier);
            if (__pkgPeer && __pkgPeer.exports !== undefined) return __pkgPeer.exports;
        }}
        // 2) Resolve relative path against this module's URL.
        var url;
        try {{ url = new URL(specifier, __perry_self_url).href; }} catch (_) {{ url = specifier; }}
        if (__perry_cjs_partial.has(url)) {{
            var __peer = __perry_cjs_partial.get(url);
            if (__peer && __peer.exports !== undefined) return __peer.exports;
        }}
        var __err = new Error("Cannot find module '" + specifier + "'");
        __err.code = 'MODULE_NOT_FOUND';
        throw __err;
    }}
    function require(specifier) {{
{require_cases}
        // Fall through: the specifier wasn't a string literal in this
        // module's source so we couldn't hoist a static import for it.
        // (Common case: NestJS's `loadAdapter` does `require(defaultPlatform)`
        // where defaultPlatform is a runtime arg.) Best-effort: if some
        // other module has already loaded this specifier and registered it
        // by URL, return that. Otherwise raise MODULE_NOT_FOUND so caller
        // try/catch can react (NestJS's loadAdapter wraps in try/catch).
        return __perry_require_lazy(specifier);
    }}

    {code}

    return module.exports;
}})();

export default _cjs;
export const __perry_commonjs = true;
{named_export_decls}
{export_star_decls}
"#,
        imports = imports,
        req_url_decls = req_url_decls,
        require_cases = require_cases,
        code = code,
        named_export_decls = named_export_decls,
        export_star_decls = export_star_decls,
    )
}

/// Skip past a JS template literal starting at `bytes[start]` (the backtick).
/// Returns the byte index immediately after the closing backtick (or `n` if
/// the source ends unterminated). Handles `${...}` interpolation with full
/// brace balancing, nested strings, and nested template literals.
pub fn scan_template_literal(bytes: &[u8], start: usize) -> usize {
    let n = bytes.len();
    debug_assert!(start < n && bytes[start] == b'`');
    let mut i = start + 1;
    while i < n {
        let b = bytes[i];
        if b == b'\\' && i + 1 < n {
            i += 2;
            continue;
        }
        if b == b'`' {
            return i + 1;
        }
        if b == b'$' && i + 1 < n && bytes[i + 1] == b'{' {
            // Enter interpolation: balance braces, skipping nested
            // strings/templates.
            i += 2; // past `${`
            let mut depth: i32 = 1;
            while i < n && depth > 0 {
                let c = bytes[i];
                match c {
                    b'\\' if i + 1 < n => i += 2,
                    b'\'' | b'"' => {
                        let q = c;
                        i += 1;
                        while i < n {
                            let bb = bytes[i];
                            if bb == b'\\' && i + 1 < n {
                                i += 2;
                                continue;
                            }
                            if bb == q || bb == b'\n' {
                                if bb == q {
                                    i += 1;
                                }
                                break;
                            }
                            i += 1;
                        }
                    }
                    b'`' => {
                        i = scan_template_literal(bytes, i);
                    }
                    b'{' => {
                        depth += 1;
                        i += 1;
                    }
                    b'}' => {
                        depth -= 1;
                        i += 1;
                    }
                    _ => i += 1,
                }
            }
            continue;
        }
        i += 1;
    }
    i
}

pub fn strip_js_comments(code: &str) -> String {
    let without_blocks = BLOCK_COMMENT_RE.replace_all(code, "");
    LINE_COMMENT_RE
        .replace_all(&without_blocks, "")
        .into_owned()
}

/// Classify every `require('...')` call in `code` (comments already stripped)
/// as top-level (zero function nesting) or lazy (inside a function/method/
/// arrow body). A `require` inside `if`/`while`/`try`/etc. at module top
/// level is still considered top-level — the typical
/// `if (...) module.exports = require('./browser.js')` pattern (debug,
/// readable-stream/.../stream-browser.js, etc.) wants eager evaluation so
/// `module.exports` ends up populated before the wrap returns.
///
/// Returns a Vec of `(spec, is_top_level)` in source order; if the same spec
/// appears both top-level and lazy we keep only the top-level entry so the
/// emitted ESM import remains eager.
///
/// The scanner is intentionally simple: it skips string/template/regex
/// literals and tracks function-introducing tokens (`function`, `=>`) to
/// bump a function-depth counter that's paired with brace tracking. It
/// does not parse JS, so edge cases (`}` inside a JSX expression, generator
/// methods, async arrow with destructuring etc.) are not handled, but
/// those don't appear in the CJS modules that hit this code path.
pub fn classify_require_specs(code: &str) -> Vec<(String, bool)> {
    let bytes = code.as_bytes();
    let mut i = 0usize;
    let n = bytes.len();
    // Stack of brace-frames. Each frame records whether opening this `{`
    // entered a function body (true) or a block/object literal (false).
    // A require is "top-level" if no frame in the stack is a function.
    let mut frame_is_function: Vec<bool> = Vec::new();
    // When the next `{` is opened, was it preceded by a function-keyword
    // or an arrow `=>`? Set true on `function`/`=>`, consumed on `{`.
    let mut next_brace_is_function: bool = false;
    // Track class method headers: after `class X {`, subsequent `methodName(...)
    // { ... }` blocks are method bodies. We approximate by setting
    // next_brace_is_function whenever we see `)` followed by `{` while inside
    // a `class { ... }` frame. To detect "inside class body" we tag class
    // frames as function-ish too (any method body counts as a function).
    let mut frame_is_class: Vec<bool> = Vec::new();
    let mut next_brace_is_class: bool = false;
    let mut last_paren_close: bool = false;
    // (spec -> first seen depth=0?). Preserve insertion order.
    let mut seen: Vec<(String, bool)> = Vec::new();

    // Helper: does byte at i look like a regex-start vs division operator?
    // We approximate: a regex follows certain tokens (operators, keywords,
    // start-of-input). Tracking "last non-whitespace token" handles most
    // cases. We keep this conservative — false negatives mean we treat a
    // regex body as code, which only affects classification, not the
    // emitted require list (REQUIRE_CALL_RE later runs over the raw code).
    let mut last_significant: u8 = b'\n';

    while i < n {
        let c = bytes[i];
        match c {
            b'"' | b'\'' => {
                // String literal
                let quote = c;
                i += 1;
                while i < n {
                    let b = bytes[i];
                    if b == b'\\' && i + 1 < n {
                        i += 2;
                        continue;
                    }
                    if b == quote {
                        i += 1;
                        break;
                    }
                    if b == b'\n' {
                        // Unterminated; bail out of this string scan
                        break;
                    }
                    i += 1;
                }
                last_significant = b'"';
            }
            b'`' => {
                // Template literal with nested `${expr}` interpolation.
                // We need to track expression nesting so that the closing
                // backtick is recognized correctly after each interpolation
                // returns. Strategy: push the current scanner mode and enter
                // template-text mode. When `${` is seen, switch to "code"
                // mode with a brace counter; the matching `}` pops back to
                // template-text mode.
                //
                // We implement this inline (a small recursive descent on
                // the stream pointer) instead of touching the outer
                // mode/frame stacks, since template expressions don't
                // contribute to top-level/function classification.
                i = scan_template_literal(bytes, i);
                last_significant = b'`';
            }
            b'/' => {
                // Could be a comment (already stripped), division, or regex.
                let prev = last_significant;
                let regex_context = matches!(
                    prev,
                    b'(' | b','
                        | b'='
                        | b'!'
                        | b'&'
                        | b'|'
                        | b'?'
                        | b'{'
                        | b'}'
                        | b';'
                        | b':'
                        | b'+'
                        | b'-'
                        | b'*'
                        | b'%'
                        | b'^'
                        | b'~'
                        | b'<'
                        | b'>'
                        | b'['
                        | b'\n'
                );
                if regex_context && i + 1 < n {
                    // Skip regex body
                    i += 1;
                    while i < n {
                        let b = bytes[i];
                        if b == b'\\' && i + 1 < n {
                            i += 2;
                            continue;
                        }
                        if b == b'[' {
                            // Character class — skip ']' but not '/'
                            i += 1;
                            while i < n && bytes[i] != b']' {
                                if bytes[i] == b'\\' && i + 1 < n {
                                    i += 2;
                                    continue;
                                }
                                if bytes[i] == b'\n' {
                                    break;
                                }
                                i += 1;
                            }
                            if i < n {
                                i += 1;
                            }
                            continue;
                        }
                        if b == b'/' {
                            i += 1;
                            // Skip flags
                            while i < n && (bytes[i].is_ascii_alphabetic() || bytes[i] == b'$') {
                                i += 1;
                            }
                            break;
                        }
                        if b == b'\n' {
                            break;
                        }
                        i += 1;
                    }
                    last_significant = b'/';
                } else {
                    i += 1;
                    last_significant = b'/';
                }
            }
            b'{' => {
                // Inside a class body, a `{` following `)` is a method body.
                let in_class = matches!(frame_is_class.last(), Some(true));
                let is_method = in_class && last_paren_close;
                let is_function = next_brace_is_function || is_method;
                frame_is_function.push(is_function);
                frame_is_class.push(next_brace_is_class);
                next_brace_is_function = false;
                next_brace_is_class = false;
                last_paren_close = false;
                i += 1;
                last_significant = b'{';
            }
            b'}' => {
                frame_is_function.pop();
                frame_is_class.pop();
                i += 1;
                last_significant = b'}';
                last_paren_close = false;
            }
            b'(' => {
                i += 1;
                last_significant = b'(';
                last_paren_close = false;
            }
            b')' => {
                i += 1;
                last_significant = b')';
                last_paren_close = true;
            }
            b'=' if i + 1 < n && bytes[i + 1] == b'>' => {
                // Arrow function head — the body that follows (either an
                // expression or a `{ ... }` block) is a function context.
                // If the body is `{`, the next `{` opens a function frame.
                // If it's an expression body, function nesting still
                // applies for any nested require inside it — we conservatively
                // bump function depth via a fake frame, but unboxing that
                // for `=> expr` (no braces) is too hairy without a parser.
                // Simplest: treat `=>` as "next brace is function". For
                // expression-bodied arrows we still under-classify, which
                // is the prior behavior anyway.
                next_brace_is_function = true;
                i += 2;
                last_significant = b'>';
                last_paren_close = false;
            }
            b'f' if i + 8 <= n && &bytes[i..i + 8] == b"function" => {
                let prev_is_ident = i > 0
                    && (bytes[i - 1].is_ascii_alphanumeric()
                        || bytes[i - 1] == b'_'
                        || bytes[i - 1] == b'$');
                let next_is_ident_break = i + 8 < n
                    && !(bytes[i + 8].is_ascii_alphanumeric()
                        || bytes[i + 8] == b'_'
                        || bytes[i + 8] == b'$');
                if !prev_is_ident && next_is_ident_break {
                    next_brace_is_function = true;
                    i += 8;
                    last_significant = b'n';
                    last_paren_close = false;
                } else {
                    i += 1;
                    last_significant = b'f';
                }
            }
            b'c' if i + 5 <= n && &bytes[i..i + 5] == b"class" => {
                let prev_is_ident = i > 0
                    && (bytes[i - 1].is_ascii_alphanumeric()
                        || bytes[i - 1] == b'_'
                        || bytes[i - 1] == b'$');
                let next_is_ident_break = i + 5 < n
                    && !(bytes[i + 5].is_ascii_alphanumeric()
                        || bytes[i + 5] == b'_'
                        || bytes[i + 5] == b'$');
                if !prev_is_ident && next_is_ident_break {
                    next_brace_is_class = true;
                    i += 5;
                    last_significant = b's';
                    last_paren_close = false;
                } else {
                    i += 1;
                    last_significant = b'c';
                }
            }
            b'r' if i + 7 <= n && &bytes[i..i + 7] == b"require" => {
                // Ensure word boundary before
                let prev_is_ident = i > 0
                    && (bytes[i - 1].is_ascii_alphanumeric()
                        || bytes[i - 1] == b'_'
                        || bytes[i - 1] == b'$');
                if prev_is_ident {
                    i += 1;
                    continue;
                }
                // Skip "require"
                let mut j = i + 7;
                // Skip whitespace
                while j < n && (bytes[j] == b' ' || bytes[j] == b'\t' || bytes[j] == b'\n') {
                    j += 1;
                }
                if j >= n || bytes[j] != b'(' {
                    i += 1;
                    continue;
                }
                j += 1;
                while j < n && (bytes[j] == b' ' || bytes[j] == b'\t' || bytes[j] == b'\n') {
                    j += 1;
                }
                if j >= n {
                    i += 1;
                    continue;
                }
                let q = bytes[j];
                if q != b'"' && q != b'\'' {
                    i += 1;
                    continue;
                }
                j += 1;
                let spec_start = j;
                while j < n && bytes[j] != q {
                    if bytes[j] == b'\\' && j + 1 < n {
                        j += 2;
                        continue;
                    }
                    if bytes[j] == b'\n' {
                        break;
                    }
                    j += 1;
                }
                if j >= n || bytes[j] != q {
                    i += 1;
                    continue;
                }
                let spec = match std::str::from_utf8(&bytes[spec_start..j]) {
                    Ok(s) => s.to_string(),
                    Err(_) => {
                        i += 1;
                        continue;
                    }
                };
                let is_top_level = !frame_is_function.iter().any(|&f| f);
                // Insert / upgrade entry
                if let Some(pos) = seen.iter().position(|(s, _)| s == &spec) {
                    if is_top_level && !seen[pos].1 {
                        seen[pos].1 = true;
                    }
                } else {
                    seen.push((spec, is_top_level));
                }
                // Advance past the closing quote and ')'
                i = j + 1;
                while i < n && (bytes[i] == b' ' || bytes[i] == b'\t' || bytes[i] == b'\n') {
                    i += 1;
                }
                if i < n && bytes[i] == b')' {
                    i += 1;
                }
                last_significant = b')';
            }
            b' ' | b'\t' | b'\r' | b'\n' => {
                i += 1;
            }
            _ => {
                last_significant = c;
                last_paren_close = false;
                i += 1;
            }
        }
    }

    seen
}

pub fn is_safe_js_binding_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut chars = name.chars();
    let first = chars.next().unwrap();
    if !(first == '_' || first == '$' || first.is_ascii_alphabetic()) {
        return false;
    }
    if !chars.all(|c| c == '_' || c == '$' || c.is_ascii_alphanumeric()) {
        return false;
    }
    !matches!(
        name,
        "await"
            | "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "debugger"
            | "default"
            | "delete"
            | "do"
            | "else"
            | "export"
            | "extends"
            | "finally"
            | "for"
            | "function"
            | "if"
            | "import"
            | "in"
            | "instanceof"
            | "new"
            | "return"
            | "static"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "try"
            | "typeof"
            | "var"
            | "void"
            | "while"
            | "with"
            | "yield"
    )
}
