//! Node.js-compatible module loader (resolve + load impls).

use super::*;

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
    pub fn is_node_builtin(specifier: &str) -> bool {
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
