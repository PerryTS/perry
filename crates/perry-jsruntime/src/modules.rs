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
fn lookup_embedded_module(path: &str) -> Option<Arc<String>> {
    EMBEDDED_MODULES
        .read()
        .ok()
        .and_then(|map| map.get(path).cloned())
}

/// Look up the build-time canonical path that a bare specifier maps to.
fn lookup_embedded_alias(specifier: &str) -> Option<String> {
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
fn lookup_embedded_path_with_extensions(base: &Path) -> Option<PathBuf> {
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
        // Issue #818 follow-up: prefer embedded-bundle lookups over disk
        // probes. For bare specifiers ("hono", "@scope/x") an alias map
        // gives us the canonical build-time path directly; for relative
        // and absolute paths we still walk the standard candidate chain
        // and then check whether the resolved path matches an embedded
        // entry even when the file is absent from the runtime filesystem.
        if !specifier.starts_with("./")
            && !specifier.starts_with("../")
            && !specifier.starts_with('/')
            && !specifier.starts_with("file://")
        {
            if let Some(embedded_path) = lookup_embedded_alias(specifier) {
                return Ok(PathBuf::from(embedded_path));
            }
        }

        // Handle file:// URLs
        if specifier.starts_with("file://") {
            let path_str = specifier.strip_prefix("file://").unwrap_or(specifier);
            let path = PathBuf::from(path_str);
            if path.exists() && path.is_file() {
                return Ok(path);
            }
            if lookup_embedded_module(&path.to_string_lossy()).is_some() {
                return Ok(path);
            }
            return self.resolve_with_extensions(path);
        }

        // Handle relative imports (./ or ../)
        if specifier.starts_with("./") || specifier.starts_with("../") {
            let referrer_dir = referrer.parent().unwrap_or(&self.base_dir);
            let resolved = referrer_dir.join(specifier);
            match self.resolve_with_extensions(resolved.clone()) {
                Ok(resolved) => {
                    // Check browser field mapping (e.g., ethers geturl.js -> geturl-browser.js)
                    if let Some(browser_path) = self.check_browser_field(&resolved) {
                        return Ok(browser_path);
                    }
                    return Ok(resolved);
                }
                Err(e) => {
                    // Self-contained binary path: the file isn't on disk
                    // because node_modules/ was left behind. Probe the
                    // embedded map with the same extension/index candidates
                    // we'd try against the filesystem.
                    if let Some(p) = lookup_embedded_path_with_extensions(&resolved) {
                        return Ok(p);
                    }
                    return Err(e);
                }
            }
        }

        // Handle absolute paths
        if specifier.starts_with('/') {
            let resolved = PathBuf::from(specifier);
            if let Ok(p) = self.resolve_with_extensions(resolved.clone()) {
                return Ok(p);
            }
            if let Some(p) = lookup_embedded_path_with_extensions(&resolved) {
                return Ok(p);
            }
            return self.resolve_with_extensions(resolved);
        }

        // Handle node_modules
        match self.resolve_from_node_modules(specifier, referrer) {
            Ok(p) => Ok(p),
            Err(e) => {
                if let Some(embedded_path) = lookup_embedded_alias(specifier) {
                    return Ok(PathBuf::from(embedded_path));
                }
                Err(e)
            }
        }
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
        } else if referrer.starts_with("perry-missing:") {
            // Missing-stub referrer: anchor further lookups at the project root.
            self.base_dir.join("index.js")
        } else {
            PathBuf::from(referrer)
        };

        let resolved_path = match self.resolve_module_path(specifier, &referrer_path) {
            Ok(p) => p,
            Err(e) => {
                // V8-fallback graceful-degradation: bare specifiers that fail to
                // resolve in node_modules (common case: optional/peer deps like
                // debug → `require('supports-color')` inside a try/catch) become
                // synthetic `perry-missing:<spec>` modules. `load()` returns a
                // marker stub; the CJS wrapper's `require()` function then
                // throws a JS MODULE_NOT_FOUND error inside the user's
                // try/catch instead of aborting the whole module graph at
                // static-import time. Only applies to bare specifiers (no
                // ./, ../, /, or file:// prefix) — relative/absolute path
                // failures stay hard errors.
                let is_bare = !specifier.starts_with("./")
                    && !specifier.starts_with("../")
                    && !specifier.starts_with('/')
                    && !specifier.starts_with("file://")
                    && !specifier.contains("://");
                if is_bare {
                    log::warn!(
                        "[js_load_module] missing bare module '{}' — returning soft-throw stub ({})",
                        specifier,
                        e
                    );
                    return ModuleSpecifier::parse(&format!("perry-missing:{}", specifier))
                        .map_err(|err| JsErrorBox::generic(err.to_string()));
                }
                return Err(JsErrorBox::generic(e.to_string()));
            }
        };

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

        // Handle missing-module stubs. The synthetic `perry-missing:<spec>`
        // scheme is produced by resolve() when a bare specifier can't be
        // located in node_modules. The stub exposes a marker so the
        // wrap_commonjs() generated `require()` can throw a JS
        // MODULE_NOT_FOUND error (caught by the caller's try/catch)
        // instead of failing static-import time.
        if module_specifier.scheme() == "perry-missing" {
            let spec = module_specifier.path();
            // Escape single quotes / backslashes for embedding in the JS string.
            let escaped = spec.replace('\\', "\\\\").replace('\'', "\\'");
            let stub_code = format!(
                "export const __perry_missing = true;\n\
                 export const __perry_specifier = '{}';\n\
                 export default undefined;\n",
                escaped
            );
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

        // Issue #818 follow-up: embedded-bundle first. Self-contained
        // binaries register every JS module they import at startup; the
        // map is keyed on build-time canonical paths, which is what
        // `resolve()` returns. Falls through to disk only when nothing's
        // registered for this path — preserves the dev-build behavior
        // where `node_modules/` sits next to the binary.
        let path_key = path.to_string_lossy().to_string();
        let code = if let Some(embedded) = lookup_embedded_module(&path_key) {
            (*embedded).clone()
        } else {
            match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(e) => {
                    return ModuleLoadResponse::Sync(Err(JsErrorBox::generic(format!(
                        "Failed to read module {:?}: {}",
                        path, e
                    ))))
                }
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

    // Generate require() lookup cases. Each non-JSON case first checks
    // whether the imported namespace carries the `perry-missing:` stub
    // marker — if so we throw a Node-compatible MODULE_NOT_FOUND error
    // INSIDE the require() function body so a wrapping try/catch (the
    // optional-dependency pattern used by debug, etc.) can catch it.
    let require_cases = require_specs
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
                     \x20           return __perry_require_namespace(_req_{idx});\n\
                     \x20       }}",
                    spec = escaped_spec,
                    idx = i
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
//
// `createServer(handler)` bridges to the V8-fallback HTTP server via
// the `op_perry_http_*` ops (see crates/perry-jsruntime/src/ops.rs).
// This is the minimum viable shape — enough for express.listen() to
// receive real requests and respond. NOT a full Node `http.Server`
// (no streams, no trailers, no upgrade events). Apps that need richer
// HTTP server semantics should compile through the native path
// (perry-ext-http-server).
const __perry_core = (typeof Deno !== 'undefined' && Deno.core) ? Deno.core : null;
const __perry_ops = __perry_core && __perry_core.ops ? __perry_core.ops : null;

export class IncomingMessage {
    constructor(opaque) {
        this.method = opaque.method;
        this.url = opaque.url;
        this.headers = opaque.headers || {};
        this.rawHeaders = opaque.rawHeaders || [];
        this.httpVersion = '1.1';
        this.httpVersionMajor = 1;
        this.httpVersionMinor = 1;
        this.complete = true;
        this._body = opaque.body || '';
        this._listeners = Object.create(null);
        // Minimal stream-ish events: synthesize 'data' + 'end' on next tick
        // so handlers that wire `.on('data', ...).on('end', ...)` work.
        Promise.resolve().then(() => {
            const dataCbs = this._listeners['data'] || [];
            if (dataCbs.length > 0 && this._body) {
                for (const cb of dataCbs) cb(this._body);
            }
            const endCbs = this._listeners['end'] || [];
            for (const cb of endCbs) cb();
        });
    }
    on(event, listener) {
        (this._listeners[event] = this._listeners[event] || []).push(listener);
        return this;
    }
    once(event, listener) {
        const wrapped = (...args) => { this.off(event, wrapped); listener(...args); };
        return this.on(event, wrapped);
    }
    off(event, listener) {
        const arr = this._listeners[event];
        if (arr) { const i = arr.indexOf(listener); if (i >= 0) arr.splice(i, 1); }
        return this;
    }
    removeListener(event, listener) { return this.off(event, listener); }
    emit(event, ...args) {
        const arr = this._listeners[event];
        if (arr) arr.slice().forEach(fn => fn.apply(this, args));
        return !!arr;
    }
    setEncoding() { return this; }
    pause() { return this; }
    resume() { return this; }
}

export class ServerResponse {
    constructor(reqId) {
        this._reqId = reqId;
        this._status = 200;
        this._headers = []; // array of [name, value] preserving order
        this._headerMap = Object.create(null); // lowercase -> last value
        this._body = '';
        this._ended = false;
        this.headersSent = false;
        this.finished = false;
        this.statusCode = 200;
        this.statusMessage = '';
        this._listeners = Object.create(null);
    }
    setHeader(name, value) {
        const lower = String(name).toLowerCase();
        // Replace any previous entry for the same header.
        this._headers = this._headers.filter(p => p[0].toLowerCase() !== lower);
        this._headers.push([String(name), String(value)]);
        this._headerMap[lower] = String(value);
        return this;
    }
    getHeader(name) { return this._headerMap[String(name).toLowerCase()]; }
    getHeaders() {
        const out = {};
        for (const [k, v] of this._headers) out[k.toLowerCase()] = v;
        return out;
    }
    getHeaderNames() { return Object.keys(this._headerMap); }
    hasHeader(name) { return String(name).toLowerCase() in this._headerMap; }
    removeHeader(name) {
        const lower = String(name).toLowerCase();
        this._headers = this._headers.filter(p => p[0].toLowerCase() !== lower);
        delete this._headerMap[lower];
        return this;
    }
    writeHead(status, statusMessageOrHeaders, headers) {
        this._status = status;
        this.statusCode = status;
        let h = headers;
        if (statusMessageOrHeaders && typeof statusMessageOrHeaders !== 'string') {
            h = statusMessageOrHeaders;
        } else if (typeof statusMessageOrHeaders === 'string') {
            this.statusMessage = statusMessageOrHeaders;
        }
        if (h) {
            if (Array.isArray(h)) {
                // [name1, val1, name2, val2, ...]
                for (let i = 0; i + 1 < h.length; i += 2) this.setHeader(h[i], h[i + 1]);
            } else {
                for (const k of Object.keys(h)) this.setHeader(k, h[k]);
            }
        }
        return this;
    }
    write(chunk) {
        if (this._ended) return false;
        if (chunk == null) return true;
        if (typeof chunk === 'string') {
            this._body += chunk;
        } else if (chunk instanceof Uint8Array) {
            this._body += new TextDecoder().decode(chunk);
        } else if (chunk && chunk.buffer instanceof ArrayBuffer) {
            this._body += new TextDecoder().decode(new Uint8Array(chunk.buffer));
        } else {
            this._body += String(chunk);
        }
        return true;
    }
    end(chunk, encoding, cb) {
        if (this._ended) return this;
        if (typeof chunk === 'function') { cb = chunk; chunk = undefined; }
        else if (typeof encoding === 'function') { cb = encoding; encoding = undefined; }
        if (chunk != null) this.write(chunk);
        this._ended = true;
        this.finished = true;
        this.headersSent = true;
        // Default Content-Length when caller hasn't set Transfer-Encoding.
        const lower = this._headerMap;
        if (!lower['content-length'] && !lower['transfer-encoding']) {
            const len = (typeof TextEncoder !== 'undefined')
                ? new TextEncoder().encode(this._body).length
                : this._body.length;
            this.setHeader('Content-Length', String(len));
        }
        if (__perry_ops && __perry_ops.op_perry_http_respond) {
            try {
                __perry_ops.op_perry_http_respond(
                    this._reqId, this._status, JSON.stringify(this._headers), this._body);
            } catch (_) {}
        }
        const finishCbs = this._listeners['finish'] || [];
        for (const f of finishCbs) { try { f(); } catch (_) {} }
        const closeCbs = this._listeners['close'] || [];
        for (const f of closeCbs) { try { f(); } catch (_) {} }
        if (typeof cb === 'function') { try { cb(); } catch (_) {} }
        return this;
    }
    on(event, listener) {
        (this._listeners[event] = this._listeners[event] || []).push(listener);
        return this;
    }
    once(event, listener) {
        const wrapped = (...args) => { this.off(event, wrapped); listener(...args); };
        return this.on(event, wrapped);
    }
    off(event, listener) {
        const arr = this._listeners[event];
        if (arr) { const i = arr.indexOf(listener); if (i >= 0) arr.splice(i, 1); }
        return this;
    }
    removeListener(event, listener) { return this.off(event, listener); }
    emit(event, ...args) {
        const arr = this._listeners[event];
        if (arr) arr.slice().forEach(fn => fn.apply(this, args));
        return !!arr;
    }
    flushHeaders() { this.headersSent = true; }
}

export class Agent {}

class Server {
    constructor(handler) {
        this._handler = handler || (() => {});
        this._serverId = 0;
        this._listening = false;
        this._listeners = Object.create(null);
        this._listenAddress = null;
    }
    _emit(event, ...args) {
        const arr = this._listeners[event];
        if (arr) arr.slice().forEach(fn => { try { fn.apply(this, args); } catch (_) {} });
    }
    on(event, listener) {
        if (event === 'request' && typeof listener === 'function') {
            // Mirror Node: 'request' listeners run in addition to the
            // constructor handler. Stash them in _listeners and dispatch
            // alongside the main handler in the accept loop.
        }
        (this._listeners[event] = this._listeners[event] || []).push(listener);
        return this;
    }
    once(event, listener) {
        const wrapped = (...args) => { this.off(event, wrapped); listener(...args); };
        return this.on(event, wrapped);
    }
    off(event, listener) {
        const arr = this._listeners[event];
        if (arr) { const i = arr.indexOf(listener); if (i >= 0) arr.splice(i, 1); }
        return this;
    }
    removeListener(event, listener) { return this.off(event, listener); }
    addListener(event, listener) { return this.on(event, listener); }
    emit(event, ...args) { this._emit(event, ...args); return true; }
    setTimeout() { return this; }
    address() {
        if (!this._listenAddress) return null;
        return { port: this._listenAddress.port, address: this._listenAddress.host, family: 'IPv4' };
    }
    listen(...args) {
        // express calls listen(port, cb); also support (port, host, cb) and ({port, host}, cb).
        let port = 3000;
        let host = '0.0.0.0';
        let cb = null;
        for (const a of args) {
            if (typeof a === 'number') port = a;
            else if (typeof a === 'string') {
                const n = parseInt(a, 10);
                if (!isNaN(n)) port = n;
            } else if (typeof a === 'function') cb = a;
            else if (a && typeof a === 'object') {
                if (typeof a.port === 'number') port = a.port;
                if (typeof a.host === 'string') host = a.host;
            }
        }
        if (!__perry_ops || !__perry_ops.op_perry_http_listen) {
            throw new Error('http.createServer: Perry V8-fallback HTTP ops unavailable');
        }
        const self = this;
        // Kick off the bind + accept loop. Once bound, fire 'listening'
        // + the user callback and begin dispatching to the handler.
        (async () => {
            try {
                const sid = await __perry_ops.op_perry_http_listen(port, host);
                self._serverId = sid;
                self._listening = true;
                self._listenAddress = { port, host };
                self._emit('listening');
                if (typeof cb === 'function') { try { cb(); } catch (_) {} }
                // Accept loop
                while (self._listening && self._serverId !== 0) {
                    let r;
                    try { r = await __perry_ops.op_perry_http_accept(sid); }
                    catch (_) { break; }
                    if (!r || r.id === 0) break;
                    const req = new IncomingMessage(r);
                    const res = new ServerResponse(r.id);
                    // Fire 'request' listeners + the constructor handler.
                    const reqListeners = self._listeners['request'] || [];
                    for (const fn of reqListeners) {
                        try { fn.call(self, req, res); } catch (e) { /* swallow */ }
                    }
                    try {
                        const ret = self._handler(req, res);
                        if (ret && typeof ret.then === 'function') {
                            ret.catch(() => {});
                        }
                    } catch (e) {
                        if (!res._ended) {
                            try {
                                res.statusCode = 500;
                                res.end('Internal Server Error');
                            } catch (_) {}
                        }
                    }
                }
            } catch (err) {
                self._emit('error', err);
                if (typeof cb === 'function') { try { cb(err); } catch (_) {} }
            }
        })();
        return this;
    }
    close(cb) {
        if (this._serverId && __perry_ops && __perry_ops.op_perry_http_close) {
            try { __perry_ops.op_perry_http_close(this._serverId); } catch (_) {}
        }
        this._listening = false;
        this._serverId = 0;
        this._emit('close');
        if (typeof cb === 'function') { try { cb(); } catch (_) {} }
        return this;
    }
    closeAllConnections() {}
    closeIdleConnections() {}
    get listening() { return this._listening; }
}

// Issue #912 (#909 follow-up): express/router read `const { METHODS } =
// require('node:http')` at module init and immediately call `METHODS.map(...)`.
export const METHODS = [
    'ACL', 'BIND', 'CHECKOUT', 'CONNECT', 'COPY', 'DELETE', 'GET', 'HEAD',
    'LINK', 'LOCK', 'M-SEARCH', 'MERGE', 'MKACTIVITY', 'MKCALENDAR', 'MKCOL',
    'MOVE', 'NOTIFY', 'OPTIONS', 'PATCH', 'POST', 'PROPFIND', 'PROPPATCH',
    'PURGE', 'PUT', 'QUERY', 'REBIND', 'REPORT', 'SEARCH', 'SOURCE',
    'SUBSCRIBE', 'TRACE', 'UNBIND', 'UNLINK', 'UNLOCK', 'UNSUBSCRIBE'
];
export const STATUS_CODES = {
    100: 'Continue', 101: 'Switching Protocols', 200: 'OK', 201: 'Created',
    202: 'Accepted', 204: 'No Content', 301: 'Moved Permanently',
    302: 'Found', 304: 'Not Modified', 400: 'Bad Request', 401: 'Unauthorized',
    403: 'Forbidden', 404: 'Not Found', 405: 'Method Not Allowed',
    408: 'Request Timeout', 409: 'Conflict', 410: 'Gone', 413: 'Payload Too Large',
    414: 'URI Too Long', 415: 'Unsupported Media Type', 429: 'Too Many Requests',
    500: 'Internal Server Error', 501: 'Not Implemented', 502: 'Bad Gateway',
    503: 'Service Unavailable', 504: 'Gateway Timeout'
};
export function request() { throw new Error('http.request not supported in this environment'); }
export function get() { throw new Error('http.get not supported in this environment'); }
export function createServer(handler) { return new Server(handler); }
export function createSecureServer() { throw new Error('http2.createSecureServer not supported in this environment'); }
export { Server };
export default { IncomingMessage, ServerResponse, Server, Agent, METHODS, STATUS_CODES, request, get, createServer, createSecureServer };
"#.to_string(),
        "crypto" => r#"
// Stub implementation for Node.js 'crypto' module
export function randomBytes(size) {
    const arr = new Uint8Array(size);
    crypto.getRandomValues(arr);
    return arr;
}
// Issue: jose imports `randomFillSync` from `node:crypto` via the V8/JS
// fallback path. Node's `randomFillSync(buffer, offset?, size?)` fills the
// given TypedArray/Buffer with cryptographically secure random bytes in
// place and returns the buffer.
export function randomFillSync(buf, offset, size) {
    const o = offset || 0;
    const len = (buf && typeof buf.length === 'number') ? buf.length : 0;
    const n = (size != null) ? size : (len - o);
    let view;
    if (typeof buf.subarray === 'function') {
        view = buf.subarray(o, o + n);
    } else if (buf && buf.buffer) {
        view = new Uint8Array(buf.buffer, (buf.byteOffset || 0) + o, n);
    } else {
        view = buf;
    }
    crypto.getRandomValues(view);
    return buf;
}
export function randomUUID() {
    // RFC 4122 v4 UUID using getRandomValues
    const b = new Uint8Array(16);
    crypto.getRandomValues(b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    const h = [];
    for (let i = 0; i < 16; i++) h.push((b[i] + 0x100).toString(16).slice(1));
    return h[0]+h[1]+h[2]+h[3]+'-'+h[4]+h[5]+'-'+h[6]+h[7]+'-'+h[8]+h[9]+'-'+h[10]+h[11]+h[12]+h[13]+h[14]+h[15];
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
// Best-effort stubs for additional `node:crypto` named exports that
// libraries like `jose` import at module-init time. Returning sane
// no-ops lets the import resolve (so ESM linking succeeds and the
// library can load), even when the underlying primitive isn't wired
// through Perry's native crypto path. Calling them at runtime in the
// V8 fallback path will throw or no-op deterministically, NOT crash
// the JS module loader. Real implementations live behind the native
// FFI path (`Expr::Crypto*` in HIR / `js_crypto_*` in perry-stdlib).
export function timingSafeEqual(a, b) {
    if (!a || !b || a.length !== b.length) return false;
    let r = 0;
    for (let i = 0; i < a.length; i++) r |= a[i] ^ b[i];
    return r === 0;
}
export function createCipheriv() { throw new Error('crypto.createCipheriv not supported in V8 fallback'); }
export function createDecipheriv() { throw new Error('crypto.createDecipheriv not supported in V8 fallback'); }
export function createSecretKey(key) { return { _key: key, type: 'secret', asymmetricKeyType: undefined, symmetricKeySize: (key && key.length) || 0, export() { return key; } }; }
export function createPrivateKey() { throw new Error('crypto.createPrivateKey not supported in V8 fallback'); }
export function createPublicKey() { throw new Error('crypto.createPublicKey not supported in V8 fallback'); }
export function generateKeyPair() { throw new Error('crypto.generateKeyPair not supported in V8 fallback'); }
export function diffieHellman() { throw new Error('crypto.diffieHellman not supported in V8 fallback'); }
export function publicEncrypt() { throw new Error('crypto.publicEncrypt not supported in V8 fallback'); }
export function privateDecrypt() { throw new Error('crypto.privateDecrypt not supported in V8 fallback'); }
export function getCiphers() { return []; }
export class KeyObject {
    constructor() { this.type = 'secret'; this.asymmetricKeyType = undefined; this.symmetricKeySize = 0; }
    export() { return new Uint8Array(0); }
}
export const constants = {
    RSA_PKCS1_PADDING: 1,
    RSA_PKCS1_OAEP_PADDING: 4,
    RSA_PSS_SALTLEN_DIGEST: -1,
    RSA_PSS_SALTLEN_MAX_SIGN: -2,
    RSA_PSS_SALTLEN_AUTO: -2,
};
export default { randomBytes, randomFillSync, randomUUID, createHash, createHmac, pbkdf2Sync, pbkdf2, timingSafeEqual, createCipheriv, createDecipheriv, createSecretKey, createPrivateKey, createPublicKey, generateKeyPair, diffieHellman, publicEncrypt, privateDecrypt, getCiphers, KeyObject, constants };
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
        "stream" => r#"
// Stub implementation for Node.js 'stream' module.
//
// IMPORTANT: Node's `require('stream')` returns the legacy `Stream`
// *constructor* (a class) with `Readable`/`Writable`/etc. attached as
// static properties — NOT a plain namespace object. Packages like
// `send` / `express` rely on this shape:
//
//     var Stream = require('stream')
//     function SendStream() { Stream.call(this) }
//     util.inherits(SendStream, Stream)   // reads Stream.prototype
//
// If the default export is `{ Readable, Writable, ... }` then
// `Stream.prototype` is `undefined` and `util.inherits` blows up with
// "Object prototype may only be an Object or null: undefined".
// (See: node_modules/send/index.js:30,173.)
//
// So we make `Stream` a real class, attach the sub-classes as static
// properties, and export the *class itself* as default.
class Stream {
    constructor() {}
    pipe(dest) { return dest; }
    on() { return this; }
    once() { return this; }
    emit() { return false; }
    off() { return this; }
    addListener() { return this; }
    removeListener() { return this; }
    removeAllListeners() { return this; }
}
export class Readable extends Stream {
    constructor() { super(); }
    read() { return null; }
    pipe(dest) { return dest; }
}
export class Writable extends Stream {
    constructor() { super(); }
    write() { return true; }
    end() {}
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
// Attach sub-classes as static properties so `Stream.Readable`,
// `Stream.Writable`, etc. resolve the way Node ships them.
Stream.Readable = Readable;
Stream.Writable = Writable;
Stream.Duplex = Duplex;
Stream.Transform = Transform;
Stream.PassThrough = PassThrough;
Stream.pipeline = pipeline;
Stream.finished = finished;
export { Stream };
// `__perry_commonjs = true` tells the wrap_commonjs() require() shim in
// modules.rs to return `module.default` instead of the ESM namespace
// when this module is `require()`'d. Node's `require('stream')` returns
// the Stream class itself (with `.Readable` / `.prototype` / etc),
// NOT a namespace object. Without this flag, `var Stream =
// require('stream')` ends up as a copied null-proto object and
// `Stream.prototype` becomes undefined → `util.inherits` crashes.
export const __perry_commonjs = true;
export default Stream;
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
// Node's buffer.constants — pino / thread-stream read MAX_STRING_LENGTH at
// module init time (`const MAX_STRING = buffer.constants.MAX_STRING_LENGTH`).
// Without this, the V8-fallback evaluation throws TypeError at top-level
// and the whole module namespace is lost — surfaces as
// `[js_get_export] failed to get namespace: ...MAX_STRING_LENGTH`.
// Values mirror Node 20+: MAX_LENGTH = 2^53-1, MAX_STRING_LENGTH = 2^29-24.
export const constants = {
    MAX_LENGTH: 9007199254740991,
    MAX_STRING_LENGTH: 536870888,
};
export const kMaxLength = constants.MAX_LENGTH;
export const kStringMaxLength = constants.MAX_STRING_LENGTH;
export default { Buffer, constants, kMaxLength, kStringMaxLength };
"#.to_string(),
        "util" => r#"
// Stub implementation for Node.js 'util' module
export function promisify(fn) { return (...args) => new Promise((resolve, reject) => fn(...args, (err, result) => err ? reject(err) : resolve(result))); }
export function callbackify(fn) { return (...args) => { const cb = args.pop(); fn(...args).then(r => cb(null, r)).catch(cb); }; }
export function inspect(obj) { return JSON.stringify(obj); }
export function format(fmt, ...args) { return fmt; }
// util.formatWithOptions(inspectOptions, format[, ...args]) — identical to
// util.format with the first arg routed into util.inspect for %o/%O. Our
// stub ignores the options object and delegates to format(); full
// options-passthrough is a follow-up. Required by the `debug` npm package.
export function formatWithOptions(_inspectOptions, fmt, ...args) { return format(fmt, ...args); }
export function debuglog() { return () => {}; }
export function deprecate(fn) { return fn; }
// `util.inherits(ctor, superCtor)` — Node's pre-class inheritance helper.
// Real Node semantics:
//   Object.defineProperty(ctor, 'super_', { value: superCtor });
//   Object.setPrototypeOf(ctor.prototype, superCtor.prototype);
// Throws TypeError if either arg is missing a `.prototype`. We mirror that
// contract so packages like `send` (which derives `SendStream` from
// `require('stream')`) work transparently.
export function inherits(ctor, superCtor) {
    if (ctor === undefined || ctor === null) {
        throw new TypeError('The constructor to "inherits" must not be null or undefined');
    }
    if (superCtor === undefined || superCtor === null) {
        throw new TypeError('The super constructor to "inherits" must not be null or undefined');
    }
    if (superCtor.prototype === undefined) {
        throw new TypeError('The super constructor to "inherits" must have a prototype');
    }
    Object.defineProperty(ctor, 'super_', { value: superCtor, writable: true, configurable: true });
    Object.setPrototypeOf(ctor.prototype, superCtor.prototype);
}
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
export default { promisify, callbackify, inspect, format, formatWithOptions, debuglog, deprecate, inherits, TextEncoder, TextDecoder, types };
"#.to_string(),
        "events" => r#"
// Stub implementation for Node.js 'events' module.
// Every method lazy-initializes `_events` so mixin/inherit patterns that
// copy EventEmitter.prototype without invoking the constructor (e.g.
// express's createApplication -> mixin(app, EventEmitter.prototype, false))
// still work. This mirrors Node's real lib/events.js, which does
// `this._events ??= ObjectCreate(null)` inside every method.
function __perry_ee_init(self) { if (!self._events) self._events = Object.create(null); return self._events; }
export class EventEmitter {
    constructor() { this._events = Object.create(null); }
    on(event, listener) { const e = __perry_ee_init(this); (e[event] = e[event] || []).push(listener); return this; }
    addListener(event, listener) { return this.on(event, listener); }
    once(event, listener) { __perry_ee_init(this); const wrapped = (...args) => { this.off(event, wrapped); listener(...args); }; return this.on(event, wrapped); }
    off(event, listener) { const e = __perry_ee_init(this); const arr = e[event]; if (arr) { const i = arr.indexOf(listener); if (i >= 0) arr.splice(i, 1); } return this; }
    removeListener(event, listener) { return this.off(event, listener); }
    emit(event, ...args) { const e = __perry_ee_init(this); const arr = e[event]; if (arr) arr.slice().forEach(fn => fn.apply(this, args)); return !!arr; }
    removeAllListeners(event) { const e = __perry_ee_init(this); if (event) delete e[event]; else this._events = Object.create(null); return this; }
    prependListener(event, listener) { const e = __perry_ee_init(this); (e[event] = e[event] || []).unshift(listener); return this; }
    prependOnceListener(event, listener) { __perry_ee_init(this); const wrapped = (...args) => { this.off(event, wrapped); listener(...args); }; return this.prependListener(event, wrapped); }
    listeners(event) { const e = __perry_ee_init(this); return (e[event] || []).slice(); }
    listenerCount(event) { const e = __perry_ee_init(this); return (e[event] || []).length; }
    eventNames() { const e = __perry_ee_init(this); return Object.keys(e); }
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
// Lightweight implementation for Node.js 'async_hooks' module.
// This is intentionally self-contained because built-in modules are loaded as
// synthetic ESM sources by perry-jsruntime. It models the public lifecycle
// enough for tracers that use createHook(), AsyncResource, and async ids.
let __perryNextAsyncId = 1;
let __perryExecutionAsyncId = 0;
let __perryTriggerAsyncId = 0;
let __perryInHookCallback = false;
const __perryExecutionStack = [];
const __perryHooks = [];

function __perryEnabledHooks() {
    return __perryHooks.filter((hook) => hook && hook.enabled);
}

function __perryEmit(name, ...args) {
    if (__perryInHookCallback) return;
    const enabled = __perryEnabledHooks();
    if (enabled.length === 0) return;
    __perryInHookCallback = true;
    try {
        for (const hook of enabled) {
            const cb = hook.callbacks && hook.callbacks[name];
            if (typeof cb === "function") cb(...args);
        }
    } finally {
        __perryInHookCallback = false;
    }
}

function __perryEnter(asyncId, triggerAsyncId) {
    __perryExecutionStack.push([__perryExecutionAsyncId, __perryTriggerAsyncId]);
    __perryExecutionAsyncId = asyncId;
    __perryTriggerAsyncId = triggerAsyncId;
    __perryEmit("before", asyncId);
}

function __perryLeave(asyncId) {
    try {
        __perryEmit("after", asyncId);
    } finally {
        const previous = __perryExecutionStack.pop() || [0, 0];
        __perryExecutionAsyncId = previous[0];
        __perryTriggerAsyncId = previous[1];
    }
}

function __perryAllocateResource(type, resource, triggerAsyncId = __perryExecutionAsyncId) {
    const asyncId = __perryNextAsyncId++;
    __perryEmit("init", asyncId, String(type || "AsyncResource"), triggerAsyncId, resource);
    return { asyncId, triggerAsyncId, destroyed: false };
}

function __perryDestroy(state) {
    if (!state || state.destroyed) return;
    state.destroyed = true;
    __perryEmit("destroy", state.asyncId);
}

function __perryWrapCallback(type, callback) {
    if (typeof callback !== "function") return callback;
    const state = __perryAllocateResource(type, callback);
    return function (...args) {
        __perryEnter(state.asyncId, state.triggerAsyncId);
        try {
            return callback.apply(this, args);
        } finally {
            __perryLeave(state.asyncId);
            __perryDestroy(state);
        }
    };
}

export class AsyncResource {
    constructor(type, options = {}) {
        const triggerAsyncId = options && Object.prototype.hasOwnProperty.call(options, "triggerAsyncId")
            ? Number(options.triggerAsyncId)
            : __perryExecutionAsyncId;
        this.__perryAsyncState = __perryAllocateResource(type || "AsyncResource", this, triggerAsyncId);
    }
    runInAsyncScope(fn, thisArg, ...args) {
        const state = this.__perryAsyncState;
        __perryEnter(state.asyncId, state.triggerAsyncId);
        try { return fn.apply(thisArg, args); }
        finally { __perryLeave(state.asyncId); }
    }
    emitDestroy() { __perryDestroy(this.__perryAsyncState); return this; }
    asyncId() { return this.__perryAsyncState.asyncId; }
    triggerAsyncId() { return this.__perryAsyncState.triggerAsyncId; }
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

export function executionAsyncId() { return __perryExecutionAsyncId; }
export function executionAsyncResource() { return {}; }
export function triggerAsyncId() { return __perryTriggerAsyncId; }
export function createHook(callbacks = {}) {
    const hook = {
        callbacks,
        enabled: false,
        enable() {
            if (!__perryHooks.includes(hook)) __perryHooks.push(hook);
            hook.enabled = true;
            return hook;
        },
        disable() { hook.enabled = false; return hook; },
    };
    return hook;
}

const __perryNativeSetTimeout = globalThis.setTimeout;
if (typeof __perryNativeSetTimeout === "function" && !__perryNativeSetTimeout.__perryAsyncHooksWrapped) {
    const wrapped = function (callback, delay, ...args) {
        return __perryNativeSetTimeout.call(this, __perryWrapCallback("Timeout", callback), delay, ...args);
    };
    wrapped.__perryAsyncHooksWrapped = true;
    globalThis.setTimeout = wrapped;
}

const __perryNativeSetImmediate = globalThis.setImmediate;
if (typeof __perryNativeSetImmediate === "function" && !__perryNativeSetImmediate.__perryAsyncHooksWrapped) {
    const wrapped = function (callback, ...args) {
        return __perryNativeSetImmediate.call(this, __perryWrapCallback("Immediate", callback), ...args);
    };
    wrapped.__perryAsyncHooksWrapped = true;
    globalThis.setImmediate = wrapped;
}

if (globalThis.process && typeof globalThis.process.nextTick === "function" && !globalThis.process.nextTick.__perryAsyncHooksWrapped) {
    const nativeNextTick = globalThis.process.nextTick;
    const wrapped = function (callback, ...args) {
        return nativeNextTick.call(this, __perryWrapCallback("TickObject", callback), ...args);
    };
    wrapped.__perryAsyncHooksWrapped = true;
    globalThis.process.nextTick = wrapped;
}

const __perryNativePromise = globalThis.Promise;
if (typeof __perryNativePromise === "function" && !__perryNativePromise.__perryAsyncHooksWrapped) {
    class PerryAsyncHookPromise extends __perryNativePromise {
        constructor(executor) {
            let state;
            super((resolve, reject) => {
                state = __perryAllocateResource("PROMISE", undefined);
                const settle = (fn) => (value) => {
                    if (!state.destroyed) {
                        __perryEmit("promiseResolve", state.asyncId);
                        __perryDestroy(state);
                    }
                    return fn(value);
                };
                return executor(settle(resolve), settle(reject));
            });
            this.__perryAsyncState = state;
        }
        static get [Symbol.species]() { return __perryNativePromise; }
    }
    PerryAsyncHookPromise.__perryAsyncHooksWrapped = true;
    globalThis.Promise = PerryAsyncHookPromise;
}

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
