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
use std::path::{Path, PathBuf};

// CJS heuristics regex set. These are tight, hot path on every loaded JS
// module (called once per import); compiling them once amortizes the cost.
static EXPORTS_WORD_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\bexports\b").unwrap());
static REQUIRE_CALL_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"require\s*\(\s*['"]([^'"]+)['"]\s*\)"#).unwrap());
static EXPORTS_ASSIGN_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"exports\.(\w+)\s*=").unwrap());
static EXPORT_STAR_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"__exportStar\s*\(\s*require\s*\(\s*['"]([^'"]+)['"]\s*\)\s*,\s*exports\s*\)"#)
        .unwrap()
});
static BLOCK_COMMENT_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?s)/\*.*?\*/").unwrap());
static LINE_COMMENT_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)//.*$").unwrap());

/// Node.js-compatible module loader
pub struct NodeModuleLoader {
    /// Base directory for module resolution
    base_dir: PathBuf,
}

impl NodeModuleLoader {
    pub fn new() -> Self {
        Self {
            base_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }

    pub fn with_base_dir(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    /// Check if a resolved path has a browser field mapping in its package.json
    /// Returns the browser-mapped path if found, None otherwise.
    fn check_browser_field(&self, resolved: &Path) -> Option<PathBuf> {
        // Canonicalize the resolved path to remove ./ and ../ components
        let resolved = std::fs::canonicalize(resolved).ok()?;
        // Walk up from the resolved path to find a package.json with a browser field
        let mut dir = resolved.parent()?;
        loop {
            let pkg_json = dir.join("package.json");
            if pkg_json.exists() {
                let content = std::fs::read_to_string(&pkg_json).ok()?;
                let pkg: serde_json::Value = serde_json::from_str(&content).ok()?;
                if let Some(browser) = pkg.get("browser") {
                    if let Some(browser_map) = browser.as_object() {
                        // Browser field keys are relative to the package root (prefixed with "./")
                        let relative = resolved.strip_prefix(dir).ok()?;
                        let relative_str = format!("./{}", relative.to_string_lossy());
                        if let Some(replacement) = browser_map.get(&relative_str) {
                            if let Some(replacement_str) = replacement.as_str() {
                                let browser_path =
                                    dir.join(replacement_str.trim_start_matches("./"));
                                if browser_path.exists() {
                                    return Some(browser_path);
                                }
                            }
                        }
                    }
                }
                return None; // Found package.json but no browser mapping
            }
            dir = dir.parent()?;
        }
    }

    /// Resolve a module specifier to an absolute path
    fn resolve_module_path(&self, specifier: &str, referrer: &Path) -> Result<PathBuf> {
        // Handle file:// URLs
        if specifier.starts_with("file://") {
            let path_str = specifier.strip_prefix("file://").unwrap_or(specifier);
            let path = PathBuf::from(path_str);
            if path.exists() && path.is_file() {
                return Ok(path);
            }
            return self.resolve_with_extensions(path);
        }

        // Handle relative imports (./ or ../)
        if specifier.starts_with("./") || specifier.starts_with("../") {
            let referrer_dir = referrer.parent().unwrap_or(&self.base_dir);
            let resolved = referrer_dir.join(specifier);
            let resolved = self.resolve_with_extensions(resolved)?;
            // Check browser field mapping (e.g., ethers geturl.js -> geturl-browser.js)
            if let Some(browser_path) = self.check_browser_field(&resolved) {
                return Ok(browser_path);
            }
            return Ok(resolved);
        }

        // Handle absolute paths
        if specifier.starts_with('/') {
            let resolved = PathBuf::from(specifier);
            return self.resolve_with_extensions(resolved);
        }

        // Handle node_modules
        self.resolve_from_node_modules(specifier, referrer)
    }

    /// Try resolving a path with common extensions
    fn resolve_with_extensions(&self, base: PathBuf) -> Result<PathBuf> {
        // If it already exists as-is
        if base.exists() && base.is_file() {
            return Ok(base);
        }

        // Try with extensions
        let extensions = [".js", ".mjs", ".cjs", ".json"];
        for ext in extensions {
            let with_ext = base.with_extension(ext.trim_start_matches('.'));
            if with_ext.exists() {
                return Ok(with_ext);
            }

            // Also try adding extension to full path (for paths like ./foo.js)
            let path_str = base.to_string_lossy();
            let with_ext = PathBuf::from(format!("{}{}", path_str, ext));
            if with_ext.exists() {
                return Ok(with_ext);
            }
        }

        // Try index files in directory
        if base.is_dir() {
            for ext in extensions {
                let index = base.join(format!("index{}", ext));
                if index.exists() {
                    return Ok(index);
                }
            }
        }

        Err(anyhow!("Cannot resolve module: {:?}", base))
    }

    /// Check if a specifier is a Node.js built-in module
    ///
    /// Issue #755: `fs/promises` (and the other `*/promises` subpath aliases
    /// that Node exposes as standalone builtins — `stream/promises`,
    /// `dns/promises`, `timers/promises`, `readline/promises`) must be
    /// recognized here, otherwise the resolver falls through to
    /// `resolve_from_node_modules` and fails with
    /// "Cannot find module 'fs/promises' in node_modules". Real packages
    /// (colyseus, etc.) `import` these directly. The base `is_node_builtin`
    /// uses exact string matches so each subpath needs its own entry.
    fn is_node_builtin(specifier: &str) -> bool {
        let specifier = specifier.trim_end_matches('/');
        matches!(
            specifier,
            "net"
                | "tls"
                | "http"
                | "http2"
                | "https"
                | "fs"
                | "fs/promises"
                | "path"
                | "os"
                | "crypto"
                | "stream"
                | "stream/promises"
                | "stream/consumers"
                | "stream/web"
                | "buffer"
                | "util"
                | "util/types"
                | "events"
                | "assert"
                | "assert/strict"
                | "child_process"
                | "dns"
                | "dns/promises"
                | "dgram"
                | "url"
                | "querystring"
                | "string_decoder"
                | "zlib"
                | "readline"
                | "readline/promises"
                | "repl"
                | "timers"
                | "timers/promises"
                | "tty"
                | "vm"
                | "worker_threads"
                | "cluster"
                | "async_hooks"
                | "perf_hooks"
                | "trace_events"
                | "inspector"
                | "v8"
                | "process"
                | "node:net"
                | "node:tls"
                | "node:http"
                | "node:http2"
                | "node:https"
                | "node:fs"
                | "node:fs/promises"
                | "node:path"
                | "node:os"
                | "node:crypto"
                | "node:stream"
                | "node:stream/promises"
                | "node:stream/consumers"
                | "node:stream/web"
                | "node:buffer"
                | "node:util"
                | "node:util/types"
                | "node:events"
                | "node:assert"
                | "node:assert/strict"
                | "node:child_process"
                | "node:dns"
                | "node:dns/promises"
                | "node:dgram"
                | "node:url"
                | "node:querystring"
                | "node:string_decoder"
                | "node:zlib"
                | "node:readline"
                | "node:readline/promises"
                | "node:repl"
                | "node:timers"
                | "node:timers/promises"
                | "node:tty"
                | "node:vm"
                | "node:worker_threads"
                | "node:cluster"
                | "node:async_hooks"
                | "node:perf_hooks"
                | "node:trace_events"
                | "node:inspector"
                | "node:v8"
                | "node:process"
        )
    }

    /// Resolve a module from node_modules
    fn resolve_from_node_modules(&self, specifier: &str, referrer: &Path) -> Result<PathBuf> {
        let mut current_dir = referrer.parent().unwrap_or(&self.base_dir).to_path_buf();

        // Parse package name and subpath
        let (package_name, subpath) = parse_package_specifier(specifier);

        // Walk up the directory tree looking for node_modules
        loop {
            let node_modules = current_dir.join("node_modules").join(&package_name);

            if node_modules.exists() {
                // Check for package.json
                let package_json = node_modules.join("package.json");
                if package_json.exists() {
                    if let Ok(entry_point) =
                        self.resolve_package_entry(&node_modules, &package_json, subpath.as_deref())
                    {
                        return Ok(entry_point);
                    }
                }

                // Fall back to index.js
                let index = node_modules.join("index.js");
                if index.exists() {
                    return Ok(index);
                }
            }

            // Move up to parent directory
            if let Some(parent) = current_dir.parent() {
                current_dir = parent.to_path_buf();
            } else {
                break;
            }
        }

        Err(anyhow!(
            "Cannot find module '{}' in node_modules",
            specifier
        ))
    }

    /// Resolve the entry point from package.json
    fn resolve_package_entry(
        &self,
        package_dir: &Path,
        package_json: &Path,
        subpath: Option<&str>,
    ) -> Result<PathBuf> {
        let content = std::fs::read_to_string(package_json)?;
        let pkg: serde_json::Value = serde_json::from_str(&content)?;

        // If there's a subpath, first check "exports" field, then fall back to direct resolution
        if let Some(sub) = subpath {
            // Check "exports" field for subpath (e.g., "./sha3" in @noble/hashes)
            if let Some(exports) = pkg.get("exports") {
                let export_key = format!("./{}", sub);
                if let Some(entry) = resolve_exports(exports, &export_key) {
                    let entry_path = package_dir.join(entry);
                    if entry_path.exists() {
                        return Ok(entry_path);
                    }
                }
            }
            let subpath_resolved = package_dir.join(sub);
            return self.resolve_with_extensions(subpath_resolved);
        }

        // Try "exports" field first (modern packages)
        if let Some(exports) = pkg.get("exports") {
            if let Some(entry) = resolve_exports(exports, ".") {
                let entry_path = package_dir.join(entry);
                return self.resolve_with_extensions(entry_path);
            }
        }

        // Try "module" field (ESM)
        if let Some(module) = pkg.get("module").and_then(|v| v.as_str()) {
            let module_path = package_dir.join(module);
            if module_path.exists() {
                return Ok(module_path);
            }
        }

        // Try "main" field (CommonJS)
        if let Some(main) = pkg.get("main").and_then(|v| v.as_str()) {
            let main_path = package_dir.join(main);
            return self.resolve_with_extensions(main_path);
        }

        // Fall back to index.js
        let index = package_dir.join("index.js");
        if index.exists() {
            return Ok(index);
        }

        Err(anyhow!("Cannot resolve package entry point"))
    }

    /// Detect if a file is CommonJS or ESM
    fn detect_module_type(&self, path: &Path) -> ModuleType {
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        match extension {
            "mjs" => ModuleType::JavaScript,
            "cjs" => ModuleType::JavaScript, // Will be wrapped as CommonJS
            "json" => ModuleType::Json,
            _ => {
                // Check package.json for "type": "module"
                if let Some(parent) = path.parent() {
                    let package_json = parent.join("package.json");
                    if package_json.exists() {
                        if let Ok(content) = std::fs::read_to_string(&package_json) {
                            if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) {
                                if pkg.get("type").and_then(|v| v.as_str()) == Some("module") {
                                    return ModuleType::JavaScript;
                                }
                            }
                        }
                    }
                }
                ModuleType::JavaScript
            }
        }
    }
}

impl Default for NodeModuleLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl ModuleLoader for NodeModuleLoader {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        _kind: ResolutionKind,
    ) -> Result<ModuleSpecifier, ModuleLoaderError> {
        // Handle Node.js built-in modules with a special URL scheme
        if Self::is_node_builtin(specifier) {
            let builtin_name = specifier
                .strip_prefix("node:")
                .unwrap_or(specifier)
                .trim_end_matches('/');
            // Use a special URL scheme for built-ins so we can intercept them in load()
            return ModuleSpecifier::parse(&format!("node:{}", builtin_name))
                .map_err(|e| JsErrorBox::generic(e.to_string()));
        }

        let referrer_path = if referrer.starts_with("file://") {
            PathBuf::from(referrer.strip_prefix("file://").unwrap_or(referrer))
        } else if referrer.starts_with("node:") {
            // If referrer is a built-in, use current directory
            self.base_dir.join("index.js")
        } else {
            PathBuf::from(referrer)
        };

        let resolved_path = self
            .resolve_module_path(specifier, &referrer_path)
            .map_err(|e| JsErrorBox::generic(e.to_string()))?;

        let canonical = std::fs::canonicalize(&resolved_path).unwrap_or(resolved_path);
        let canonical = if canonical.is_dir() {
            self.resolve_with_extensions(canonical)
                .map_err(|e| JsErrorBox::generic(e.to_string()))?
        } else {
            canonical
        };

        ModuleSpecifier::from_file_path(&canonical).map_err(|_| {
            JsErrorBox::generic(format!(
                "Failed to create module specifier for {:?}",
                canonical
            ))
        })
    }

    fn load(
        &self,
        module_specifier: &ModuleSpecifier,
        _maybe_referrer: Option<&ModuleLoadReferrer>,
        _options: ModuleLoadOptions,
    ) -> ModuleLoadResponse {
        // Handle Node.js built-in modules with stubs
        if module_specifier.scheme() == "node" {
            let builtin_name = module_specifier.path();
            let stub_code = get_builtin_stub(builtin_name);
            return ModuleLoadResponse::Sync(Ok(ModuleSource::new(
                ModuleType::JavaScript,
                ModuleSourceCode::String(stub_code.into()),
                module_specifier,
                None,
            )));
        }

        let path = match module_specifier.to_file_path() {
            Ok(p) => p,
            Err(_) => {
                return ModuleLoadResponse::Sync(Err(JsErrorBox::generic("Invalid file path")))
            }
        };

        let code = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                return ModuleLoadResponse::Sync(Err(JsErrorBox::generic(format!(
                    "Failed to read module {:?}: {}",
                    path, e
                ))))
            }
        };

        let module_type = self.detect_module_type(&path);

        // Wrap CommonJS modules if needed
        let code = if module_type != ModuleType::Json && is_commonjs(&code) {
            wrap_commonjs(&code)
        } else {
            code
        };

        ModuleLoadResponse::Sync(Ok(ModuleSource::new(
            module_type,
            ModuleSourceCode::String(code.into()),
            module_specifier,
            None,
        )))
    }
}

/// Parse a package specifier into (package_name, subpath)
fn parse_package_specifier(specifier: &str) -> (String, Option<String>) {
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
fn resolve_exports(exports: &serde_json::Value, subpath: &str) -> Option<String> {
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
fn is_commonjs(code: &str) -> bool {
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

fn looks_like_esm(code: &str) -> bool {
    code.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("import ")
            || trimmed.starts_with("export ")
            || trimmed.starts_with("export{")
    })
}

/// Wrap CommonJS code as ESM
fn wrap_commonjs(code: &str) -> String {
    // Extract all require() specifiers so we can convert them to ESM imports
    let code_without_comments = strip_js_comments(code);
    let mut require_specs: Vec<String> = Vec::new();
    for cap in REQUIRE_CALL_RE.captures_iter(&code_without_comments) {
        if let Some(spec) = cap.get(1) {
            let spec_str = spec.as_str().to_string();
            if !require_specs.contains(&spec_str) {
                require_specs.push(spec_str);
            }
        }
    }

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

    // Generate require() lookup cases
    let require_cases = require_specs
        .iter()
        .enumerate()
        .map(|(i, spec)| {
            if spec.ends_with(".json") {
                format!("        if (specifier === '{}') return _req_{};", spec, i)
            } else {
                format!(
                    "        if (specifier === '{}') return __perry_require_namespace(_req_{});",
                    spec, i
                )
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

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
        r#"{}
const _cjs = (function() {{
    var module = {{ exports: {{}} }};
    var exports = module.exports;
    function __perry_require_namespace(ns) {{
        try {{
            if (ns.__perry_commonjs === true && ns.default !== undefined) return ns.default;
        }} catch (_) {{
        }}
        return ns;
    }}
    function require(specifier) {{
{}
        throw new Error('require() is not supported: ' + specifier);
    }}

    {}

    return module.exports;
}})();

export default _cjs;
export const __perry_commonjs = true;
{}
{}
"#,
        imports, require_cases, code, named_export_decls, export_star_decls
    )
}

fn strip_js_comments(code: &str) -> String {
    let without_blocks = BLOCK_COMMENT_RE.replace_all(code, "");
    LINE_COMMENT_RE
        .replace_all(&without_blocks, "")
        .into_owned()
}

fn is_safe_js_binding_name(name: &str) -> bool {
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

/// Get a stub implementation for a Node.js built-in module
fn get_builtin_stub(name: &str) -> String {
    match name {
        "net" => r#"
// Stub implementation for Node.js 'net' module
export class Socket {
    constructor() {}
    connect() { return this; }
    write() { return true; }
    end() {}
    destroy() {}
    on() { return this; }
    once() { return this; }
    removeListener() { return this; }
    setTimeout() { return this; }
    setNoDelay() { return this; }
    setKeepAlive() { return this; }
}
export class Server {
    constructor() {}
    listen() { return this; }
    close() {}
    on() { return this; }
}
export function createServer() { return new Server(); }
export function createConnection() { return new Socket(); }
export function connect() { return new Socket(); }
export function isIP() { return 0; }
export function isIPv4() { return false; }
export function isIPv6() { return false; }
export default { Socket, Server, createServer, createConnection, connect, isIP, isIPv4, isIPv6 };
"#.to_string(),
        "tls" => r#"
// Stub implementation for Node.js 'tls' module
export class TLSSocket {
    constructor() {}
    connect() { return this; }
    on() { return this; }
}
export function connect() { return new TLSSocket(); }
export function createSecureContext() { return {}; }
export default { TLSSocket, connect, createSecureContext };
"#.to_string(),
        "http" | "https" | "http2" => r#"
// Stub implementation for Node.js http/https/http2 module
export class IncomingMessage {}
export class ServerResponse {}
export class Agent {}
export function request() { throw new Error('http.request not supported in this environment'); }
export function get() { throw new Error('http.get not supported in this environment'); }
export function createServer() { throw new Error('http.createServer not supported in this environment'); }
export function createSecureServer() { throw new Error('http2.createSecureServer not supported in this environment'); }
export default { IncomingMessage, ServerResponse, Agent, request, get, createServer, createSecureServer };
"#.to_string(),
        "crypto" => r#"
// Stub implementation for Node.js 'crypto' module
export function randomBytes(size) {
    const arr = new Uint8Array(size);
    crypto.getRandomValues(arr);
    return arr;
}
export function createHash(algorithm) {
    return {
        update(data) { this._data = (this._data || '') + data; return this; },
        digest(encoding) { return ''; }
    };
}
export function createHmac(algorithm, key) {
    return {
        update(data) { return this; },
        digest(encoding) { return ''; }
    };
}
export function pbkdf2Sync() { return new Uint8Array(32); }
export function pbkdf2() { return Promise.resolve(new Uint8Array(32)); }
export default { randomBytes, createHash, createHmac, pbkdf2Sync, pbkdf2 };
"#.to_string(),
        "fs" => r#"
// Stub implementation for Node.js 'fs' module
export function readFileSync() { throw new Error('fs.readFileSync not supported'); }
export function writeFileSync() { throw new Error('fs.writeFileSync not supported'); }
export function existsSync() { return false; }
export function mkdirSync() {}
export function readdirSync() { return []; }
export function statSync() { throw new Error('fs.statSync not supported'); }
export function isDirectory() { return 0; }
export const promises = {
    readFile: async () => { throw new Error('fs.promises.readFile not supported'); },
    writeFile: async () => { throw new Error('fs.promises.writeFile not supported'); },
};
export default { readFileSync, writeFileSync, existsSync, mkdirSync, readdirSync, statSync, isDirectory, promises };
"#.to_string(),
        "path" => r#"
// Stub implementation for Node.js 'path' module
export const sep = '/';
export const delimiter = ':';
export function join(...parts) { return parts.join('/').replace(/\/+/g, '/'); }
export function resolve(...parts) { return '/' + parts.join('/').replace(/\/+/g, '/'); }
export function dirname(p) { return p.split('/').slice(0, -1).join('/') || '/'; }
export function basename(p, ext) {
    let base = p.split('/').pop() || '';
    if (ext && base.endsWith(ext)) base = base.slice(0, -ext.length);
    return base;
}
export function extname(p) { const m = p.match(/\.[^.]+$/); return m ? m[0] : ''; }
export function isAbsolute(p) { return p.startsWith('/'); }
export function normalize(p) { return p.replace(/\/+/g, '/'); }
export function relative(from, to) { return to; }
export function parse(p) { return { root: '/', dir: dirname(p), base: basename(p), ext: extname(p), name: basename(p, extname(p)) }; }
export function format(obj) { return (obj.dir || '') + '/' + (obj.base || obj.name + obj.ext); }
export default { sep, delimiter, join, resolve, dirname, basename, extname, isAbsolute, normalize, relative, parse, format };
"#.to_string(),
        "os" => r#"
// Stub implementation for Node.js 'os' module
export function platform() { return 'unknown'; }
export function arch() { return 'unknown'; }
export function cpus() { return []; }
export function homedir() { return '/'; }
export function tmpdir() { return '/tmp'; }
export function hostname() { return 'localhost'; }
export function type() { return 'Unknown'; }
export function release() { return '0.0.0'; }
export function totalmem() { return 0; }
export function freemem() { return 0; }
export function uptime() { return 0; }
export function loadavg() { return [0, 0, 0]; }
export function networkInterfaces() { return {}; }
export const EOL = '\n';
export default { platform, arch, cpus, homedir, tmpdir, hostname, type, release, totalmem, freemem, uptime, loadavg, networkInterfaces, EOL };
"#.to_string(),
        "stream" | "stream/web" => r#"
// Stub implementation for Node.js 'stream' module
export class Readable {
    constructor() {}
    read() { return null; }
    on() { return this; }
    pipe() { return this; }
}
export class Writable {
    constructor() {}
    write() { return true; }
    end() {}
    on() { return this; }
}
export class Duplex extends Readable {
    write() { return true; }
    end() {}
}
export class Transform extends Duplex {}
export class PassThrough extends Transform {}
export class ReadableStream {}
export class WritableStream {}
export class TransformStream {}
export function pipeline() {}
export function finished() {}
export default { Readable, Writable, Duplex, Transform, PassThrough, ReadableStream, WritableStream, TransformStream, pipeline, finished };
"#.to_string(),
        "repl" => r#"
// Stub implementation for Node.js 'repl' module
export function start() {
    return {
        context: {},
        on() { return this; },
        close() {}
    };
}
export default { start };
"#.to_string(),
        "timers" => r#"
// Stub implementation for Node.js 'timers' module
export const setTimeout = globalThis.setTimeout.bind(globalThis);
export const clearTimeout = globalThis.clearTimeout.bind(globalThis);
export const setInterval = globalThis.setInterval.bind(globalThis);
export const clearInterval = globalThis.clearInterval.bind(globalThis);
export const setImmediate = globalThis.setImmediate || ((fn, ...args) => setTimeout(fn, 0, ...args));
export const clearImmediate = globalThis.clearImmediate || clearTimeout;
export default { setTimeout, clearTimeout, setInterval, clearInterval, setImmediate, clearImmediate };
"#.to_string(),
        "buffer" => r#"
// Stub implementation for Node.js 'buffer' module
export const Buffer = globalThis.Buffer || {
    from: (data, encoding) => new Uint8Array(typeof data === 'string' ? new TextEncoder().encode(data) : data),
    alloc: (size) => new Uint8Array(size),
    allocUnsafe: (size) => new Uint8Array(size),
    isBuffer: (obj) => obj instanceof Uint8Array,
    concat: (list) => {
        const total = list.reduce((acc, arr) => acc + arr.length, 0);
        const result = new Uint8Array(total);
        let offset = 0;
        for (const arr of list) { result.set(arr, offset); offset += arr.length; }
        return result;
    },
};
export default { Buffer };
"#.to_string(),
        "util" => r#"
// Stub implementation for Node.js 'util' module
export function promisify(fn) { return (...args) => new Promise((resolve, reject) => fn(...args, (err, result) => err ? reject(err) : resolve(result))); }
export function callbackify(fn) { return (...args) => { const cb = args.pop(); fn(...args).then(r => cb(null, r)).catch(cb); }; }
export function inspect(obj) { return JSON.stringify(obj); }
export function format(fmt, ...args) { return fmt; }
export function debuglog() { return () => {}; }
export function deprecate(fn) { return fn; }
export function inherits(ctor, superCtor) { Object.setPrototypeOf(ctor.prototype, superCtor.prototype); }
export const TextEncoder = globalThis.TextEncoder;
export const TextDecoder = globalThis.TextDecoder;
// util.types — Node's runtime introspection namespace. NestJS / rxjs
// reach into this for cheap Promise / TypedArray / Map / Set probes
// during DI dispatch. Most call sites just want a boolean; returning
// `false` for an unknown shape is the conservative answer (the caller
// then falls through to its own duck-typing path).
const _isPromiseLike = (v) => v != null && (typeof v === "object" || typeof v === "function") && typeof v.then === "function";
export const types = {
    isPromise: (v) => _isPromiseLike(v),
    isAsyncFunction: (v) => typeof v === "function" && v.constructor && v.constructor.name === "AsyncFunction",
    isGeneratorFunction: (v) => typeof v === "function" && v.constructor && v.constructor.name === "GeneratorFunction",
    isMap: (v) => v instanceof Map,
    isSet: (v) => v instanceof Set,
    isWeakMap: (v) => v instanceof WeakMap,
    isWeakSet: (v) => v instanceof WeakSet,
    isRegExp: (v) => v instanceof RegExp,
    isDate: (v) => v instanceof Date,
    isArrayBuffer: (v) => v instanceof ArrayBuffer,
    isSharedArrayBuffer: () => false,
    isDataView: (v) => v instanceof DataView,
    isUint8Array: (v) => v instanceof Uint8Array,
    isTypedArray: (v) => ArrayBuffer.isView(v) && !(v instanceof DataView),
    isProxy: () => false,
    isNativeError: (v) => v instanceof Error,
    isBoxedPrimitive: () => false,
    isAnyArrayBuffer: (v) => v instanceof ArrayBuffer,
    isModuleNamespaceObject: () => false,
};
export default { promisify, callbackify, inspect, format, debuglog, deprecate, inherits, TextEncoder, TextDecoder, types };
"#.to_string(),
        "events" => r#"
// Stub implementation for Node.js 'events' module
export class EventEmitter {
    constructor() { this._events = {}; }
    on(event, listener) { (this._events[event] = this._events[event] || []).push(listener); return this; }
    once(event, listener) { const wrapped = (...args) => { this.off(event, wrapped); listener(...args); }; return this.on(event, wrapped); }
    off(event, listener) { const arr = this._events[event]; if (arr) { const i = arr.indexOf(listener); if (i >= 0) arr.splice(i, 1); } return this; }
    removeListener(event, listener) { return this.off(event, listener); }
    emit(event, ...args) { const arr = this._events[event]; if (arr) arr.forEach(fn => fn(...args)); return !!arr; }
    removeAllListeners(event) { if (event) delete this._events[event]; else this._events = {}; return this; }
    listeners(event) { return this._events[event] || []; }
    listenerCount(event) { return (this._events[event] || []).length; }
    setMaxListeners() { return this; }
    getMaxListeners() { return 10; }
}
export function once(emitter, event) {
    return new Promise((resolve) => emitter.once(event, (...args) => resolve(args)));
}
EventEmitter.EventEmitter = EventEmitter;
EventEmitter.once = once;
export const __perry_commonjs = true;
export default EventEmitter;
"#.to_string(),
        "assert" => r#"
// Stub implementation for Node.js 'assert' module
export function ok(value, message) { if (!value) throw new Error(message || 'Assertion failed'); }
export function strictEqual(a, b, message) { if (a !== b) throw new Error(message || 'Assertion failed'); }
export function deepStrictEqual(a, b, message) { if (JSON.stringify(a) !== JSON.stringify(b)) throw new Error(message || 'Assertion failed'); }
export function notStrictEqual(a, b, message) { if (a === b) throw new Error(message || 'Assertion failed'); }
export function throws(fn, message) { try { fn(); throw new Error(message || 'Expected function to throw'); } catch (e) {} }
export function doesNotThrow(fn, message) { try { fn(); } catch (e) { throw new Error(message || 'Expected function not to throw'); } }
export function rejects(fn, message) { return fn().then(() => { throw new Error(message || 'Expected promise to reject'); }).catch(() => {}); }
export default { ok, strictEqual, deepStrictEqual, notStrictEqual, throws, doesNotThrow, rejects };
"#.to_string(),
        "url" => r#"
// Stub implementation for Node.js 'url' module
export const URL = globalThis.URL;
export const URLSearchParams = globalThis.URLSearchParams;
export function parse(urlString) { const u = new URL(urlString, 'http://localhost'); return { protocol: u.protocol, host: u.host, hostname: u.hostname, port: u.port, pathname: u.pathname, search: u.search, hash: u.hash, href: u.href }; }
export function format(urlObj) { return urlObj.href || ''; }
export function resolve(from, to) { return new URL(to, from).href; }
export default { URL, URLSearchParams, parse, format, resolve };
"#.to_string(),
        "querystring" => r#"
// Stub implementation for Node.js 'querystring' module
export function stringify(obj) { return new URLSearchParams(obj).toString(); }
export function parse(str) { const params = new URLSearchParams(str); const obj = {}; for (const [k, v] of params) obj[k] = v; return obj; }
export function escape(str) { return encodeURIComponent(str); }
export function unescape(str) { return decodeURIComponent(str); }
export default { stringify, parse, escape, unescape };
"#.to_string(),
        "tty" => r#"
// Stub implementation for Node.js 'tty' module
export function isatty() { return false; }
export class ReadStream {}
export class WriteStream {}
export default { isatty, ReadStream, WriteStream };
"#.to_string(),
        "string_decoder" => r#"
// Stub implementation for Node.js 'string_decoder' module
export class StringDecoder {
    constructor(encoding) { this.encoding = encoding || 'utf8'; }
    write(buffer) { return new TextDecoder(this.encoding).decode(buffer); }
    end(buffer) { return buffer ? this.write(buffer) : ''; }
}
export default { StringDecoder };
"#.to_string(),
        "zlib" => r#"
// Stub implementation for Node.js 'zlib' module
export function gzip() { throw new Error('zlib.gzip not supported'); }
export function gunzip() { throw new Error('zlib.gunzip not supported'); }
export function gzipSync() { throw new Error('zlib.gzipSync not supported'); }
export function gunzipSync(data) { throw new Error('zlib.gunzipSync not supported'); }
export function deflate() { throw new Error('zlib.deflate not supported'); }
export function inflate() { throw new Error('zlib.inflate not supported'); }
export function deflateSync() { throw new Error('zlib.deflateSync not supported'); }
export function inflateSync() { throw new Error('zlib.inflateSync not supported'); }
export function brotliCompress() { throw new Error('zlib.brotliCompress not supported'); }
export function brotliDecompress() { throw new Error('zlib.brotliDecompress not supported'); }
export function brotliCompressSync() { throw new Error('zlib.brotliCompressSync not supported'); }
export function brotliDecompressSync() { throw new Error('zlib.brotliDecompressSync not supported'); }
export function createGzip() { throw new Error('zlib.createGzip not supported'); }
export function createGunzip() { throw new Error('zlib.createGunzip not supported'); }
export function createDeflate() { throw new Error('zlib.createDeflate not supported'); }
export function createInflate() { throw new Error('zlib.createInflate not supported'); }
export default { gzip, gunzip, gzipSync, gunzipSync, deflate, inflate, deflateSync, inflateSync, brotliCompress, brotliDecompress, brotliCompressSync, brotliDecompressSync, createGzip, createGunzip, createDeflate, createInflate };
"#.to_string(),
        "async_hooks" => r#"
// Stub implementation for Node.js 'async_hooks' module
// Used by @nestjs/core for request-scoped DI context propagation (PR #754).
// No real async-context tracking here: each AsyncResource is a thin
// wrapper that just runs the callback in the current context.
export class AsyncResource {
    constructor(_type, _options) {}
    runInAsyncScope(fn, thisArg, ...args) { return fn.apply(thisArg, args); }
    emitDestroy() { return this; }
    asyncId() { return 0; }
    triggerAsyncId() { return 0; }
    bind(fn) {
        const ar = this;
        return function (...args) { return ar.runInAsyncScope(fn, this, ...args); };
    }
    static bind(fn, type, thisArg) {
        const ar = new AsyncResource(type || "bound-anonymous-fn");
        return ar.bind(thisArg !== undefined ? fn.bind(thisArg) : fn);
    }
}
export class AsyncLocalStorage {
    constructor() { this._store = undefined; }
    run(store, fn, ...args) {
        const prev = this._store;
        this._store = store;
        try { return fn(...args); } finally { this._store = prev; }
    }
    exit(fn, ...args) {
        const prev = this._store;
        this._store = undefined;
        try { return fn(...args); } finally { this._store = prev; }
    }
    getStore() { return this._store; }
    enterWith(store) { this._store = store; }
    disable() { this._store = undefined; }
}
export function executionAsyncId() { return 0; }
export function executionAsyncResource() { return {}; }
export function triggerAsyncId() { return 0; }
export function createHook() { return { enable() { return this; }, disable() { return this; } }; }
export default { AsyncResource, AsyncLocalStorage, executionAsyncId, executionAsyncResource, triggerAsyncId, createHook };
"#.to_string(),
        // Issue #755: Node built-in subpath aliases. These ship in real Node
        // as separate module IDs (`fs/promises`, `stream/promises`, etc.)
        // and packages like colyseus import them directly. Stubs mirror the
        // promise-flavored shape of the corresponding base module.
        "fs/promises" => r#"
// Stub implementation for Node.js 'fs/promises' module
export async function readFile() { throw new Error('fs.promises.readFile not supported'); }
export async function writeFile() { throw new Error('fs.promises.writeFile not supported'); }
export async function appendFile() { throw new Error('fs.promises.appendFile not supported'); }
export async function access() { throw new Error('fs.promises.access not supported'); }
export async function stat() { throw new Error('fs.promises.stat not supported'); }
export async function lstat() { throw new Error('fs.promises.lstat not supported'); }
export async function mkdir() { throw new Error('fs.promises.mkdir not supported'); }
export async function readdir() { return []; }
export async function rmdir() { throw new Error('fs.promises.rmdir not supported'); }
export async function rm() { throw new Error('fs.promises.rm not supported'); }
export async function unlink() { throw new Error('fs.promises.unlink not supported'); }
export async function rename() { throw new Error('fs.promises.rename not supported'); }
export async function copyFile() { throw new Error('fs.promises.copyFile not supported'); }
export async function chmod() { throw new Error('fs.promises.chmod not supported'); }
export async function chown() { throw new Error('fs.promises.chown not supported'); }
export async function realpath() { throw new Error('fs.promises.realpath not supported'); }
export async function symlink() { throw new Error('fs.promises.symlink not supported'); }
export async function readlink() { throw new Error('fs.promises.readlink not supported'); }
export async function open() { throw new Error('fs.promises.open not supported'); }
export async function utimes() { throw new Error('fs.promises.utimes not supported'); }
export async function truncate() { throw new Error('fs.promises.truncate not supported'); }
export async function cp() { throw new Error('fs.promises.cp not supported'); }
export const constants = {};
export default { readFile, writeFile, appendFile, access, stat, lstat, mkdir, readdir, rmdir, rm, unlink, rename, copyFile, chmod, chown, realpath, symlink, readlink, open, utimes, truncate, cp, constants };
"#.to_string(),
        "stream/promises" => r#"
// Stub implementation for Node.js 'stream/promises' module
export async function pipeline() { throw new Error('stream.promises.pipeline not supported'); }
export async function finished() { throw new Error('stream.promises.finished not supported'); }
export default { pipeline, finished };
"#.to_string(),
        "stream/consumers" => r#"
// Stub implementation for Node.js 'stream/consumers' module
export async function arrayBuffer() { throw new Error('stream.consumers.arrayBuffer not supported'); }
export async function blob() { throw new Error('stream.consumers.blob not supported'); }
export async function buffer() { throw new Error('stream.consumers.buffer not supported'); }
export async function json() { throw new Error('stream.consumers.json not supported'); }
export async function text() { throw new Error('stream.consumers.text not supported'); }
export default { arrayBuffer, blob, buffer, json, text };
"#.to_string(),
        "stream/web" => r#"
// Stub implementation for Node.js 'stream/web' module
export const ReadableStream = globalThis.ReadableStream;
export const WritableStream = globalThis.WritableStream;
export const TransformStream = globalThis.TransformStream;
export const ByteLengthQueuingStrategy = globalThis.ByteLengthQueuingStrategy;
export const CountQueuingStrategy = globalThis.CountQueuingStrategy;
export default { ReadableStream, WritableStream, TransformStream, ByteLengthQueuingStrategy, CountQueuingStrategy };
"#.to_string(),
        "dns/promises" => r#"
// Stub implementation for Node.js 'dns/promises' module
export async function lookup() { throw new Error('dns.promises.lookup not supported'); }
export async function resolve() { throw new Error('dns.promises.resolve not supported'); }
export async function resolve4() { throw new Error('dns.promises.resolve4 not supported'); }
export async function resolve6() { throw new Error('dns.promises.resolve6 not supported'); }
export async function reverse() { throw new Error('dns.promises.reverse not supported'); }
export default { lookup, resolve, resolve4, resolve6, reverse };
"#.to_string(),
        "timers/promises" => r#"
// Stub implementation for Node.js 'timers/promises' module
export function setTimeout(ms, value) { return new Promise((resolve) => globalThis.setTimeout(() => resolve(value), ms)); }
export function setImmediate(value) { return new Promise((resolve) => globalThis.setTimeout(() => resolve(value), 0)); }
export async function* setInterval(ms, value) { while (true) { await new Promise((r) => globalThis.setTimeout(r, ms)); yield value; } }
export default { setTimeout, setImmediate, setInterval };
"#.to_string(),
        "readline/promises" => r#"
// Stub implementation for Node.js 'readline/promises' module
export class Interface {
    constructor() {}
    async question() { throw new Error('readline.promises.question not supported'); }
    close() {}
    on() { return this; }
}
export function createInterface() { return new Interface(); }
export default { Interface, createInterface };
"#.to_string(),
        "util/types" => r#"
// Stub implementation for Node.js 'util/types' module
export function isDate(v) { return v instanceof Date; }
export function isRegExp(v) { return v instanceof RegExp; }
export function isMap(v) { return v instanceof Map; }
export function isSet(v) { return v instanceof Set; }
export function isPromise(v) { return v && typeof v.then === 'function'; }
export function isArrayBuffer(v) { return v instanceof ArrayBuffer; }
export function isTypedArray(v) { return ArrayBuffer.isView(v) && !(v instanceof DataView); }
export function isUint8Array(v) { return v instanceof Uint8Array; }
export default { isDate, isRegExp, isMap, isSet, isPromise, isArrayBuffer, isTypedArray, isUint8Array };
"#.to_string(),
        "assert/strict" => r#"
// Stub implementation for Node.js 'assert/strict' module
export function ok(value, message) { if (!value) throw new Error(message || 'Assertion failed'); }
export function strictEqual(a, b, message) { if (a !== b) throw new Error(message || 'Assertion failed'); }
export function deepStrictEqual(a, b, message) { if (JSON.stringify(a) !== JSON.stringify(b)) throw new Error(message || 'Assertion failed'); }
export function notStrictEqual(a, b, message) { if (a === b) throw new Error(message || 'Assertion failed'); }
export default { ok, strictEqual, deepStrictEqual, notStrictEqual };
"#.to_string(),
        _ => format!(r#"
// Empty stub for unsupported Node.js built-in: {}
export default {{}};
"#, name),
    }
}

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
        let wrapped = wrap_commonjs("exports.default = 1;\nexports.iterate = 2;");

        assert!(!wrapped.contains("export const default"));
        assert!(wrapped.contains("export default _cjs;"));
        assert!(wrapped.contains("export const iterate = _cjs.iterate;"));
    }

    #[test]
    fn test_wrap_commonjs_requires_namespace_imports() {
        let wrapped = wrap_commonjs("const uid = require('uid');\nexports.value = uid.uid();");

        assert!(wrapped.contains("import * as _req_0 from 'uid';"));
        assert!(
            wrapped.contains("if (specifier === 'uid') return __perry_require_namespace(_req_0);")
        );
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
        );

        assert!(!wrapped.contains("import * as _req_0 from './compiled.js';"));
        assert!(!wrapped.contains("specifier === './compiled.js'"));
    }

    #[test]
    fn test_wrap_commonjs_imports_json_with_attribute() {
        let wrapped = wrap_commonjs("exports.version = require('../package.json').version;");

        assert!(wrapped.contains("import _req_0 from '../package.json' with { type: 'json' };"));
        assert!(wrapped.contains("if (specifier === '../package.json') return _req_0;"));
    }

    #[test]
    fn test_wrap_commonjs_emits_export_star_barrels() {
        let wrapped = wrap_commonjs(
            "const tslib_1 = require('tslib');\ntslib_1.__exportStar(require('./decorators'), exports);",
        );

        assert!(wrapped.contains("export * from './decorators';"));
    }

    #[test]
    fn test_wrap_commonjs_aliases_reserved_export_names() {
        let wrapped = wrap_commonjs("exports.static = require('serve-static');");

        assert!(wrapped.contains("const _cjs_export_static = _cjs.static;"));
        assert!(wrapped.contains("export { _cjs_export_static as static };"));
        assert!(!wrapped.contains("export const static"));
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
}
