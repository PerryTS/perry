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
            wrap_commonjs(&code, Some(&path))
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
fn wrap_commonjs(code: &str, self_path: Option<&Path>) -> String {
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
fn scan_template_literal(bytes: &[u8], start: usize) -> usize {
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

fn strip_js_comments(code: &str) -> String {
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
fn classify_require_specs(code: &str) -> Vec<(String, bool)> {
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
        //
        // Critical ordering for the express + native-fetch self-call
        // pattern (#997): the listening callback is invoked AFTER the
        // accept loop has registered its first `op_perry_http_accept`
        // op. This matters because user callbacks compiled from Perry
        // TypeScript come in as native trampolines — the V8 thread is
        // blocked synchronously while the callback's native body runs,
        // and any `await` inside the body busy-waits on the V8 thread
        // (pumping `js_run_jsruntime_pump` inside the wait so microtasks
        // still progress). If we called `cb()` BEFORE the accept loop
        // queued its op, an `await fetch('http://127.0.0.1:port/...')`
        // inside cb would block waiting for the server to respond, but
        // the server can't dispatch because the accept loop hasn't
        // started — three-way deadlock against the synchronous trampoline.
        //
        // Scheduling cb on Promise.resolve().then() defers it past the
        // first `await op_perry_http_accept(...)`, so by the time the
        // trampoline starts blocking the V8 thread there's already a
        // pending accept op the inner pump can drive to resolution
        // when hyper hands a request through the mpsc.
        (async () => {
            try {
                const sid = await __perry_ops.op_perry_http_listen(port, host);
                self._serverId = sid;
                self._listening = true;
                self._listenAddress = { port, host };
                self._emit('listening');
                if (typeof cb === 'function') {
                    // Defer to a microtask so the accept loop below
                    // registers `op_perry_http_accept` first.
                    Promise.resolve().then(() => { try { cb(); } catch (_) {} });
                }
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
// `crypto.createHash(algorithm)` — real digest for sha1/sha256/sha384/
// sha512/md5 via `op_perry_hash`. Mirrors the createHmac path: accumulate
// `update()` inputs into a single Uint8Array on `digest()` to match
// Node's API shape. Used by NestJS's `ModuleTokenFactory.hashString`
// and `fast-safe-stringify` (`safeStableStringify` token hashing) so
// every module token comes back unique instead of all hashing to
// `""`. (#1021.)
export function createHash(algorithm) {
    const normalize = (a) => {
        if (typeof a !== 'string') return null;
        const x = a.toLowerCase();
        if (x === 'sha1' || x === 'sha-1') return 'sha1';
        if (x === 'sha256' || x === 'sha-256') return 'sha256';
        if (x === 'sha384' || x === 'sha-384') return 'sha384';
        if (x === 'sha512' || x === 'sha-512') return 'sha512';
        if (x === 'md5') return 'md5';
        return null;
    };
    const toBytes = (input) => {
        if (input == null) return new Uint8Array(0);
        if (input instanceof Uint8Array) return input;
        if (input instanceof ArrayBuffer) return new Uint8Array(input);
        if (ArrayBuffer.isView(input)) {
            return new Uint8Array(input.buffer, input.byteOffset, input.byteLength);
        }
        if (typeof input === 'string') {
            return new TextEncoder().encode(input);
        }
        return new Uint8Array(0);
    };
    const concat = (chunks) => {
        let total = 0;
        for (const c of chunks) total += c.length;
        const out = new Uint8Array(total);
        let off = 0;
        for (const c of chunks) { out.set(c, off); off += c.length; }
        return out;
    };
    const toHex = (bytes) => {
        let s = '';
        for (let i = 0; i < bytes.length; i++) {
            s += (bytes[i] + 0x100).toString(16).slice(1);
        }
        return s;
    };
    const toBase64 = (bytes) => {
        if (typeof Buffer !== 'undefined' && Buffer.from) {
            return Buffer.from(bytes).toString('base64');
        }
        let s = '';
        for (let i = 0; i < bytes.length; i++) s += String.fromCharCode(bytes[i]);
        return (typeof btoa === 'function') ? btoa(s) : '';
    };
    const alg = normalize(algorithm);
    const ops = (typeof Deno !== 'undefined' && Deno.core && Deno.core.ops) ? Deno.core.ops : null;
    const chunks = [];
    let finalized = false;
    return {
        update(data, _enc) {
            if (finalized) {
                throw new Error('Digest already called');
            }
            chunks.push(toBytes(data));
            return this;
        },
        digest(encoding) {
            finalized = true;
            const merged = concat(chunks);
            let out;
            if (alg && ops && typeof ops.op_perry_hash === 'function') {
                out = ops.op_perry_hash(alg, merged);
                if (!(out instanceof Uint8Array)) out = new Uint8Array(out || []);
            } else {
                out = new Uint8Array(0);
            }
            if (!encoding || encoding === 'binary') {
                if (typeof Buffer !== 'undefined' && Buffer.from) {
                    return Buffer.from(out);
                }
                return out;
            }
            if (encoding === 'hex') return toHex(out);
            if (encoding === 'base64') return toBase64(out);
            if (encoding === 'base64url') {
                return toBase64(out)
                    .replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/g, '');
            }
            return toHex(out);
        },
        copy() {
            if (finalized) throw new Error('Digest already called');
            const clone = createHash(algorithm);
            for (const c of chunks) clone.update(c);
            return clone;
        },
    };
}
// `crypto.createHmac(algorithm, key)` — real HMAC for HS256/384/512 via
// `op_perry_hmac`. The op takes a normalized algorithm name plus
// zero-copy Uint8Array buffers for the key/data. We accumulate update()
// inputs into a single Uint8Array on .digest() to match Node's API
// shape (Node's createHmac returns a Hmac that supports chained
// update().update().digest()). Without this, libraries like `jose`
// fall through to an empty signature and produce malformed JWS tokens.
const __perry_hmac_normalize_alg = (algorithm) => {
    if (typeof algorithm !== 'string') return null;
    const a = algorithm.toLowerCase();
    if (a === 'sha256' || a === 'sha-256') return 'sha256';
    if (a === 'sha384' || a === 'sha-384') return 'sha384';
    if (a === 'sha512' || a === 'sha-512') return 'sha512';
    return null;
};
const __perry_hmac_to_bytes = (input) => {
    if (input == null) return new Uint8Array(0);
    if (input instanceof Uint8Array) return input;
    if (input instanceof ArrayBuffer) return new Uint8Array(input);
    if (ArrayBuffer.isView(input)) {
        return new Uint8Array(input.buffer, input.byteOffset, input.byteLength);
    }
    if (typeof input === 'string') {
        return new TextEncoder().encode(input);
    }
    // KeyObject / shim with .export(), ._material, or ._key — best-effort
    // unwrap. Check _material first so we don't recurse through the Buffer
    // returned from export() (which would already be a Uint8Array hit).
    if (input._material instanceof Uint8Array) return input._material;
    if (typeof input.export === 'function') {
        try { return __perry_hmac_to_bytes(input.export()); } catch (_) {}
    }
    if (input._key != null) return __perry_hmac_to_bytes(input._key);
    return new Uint8Array(0);
};
const __perry_hmac_concat = (chunks) => {
    let total = 0;
    for (const c of chunks) total += c.length;
    const out = new Uint8Array(total);
    let off = 0;
    for (const c of chunks) { out.set(c, off); off += c.length; }
    return out;
};
const __perry_hmac_to_hex = (bytes) => {
    let s = '';
    for (let i = 0; i < bytes.length; i++) {
        s += (bytes[i] + 0x100).toString(16).slice(1);
    }
    return s;
};
const __perry_hmac_to_base64 = (bytes) => {
    if (typeof Buffer !== 'undefined' && Buffer.from) {
        return Buffer.from(bytes).toString('base64');
    }
    // Fallback (no Buffer): manual base64.
    let bin = '';
    for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]);
    if (typeof btoa === 'function') return btoa(bin);
    return bin;
};
export function createHmac(algorithm, key) {
    const alg = __perry_hmac_normalize_alg(algorithm);
    const keyBytes = __perry_hmac_to_bytes(key);
    const chunks = [];
    const ops = (typeof Deno !== 'undefined' && Deno.core && Deno.core.ops) ? Deno.core.ops : null;
    return {
        update(data, _inputEncoding) {
            chunks.push(__perry_hmac_to_bytes(data));
            return this;
        },
        digest(encoding) {
            const dataBytes = __perry_hmac_concat(chunks);
            let out;
            if (alg && ops && typeof ops.op_perry_hmac === 'function') {
                out = ops.op_perry_hmac(alg, keyBytes, dataBytes);
                if (!(out instanceof Uint8Array)) out = new Uint8Array(out || []);
            } else {
                out = new Uint8Array(0);
            }
            if (encoding === 'hex') return __perry_hmac_to_hex(out);
            if (encoding === 'base64') return __perry_hmac_to_base64(out);
            if (encoding === 'base64url') {
                return __perry_hmac_to_base64(out).replace(/=+$/, '').replace(/\+/g, '-').replace(/\//g, '_');
            }
            // No encoding → Buffer/Uint8Array. Prefer Node-style Buffer
            // (jose calls `.toString('base64url')` on the result). Buffer
            // extends Uint8Array, so `instanceof Uint8Array` checks still
            // pass on the returned value.
            if (typeof Buffer !== 'undefined' && Buffer.from) {
                return Buffer.from(out);
            }
            return out;
        }
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
// `KeyObject` (V8 fallback shim) — must be a real class so that
// `key instanceof KeyObject` checks in libraries like `jose` succeed.
// `createSecretKey(key, encoding)` returns a KeyObject; for utf8 input
// we encode via TextEncoder so the underlying bytes match what HMAC
// expects (Node's `createSecretKey('secret', 'utf8')` byte-aligns
// with Buffer.from('secret', 'utf8')).
export class KeyObject {
    constructor(opts) {
        opts = opts || {};
        this.type = opts.type || 'secret';
        this.asymmetricKeyType = opts.asymmetricKeyType;
        this.asymmetricKeyDetails = opts.asymmetricKeyDetails;
        this._material = opts._material || new Uint8Array(0);
        this.symmetricKeySize = this._material.length;
    }
    export(options) {
        // Node returns a Buffer for secret keys when no format is given.
        if (typeof Buffer !== 'undefined' && Buffer.from) {
            return Buffer.from(this._material);
        }
        return this._material;
    }
    static from(cryptoKey) {
        return new KeyObject({ type: 'secret', _material: new Uint8Array(0) });
    }
}
export function createSecretKey(key, encoding) {
    let bytes;
    if (key instanceof Uint8Array) {
        bytes = key;
    } else if (typeof key === 'string') {
        // Node accepts a string + encoding; only utf8 is common.
        if (encoding && encoding !== 'utf8' && encoding !== 'utf-8') {
            // Best-effort: hex / base64 decoders are not free here. Fall
            // back to utf8 bytes so the caller still gets a valid key.
        }
        bytes = new TextEncoder().encode(key);
    } else if (key && key.buffer instanceof ArrayBuffer) {
        bytes = new Uint8Array(key.buffer, key.byteOffset || 0, key.byteLength);
    } else {
        bytes = new Uint8Array(0);
    }
    return new KeyObject({ type: 'secret', _material: bytes });
}
export function createPrivateKey() { throw new Error('crypto.createPrivateKey not supported in V8 fallback'); }
export function createPublicKey() { throw new Error('crypto.createPublicKey not supported in V8 fallback'); }
export function generateKeyPair() { throw new Error('crypto.generateKeyPair not supported in V8 fallback'); }
export function diffieHellman() { throw new Error('crypto.diffieHellman not supported in V8 fallback'); }
export function publicEncrypt() { throw new Error('crypto.publicEncrypt not supported in V8 fallback'); }
export function privateDecrypt() { throw new Error('crypto.privateDecrypt not supported in V8 fallback'); }
export function getCiphers() { return []; }
export const constants = {
    RSA_PKCS1_PADDING: 1,
    RSA_PKCS1_OAEP_PADDING: 4,
    RSA_PSS_SALTLEN_DIGEST: -1,
    RSA_PSS_SALTLEN_MAX_SIGN: -2,
    RSA_PSS_SALTLEN_AUTO: -2,
};
// `crypto.webcrypto` — Node exposes the Web Crypto API namespace here.
// jose's `webcrypto.js` reads `crypto.webcrypto.CryptoKey`; returning a
// minimal object with the same surface lets ESM linking succeed and
// makes `crypto.webcrypto?.CryptoKey` resolve to `undefined` rather
// than crash with a TypeError.
export const webcrypto = (typeof globalThis !== 'undefined' && globalThis.crypto)
    ? globalThis.crypto
    : { subtle: {}, getRandomValues(arr) { return arr; } };
// `crypto.sign(algorithm, data, key, callback)` — Node's one-shot
// signer. jose's `runtime/sign.js` does `promisify(crypto.sign)` and
// only calls the resulting Promise variant for non-HS algorithms.
// Throwing here keeps the V8 stub deterministic: HS* never touches
// `crypto.sign`, and asymmetric signing is unsupported on this path.
export function sign(algorithm, data, key, callback) {
    const err = new Error('crypto.sign (asymmetric) not supported in V8 fallback');
    if (typeof callback === 'function') {
        callback(err);
        return;
    }
    throw err;
}
export function verify() { throw new Error('crypto.verify (asymmetric) not supported in V8 fallback'); }
export default { randomBytes, randomFillSync, randomUUID, createHash, createHmac, pbkdf2Sync, pbkdf2, timingSafeEqual, createCipheriv, createDecipheriv, createSecretKey, createPrivateKey, createPublicKey, generateKeyPair, diffieHellman, publicEncrypt, privateDecrypt, getCiphers, KeyObject, constants, webcrypto, sign, verify };
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
// `_isKeyObjectShape` — duck-types the `node:crypto` `KeyObject` shim
// without importing it (avoids a circular module dependency between
// node:util and node:crypto). Real Node uses an internal class brand;
// for our V8 fallback path the shape check is sufficient because
// libraries (jose) only ever produce KeyObjects via `createSecretKey`
// or `KeyObject.from`, both of which set `type: 'secret' | ...`
// alongside a typed-array `_material` field.
const _isKeyObjectShape = (v) => (
    v != null && typeof v === 'object'
    && typeof v.type === 'string'
    && (v._material instanceof Uint8Array || v._key != null
        || typeof v.export === 'function')
);
const _isCryptoKeyShape = (v) => (
    v != null && typeof v === 'object'
    && typeof v.algorithm === 'object'
    && typeof v.type === 'string'
    && typeof v.extractable === 'boolean'
);
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
    // `util.types.isKeyObject` / `isCryptoKey` are required by `jose`
    // (and other JWT/JOSE libs) to discriminate between Uint8Array,
    // CryptoKey, and Node's KeyObject before signing/verifying. The
    // V8 fallback path doesn't expose a real `internalBinding('util')`
    // so we duck-type against the shape produced by our crypto shim.
    isKeyObject: (v) => _isKeyObjectShape(v),
    isCryptoKey: (v) => _isCryptoKeyShape(v),
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
        "perf_hooks" => r#"
// Stub implementation for Node.js 'perf_hooks' module. NestJS
// (`@nestjs/core/injector/module-token-factory.js`) reaches into
// `performance.now()` during dynamic-module compile; without this stub
// the import resolves to `{}` and `performance.now` throws. Minimum
// viable surface — just enough to keep NestJS's serialization timer
// running. (#1021.)
export const performance = {
    now: () => (typeof Date !== 'undefined' ? Date.now() : 0),
    timeOrigin: 0,
    mark() {},
    measure() {},
    clearMarks() {},
    clearMeasures() {},
    getEntries: () => [],
    getEntriesByName: () => [],
    getEntriesByType: () => [],
    nodeTiming: {},
    timerify: (fn) => fn,
    eventLoopUtilization: () => ({ idle: 0, active: 0, utilization: 0 }),
};
export class PerformanceObserver {
    constructor() {}
    observe() {}
    disconnect() {}
    takeRecords() { return []; }
}
PerformanceObserver.supportedEntryTypes = [];
export const constants = {};
export const monitorEventLoopDelay = () => ({ enable() {}, disable() {}, reset() {} });
export default { performance, PerformanceObserver, constants, monitorEventLoopDelay };
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
