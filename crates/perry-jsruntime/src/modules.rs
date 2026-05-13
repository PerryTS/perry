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
use std::path::{Path, PathBuf};

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
            if path.exists() {
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
    fn is_node_builtin(specifier: &str) -> bool {
        matches!(
            specifier,
            "net"
                | "tls"
                | "http"
                | "https"
                | "fs"
                | "path"
                | "os"
                | "crypto"
                | "stream"
                | "buffer"
                | "util"
                | "events"
                | "assert"
                | "child_process"
                | "dns"
                | "dgram"
                | "url"
                | "querystring"
                | "string_decoder"
                | "zlib"
                | "readline"
                | "tty"
                | "vm"
                | "worker_threads"
                | "cluster"
                | "async_hooks"
                | "perf_hooks"
                | "trace_events"
                | "inspector"
                | "v8"
                | "node:net"
                | "node:tls"
                | "node:http"
                | "node:https"
                | "node:fs"
                | "node:path"
                | "node:os"
                | "node:crypto"
                | "node:stream"
                | "node:buffer"
                | "node:util"
                | "node:events"
                | "node:assert"
                | "node:child_process"
                | "node:dns"
                | "node:dgram"
                | "node:url"
                | "node:querystring"
                | "node:string_decoder"
                | "node:zlib"
                | "node:readline"
                | "node:tty"
                | "node:vm"
                | "node:worker_threads"
                | "node:cluster"
                | "node:async_hooks"
                | "node:perf_hooks"
                | "node:trace_events"
                | "node:inspector"
                | "node:v8"
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
                if entry_path.exists() {
                    return Ok(entry_path);
                }
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
            let builtin_name = specifier.strip_prefix("node:").unwrap_or(specifier);
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
                    "Failed to read module: {}",
                    e
                ))))
            }
        };

        let module_type = self.detect_module_type(&path);

        // Wrap CommonJS modules if needed
        let code = if is_commonjs(&code) {
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
    // Quick heuristics for CommonJS detection
    code.contains("module.exports")
        || code.contains("exports.")
        || (code.contains("require(") && !code.contains("import "))
}

/// Wrap CommonJS code as ESM
fn wrap_commonjs(code: &str) -> String {
    // Extract all require() specifiers so we can convert them to ESM imports
    let require_re = regex::Regex::new(r#"require\s*\(\s*['"]([^'"]+)['"]\s*\)"#).unwrap();
    let mut require_specs: Vec<String> = Vec::new();
    for cap in require_re.captures_iter(code) {
        if let Some(spec) = cap.get(1) {
            let spec_str = spec.as_str().to_string();
            if !require_specs.contains(&spec_str) {
                require_specs.push(spec_str);
            }
        }
    }

    // Generate ESM import statements for each require() specifier
    let imports = require_specs
        .iter()
        .enumerate()
        .map(|(i, spec)| format!("import _req_{} from '{}';", i, spec))
        .collect::<Vec<_>>()
        .join("\n");

    // Generate require() lookup cases
    let require_cases = require_specs
        .iter()
        .enumerate()
        .map(|(i, spec)| format!("        if (specifier === '{}') return _req_{};", spec, i))
        .collect::<Vec<_>>()
        .join("\n");

    // Extract exported names from CommonJS code to properly re-export them
    let mut named_exports = Vec::new();

    // Find exports.X = assignments
    for cap in regex::Regex::new(r"exports\.(\w+)\s*=")
        .unwrap()
        .captures_iter(code)
    {
        if let Some(name) = cap.get(1) {
            let name = name.as_str();
            if name != "__esModule" && !named_exports.contains(&name.to_string()) {
                named_exports.push(name.to_string());
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
            .map(|n| format!("export const {} = _cjs.{};", n, n))
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        r#"{}
const _cjs = (function() {{
    const module = {{ exports: {{}} }};
    const exports = module.exports;
    function require(specifier) {{
{}
        throw new Error('require() is not supported: ' + specifier);
    }}

    {}

    return module.exports;
}})();

export default _cjs;
{}
"#,
        imports, require_cases, code, named_export_decls
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
        "http" | "https" => r#"
// Stub implementation for Node.js http/https module
export class IncomingMessage {}
export class ServerResponse {}
export class Agent {}
export function request() { throw new Error('http.request not supported in this environment'); }
export function get() { throw new Error('http.get not supported in this environment'); }
export function createServer() { throw new Error('http.createServer not supported in this environment'); }
export default { IncomingMessage, ServerResponse, Agent, request, get, createServer };
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
        "stream" => r#"
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
export function pipeline() {}
export function finished() {}
export default { Readable, Writable, Duplex, Transform, PassThrough, pipeline, finished };
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
export default { promisify, callbackify, inspect, format, debuglog, deprecate, inherits, TextEncoder, TextDecoder };
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
        assert!(!is_commonjs("export default {};"));
        assert!(!is_commonjs("import foo from 'bar';"));
    }
}
