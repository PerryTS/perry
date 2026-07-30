//! Dependency checking and validation

use anyhow::Result;
use perry_diagnostics::{Diagnostic, DiagnosticCode, Diagnostics, SourceCache};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Result of scanning a package for compatibility
// #854: analysis record — `path`/`files_checked` are populated for
// diagnostics but not consumed on the current report path.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct PackageCompatibility {
    pub name: String,
    pub version: Option<String>,
    pub path: PathBuf,
    pub is_compatible: bool,
    pub issues: Vec<CompatibilityIssue>,
    pub files_checked: usize,
}

#[derive(Debug, Clone)]
pub struct CompatibilityIssue {
    pub file: PathBuf,
    pub line: Option<u32>,
    pub kind: IssueKind,
    pub message: String,
}

// #854: issue-classification enum; `DynamicPropertyAccess`/`UnsupportedSyntax`
// are handled in `severity()` but not yet constructed by any scan rule.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueKind {
    /// eval() or new Function() usage
    DynamicCode,
    /// Dynamic import()
    DynamicImport,
    /// Explicit 'any' type
    AnyType,
    /// Dynamic property access with variable key
    DynamicPropertyAccess,
    /// Unsupported syntax
    UnsupportedSyntax,
    /// Missing type declarations
    MissingTypes,
}

impl IssueKind {
    pub fn severity(&self) -> &'static str {
        match self {
            IssueKind::DynamicCode => "error",
            IssueKind::DynamicImport => "error",
            IssueKind::UnsupportedSyntax => "error",
            IssueKind::AnyType => "warning",
            IssueKind::DynamicPropertyAccess => "warning",
            IssueKind::MissingTypes => "warning",
        }
    }
}

/// Dependency resolver that tracks all imports and their resolution status
// #854: `resolved_packages` is populated during resolution but not read back
// on the current path; kept as part of the resolver state.
#[allow(dead_code)]
pub struct DependencyResolver {
    /// Root directory of the project
    project_root: PathBuf,
    /// Cache of resolved packages
    resolved_packages: HashMap<String, PackageCompatibility>,
    /// Unresolved imports (package name -> list of importing files)
    unresolved_imports: HashMap<String, Vec<PathBuf>>,
    /// All import sources encountered
    all_imports: HashSet<String>,
    /// All imports with their file locations
    import_locations: HashMap<String, Vec<PathBuf>>,
}

impl DependencyResolver {
    pub fn new(project_root: PathBuf) -> Self {
        Self {
            project_root,
            resolved_packages: HashMap::new(),
            unresolved_imports: HashMap::new(),
            all_imports: HashSet::new(),
            import_locations: HashMap::new(),
        }
    }

    /// Find node_modules directory
    fn find_node_modules(&self) -> Option<PathBuf> {
        let mut current = self.project_root.clone();
        loop {
            let node_modules = current.join("node_modules");
            if node_modules.exists() && node_modules.is_dir() {
                return Some(node_modules);
            }
            if !current.pop() {
                break;
            }
        }
        None
    }

    /// Resolve a package import to its location
    pub fn resolve_package(&self, package_name: &str) -> Option<PathBuf> {
        let node_modules = self.find_node_modules()?;

        // The package name is the first segment (`lodash/map` -> `lodash`),
        // or the first TWO segments for a scoped specifier
        // (`@acme/toolkit/fs/safe` -> `@acme/toolkit`); the
        // remainder is a subpath resolved against the package's `exports`
        // map at compile time, never a directory under node_modules.
        let base_package = package_base_name(package_name);
        let package_path = node_modules.join(base_package);

        if package_path.exists() {
            Some(package_path)
        } else {
            None
        }
    }

    /// Record an import from a file
    pub fn record_import(&mut self, import_source: &str, importing_file: &Path) {
        self.all_imports.insert(import_source.to_string());

        // Track all import locations
        self.import_locations
            .entry(import_source.to_string())
            .or_default()
            .push(importing_file.to_path_buf());

        // Skip relative imports - those are project files
        if import_source.starts_with('.') {
            return;
        }

        // Node.js built-ins are tracked but not resolved
        if is_node_builtin(import_source) {
            return;
        }

        // Perry built-in modules don't need resolution
        if is_perry_builtin(import_source) {
            return;
        }

        // Try to resolve the package
        if self.resolve_package(import_source).is_none() {
            self.unresolved_imports
                .entry(import_source.to_string())
                .or_default()
                .push(importing_file.to_path_buf());
        }
    }

    /// Get all imports with their locations
    pub fn get_all_imports(&self) -> &HashSet<String> {
        &self.all_imports
    }

    /// Get import locations map
    pub fn get_import_locations(&self) -> &HashMap<String, Vec<PathBuf>> {
        &self.import_locations
    }

    /// Get all unresolved imports
    pub fn get_unresolved_imports(&self) -> &HashMap<String, Vec<PathBuf>> {
        &self.unresolved_imports
    }

    /// Check all dependencies for compatibility
    pub fn check_all_dependencies(
        &mut self,
        source_cache: &mut SourceCache,
    ) -> Result<Vec<PackageCompatibility>> {
        let _node_modules = match self.find_node_modules() {
            Some(nm) => nm,
            None => return Ok(Vec::new()),
        };

        let mut results = Vec::new();

        // Get unique package names from imports
        let packages: HashSet<String> = self
            .all_imports
            .iter()
            .filter(|s| !s.starts_with('.') && !is_node_builtin(s) && !is_perry_builtin(s))
            .map(|s| package_base_name(s).to_string())
            .collect();

        for package_name in packages {
            if let Some(package_path) = self.resolve_package(&package_name) {
                let compat =
                    check_package_compatibility(&package_name, &package_path, source_cache)?;
                results.push(compat);
            }
        }

        Ok(results)
    }
}

/// Extract the npm package name from an import specifier: the first
/// path segment, or the first two for a scoped package
/// (`@scope/name/sub/path` -> `@scope/name`).
fn package_base_name(specifier: &str) -> &str {
    let mut slashes = specifier.match_indices('/');
    let cut = if specifier.starts_with('@') {
        slashes.nth(1)
    } else {
        slashes.next()
    };
    match cut {
        Some((idx, _)) => &specifier[..idx],
        None => specifier,
    }
}

/// Check if an import is a Node.js built-in module
fn is_node_builtin(name: &str) -> bool {
    let builtins = [
        "assert",
        "async_hooks",
        "buffer",
        "child_process",
        "cluster",
        "console",
        "constants",
        "crypto",
        "dgram",
        "diagnostics_channel",
        "dns",
        "domain",
        "events",
        "fs",
        "http",
        "http2",
        "https",
        "inspector",
        "module",
        "net",
        "os",
        "path",
        "perf_hooks",
        "process",
        "punycode",
        "querystring",
        "readline",
        "repl",
        "sea",
        "sqlite",
        "stream",
        "string_decoder",
        "sys",
        "test",
        "timers",
        "tls",
        "trace_events",
        "tty",
        "url",
        "util",
        "v8",
        "vm",
        "wasi",
        "worker_threads",
        "zlib",
    ];

    let base = name.split('/').next().unwrap_or(name);
    let base = base.strip_prefix("node:").unwrap_or(base);
    builtins.contains(&base)
}

/// Check if an import is a Perry built-in module
fn is_perry_builtin(name: &str) -> bool {
    name.starts_with("perry/")
}

/// Node.js built-ins that Perry compiles and runs successfully — either
/// via a `perry-stdlib` module (`events`, `http`, `crypto`, `readline`,
/// `streams`, `net`, `worker_threads`, `zlib`) or via direct codegen
/// support in the compiler (`fs`, `path`, `os`, `util`, `process`,
/// `buffer`, `console`, `perf_hooks`, `timers`, `url`, `querystring`,
/// `assert`, `stream`, `tls`, `https`, `tty`, etc.).
///
/// Issue #419: pre-fix, `check --check-deps` flagged every Node built-in
/// as `U006: cannot be used in native compilation` regardless of
/// whether `perry compile` would in fact build and run the program.
/// MedusaJS-class real codebases reported 8+ false positives. Verified
/// with a synthetic `import * as m from "<builtin>"; console.log(typeof m)`
/// per builtin: every name in the Node `is_node_builtin` list compiles
/// to a runnable binary on current `perry`.
///
/// Names that are *known stubs* (cluster, child_process, dgram, dns,
/// domain, repl, punycode, string_decoder, sys, v8, vm, constants,
/// module) compile but expose limited functionality at runtime —
/// they're included in this allowlist so the static `--check-deps`
/// signal doesn't false-positive on them either, matching the
/// "compiles ⇒ no U006" contract from the issue. Their runtime stubs
/// are still tracked separately (via per-module gap-tests).
fn is_supported_node_builtin(name: &str) -> bool {
    let base = name.split('/').next().unwrap_or(name);
    let base = base.strip_prefix("node:").unwrap_or(base);
    if base == "sea" {
        return name == "node:sea";
    }
    matches!(
        base,
        // Real implementations
        "crypto" | "events" | "http" | "http2" | "https" | "net" | "readline"
        | "stream" | "streams" | "worker_threads" | "zlib"
        | "fs" | "path" | "os" | "util" | "process" | "buffer"
        | "console" | "perf_hooks" | "timers" | "url" | "querystring"
        | "tls" | "tty" | "assert" | "diagnostics_channel" | "sqlite"
        // Stubs (compile + import but functionality limited)
        | "cluster" | "child_process" | "dgram" | "dns" | "domain"
        | "repl" | "punycode" | "string_decoder" | "sys" | "v8" | "vm"
        | "constants" | "module" | "async_hooks" | "test" | "trace_events"
        | "wasi" // NOTE: `inspector`/`inspector/promises` are intentionally
                 // absent — `perry compile` rejects them, so `perry check` must surface
                 // the U006 diagnostic rather than report a clean build (#3744).
    )
}

/// Does this package ship TypeScript declarations?
///
/// The old test probed three hardcoded paths (`index.d.ts`,
/// `dist/index.d.ts`, `types/`) and ignored the *canonical* mechanism — the
/// `types` / `typings` field in package.json, and the `types` condition inside
/// an `exports` map. Packages that declare types anywhere else were reported
/// as untyped: `date-fns` (`./typings.d.ts`), `tldts`
/// (`dist/types/index.d.ts`) and `@inquirer/*` (`./dist/cjs/types/index.d.ts`)
/// all tripped it in the Vercel CLI corpus.
///
/// Resolution order:
///   1. `types` / `typings` in package.json (resolved relative to the package,
///      accepting the extensionless form npm also allows).
///   2. Any `"types"` key appearing in the `exports` map (nested conditions
///      included) — checked by key presence, since exports targets can be
///      arbitrarily nested per subpath/condition.
///   3. The legacy hardcoded layouts.
///   4. A bounded scan for any `.d.ts` in the package root or `dist/`, which
///      covers hand-rolled layouts without walking a huge tree.
fn package_declares_types(package_path: &Path) -> bool {
    let manifest = package_path.join("package.json");
    if let Ok(content) = fs::read_to_string(&manifest) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            for key in ["types", "typings"] {
                if let Some(rel) = json.get(key).and_then(|v| v.as_str()) {
                    let rel = rel.trim_start_matches("./");
                    let direct = package_path.join(rel);
                    if direct.exists() {
                        return true;
                    }
                    // npm allows the extensionless form (`"types": "./index"`).
                    if direct.extension().is_none()
                        && package_path.join(format!("{rel}.d.ts")).exists()
                    {
                        return true;
                    }
                }
            }
            if let Some(exports) = json.get("exports") {
                if exports_mentions_types(exports) {
                    return true;
                }
            }
        }
    }

    if package_path.join("index.d.ts").exists()
        || package_path.join("dist").join("index.d.ts").exists()
        || package_path.join("types").exists()
    {
        return true;
    }

    for dir in [package_path.to_path_buf(), package_path.join("dist")] {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if entry
                    .file_name()
                    .to_str()
                    .is_some_and(|n| n.ends_with(".d.ts"))
                {
                    return true;
                }
            }
        }
    }

    false
}

/// Does an `exports` map declare a `types` condition anywhere within it?
fn exports_mentions_types(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(map) => map
            .iter()
            .any(|(k, v)| k == "types" || exports_mentions_types(v)),
        serde_json::Value::Array(items) => items.iter().any(exports_mentions_types),
        _ => false,
    }
}

/// Check a package for compatibility issues
pub fn check_package_compatibility(
    package_name: &str,
    package_path: &Path,
    _source_cache: &mut SourceCache,
) -> Result<PackageCompatibility> {
    let mut issues = Vec::new();
    let mut files_checked = 0;

    // Read package.json for version info
    let package_json_path = package_path.join("package.json");
    let version = if package_json_path.exists() {
        let content = fs::read_to_string(&package_json_path)?;
        extract_version(&content)
    } else {
        None
    };

    // Check if types are available
    let has_types = package_declares_types(package_path);

    if !has_types {
        // Check for @types package
        let _types_package = format!("@types/{}", package_name.replace('/', "__"));
        let node_modules = package_path.parent().unwrap();
        let types_path = node_modules
            .join("@types")
            .join(package_name.replace('/', "__"));

        if !types_path.exists() {
            issues.push(CompatibilityIssue {
                file: package_path.to_path_buf(),
                line: None,
                kind: IssueKind::MissingTypes,
                message: format!(
                    "No type declarations found. Install @types/{} or ensure the package includes types.",
                    package_name.replace('/', "__")
                ),
            });
        }
    }

    // Scan TypeScript/JavaScript files for compatibility issues
    for entry in WalkDir::new(package_path)
        .follow_links(false)
        .max_depth(5) // Limit depth to avoid huge packages
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        // Skip node_modules within the package
        if path.components().any(|c| c.as_os_str() == "node_modules") {
            continue;
        }

        // Check .ts, .js, .mjs files
        let ext = path.extension().and_then(|e| e.to_str());
        if !matches!(ext, Some("ts") | Some("js") | Some("mjs")) {
            continue;
        }

        // Skip declaration files for scanning (they don't have runtime code)
        if path.to_string_lossy().ends_with(".d.ts") {
            continue;
        }

        let source = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        files_checked += 1;

        // Quick pattern-based scanning for problematic constructs
        let file_issues = scan_source_for_issues(path, &source);
        issues.extend(file_issues);
    }

    let is_compatible = !issues.iter().any(|i| i.kind.severity() == "error");

    Ok(PackageCompatibility {
        name: package_name.to_string(),
        version,
        path: package_path.to_path_buf(),
        is_compatible,
        issues,
        files_checked,
    })
}

/// Extract version from package.json content
fn extract_version(content: &str) -> Option<String> {
    // Simple extraction without full JSON parsing
    for line in content.lines() {
        if line.contains("\"version\"") {
            if let Some(start) = line.find(": \"") {
                if let Some(end) = line[start + 3..].find('"') {
                    return Some(line[start + 3..start + 3 + end].to_string());
                }
            }
        }
    }
    None
}

/// Line numbers (1-based) holding a genuinely *dynamic* `import(...)` — one
/// whose argument is not a static string literal.
///
/// Two things the old per-line `!line.contains("import('")` test got wrong:
///
///  1. **Multi-line call sites.** Prettier wraps a long specifier onto its own
///     line, so `await import(\n  './x'\n)` has no `import('` on the `import(`
///     line and was reported as a variable path. The identical call written on
///     one line was not — the check was formatting-sensitive rather than
///     argument-sensitive. (Both shapes appear in the same file in the Vercel
///     CLI: `commands/routes/shared.ts:293` vs `:296`.)
///
///  2. **`import(` inside string literals.** A loader script built as a
///     template literal and written to disk for a *child Node process* is not
///     code in this program, but was scanned as if it were
///     (`util/compile-vercel-config.ts:336`).
///
/// Both are fixed by scanning a comment/string-masked copy of the whole source
/// and resolving the argument across newlines. Masking also removes the need
/// for the old `starts_with("//")` guard.
///
/// A template-literal argument stays "dynamic" — same as before — because it
/// may interpolate.
fn dynamic_import_lines(source: &str) -> std::collections::HashSet<u32> {
    use crate::commands::compile::cjs_wrap::detect::strip_comments_and_strings;

    let mut out = std::collections::HashSet::new();

    // The masker returns a same-length copy (code bytes verbatim, comment and
    // string bodies blanked to spaces). If a partially-masked multi-byte char
    // made the lossy UTF-8 conversion change the length, byte offsets no
    // longer line up — fall back to the raw source, which is what the old
    // check scanned anyway.
    let masked = strip_comments_and_strings(source);
    let scan: &str = if masked.len() == source.len() {
        &masked
    } else {
        source
    };
    let bytes = scan.as_bytes();
    let src_bytes = source.as_bytes();

    // Newlines inside a masked string literal are blanked to spaces, so
    // restore them positionally to keep line numbering aligned with `source`.
    let mut scan_owned = bytes.to_vec();
    if scan_owned.len() == src_bytes.len() {
        for (i, b) in src_bytes.iter().enumerate() {
            if *b == b'\n' {
                scan_owned[i] = b'\n';
            }
        }
    }
    let bytes = &scan_owned[..];

    let is_ident = |c: u8| c == b'_' || c == b'$' || c.is_ascii_alphanumeric();

    let mut line = 1u32;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            line += 1;
            i += 1;
            continue;
        }
        if bytes[i] != b'i' || !scan_owned[i..].starts_with(b"import") {
            i += 1;
            continue;
        }
        // Whole-word `import` only.
        if i > 0 && is_ident(bytes[i - 1]) {
            i += 1;
            continue;
        }
        let mut p = i + "import".len();
        // `import.meta` and static `import x from '…'` both fail the `(` test
        // below, so they need no special case.
        while p < bytes.len() && (bytes[p] as char).is_whitespace() {
            p += 1;
        }
        if p >= bytes.len() || bytes[p] != b'(' {
            i += 1;
            continue;
        }
        // Resolve the argument, crossing newlines.
        //
        // Read it from the ORIGINAL source: the masker blanks string
        // delimiters as well as their bodies, so `import('./x')` is
        // `import(      )` in the masked copy and every call site would look
        // variable. Offsets are aligned (same length), so `p` indexes both.
        p += 1;
        while p < src_bytes.len() && (src_bytes[p] as char).is_whitespace() {
            p += 1;
        }
        let static_literal = p < src_bytes.len() && (src_bytes[p] == b'\'' || src_bytes[p] == b'"');
        if !static_literal {
            out.insert(line);
        }
        i += "import".len();
    }

    out
}

/// Scan source code for compatibility issues using pattern matching
fn scan_source_for_issues(path: &Path, source: &str) -> Vec<CompatibilityIssue> {
    let mut issues = Vec::new();
    let dynamic_imports = dynamic_import_lines(source);

    for (line_num, line) in source.lines().enumerate() {
        let line_num = (line_num + 1) as u32;

        // Check for eval()
        if line.contains("eval(") && !line.trim().starts_with("//") && !line.trim().starts_with("*")
        {
            issues.push(CompatibilityIssue {
                file: path.to_path_buf(),
                line: Some(line_num),
                kind: IssueKind::DynamicCode,
                message: "eval() cannot be compiled to native code".to_string(),
            });
        }

        // Check for new Function()
        if line.contains("new Function(") && !line.trim().starts_with("//") {
            issues.push(CompatibilityIssue {
                file: path.to_path_buf(),
                line: Some(line_num),
                kind: IssueKind::DynamicCode,
                message: "new Function() cannot be compiled to native code".to_string(),
            });
        }

        // Dynamic import() — resolved whole-source in `dynamic_import_lines`
        // so multi-line call sites and `import(` inside string literals are
        // handled correctly.
        if dynamic_imports.contains(&line_num) {
            issues.push(CompatibilityIssue {
                file: path.to_path_buf(),
                line: Some(line_num),
                kind: IssueKind::DynamicImport,
                message: "Dynamic import() with variable path cannot be compiled".to_string(),
            });
        }

        // Check for explicit 'any' type (in .ts files)
        if path.extension().is_some_and(|e| e == "ts")
            && (line.contains(": any") || line.contains(":any") || line.contains("<any>"))
            && !line.trim().starts_with("//")
        {
            issues.push(CompatibilityIssue {
                file: path.to_path_buf(),
                line: Some(line_num),
                kind: IssueKind::AnyType,
                message: "'any' type may cause runtime issues in native compilation".to_string(),
            });
        }
    }

    issues
}

/// Create diagnostics from unresolved imports
pub fn unresolved_imports_to_diagnostics(
    unresolved: &HashMap<String, Vec<PathBuf>>,
    _source_cache: &SourceCache,
) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();

    for (package, files) in unresolved {
        let file_list = files
            .iter()
            .map(|p| p.display().to_string())
            .take(3)
            .collect::<Vec<_>>()
            .join(", ");

        let message = if is_node_builtin(package) {
            format!(
                "Node.js built-in '{}' is not supported in native compilation",
                package
            )
        } else {
            format!(
                "Package '{}' not found in node_modules (imported from: {})",
                package, file_list
            )
        };

        diagnostics.push(
            Diagnostic::error(DiagnosticCode::UnresolvedImport, message)
                .with_help(format!("Install the package with: npm install {}", package))
                .build(),
        );
    }

    diagnostics
}

/// Check for Node.js built-in imports and create diagnostics
pub fn check_node_builtin_imports(
    all_imports: &HashSet<String>,
    import_locations: &HashMap<String, Vec<PathBuf>>,
) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();

    for import in all_imports {
        if is_node_builtin(import)
            && !is_perry_builtin(import)
            && !is_supported_node_builtin(import)
        {
            let files = import_locations
                .get(import)
                .map(|f| {
                    f.iter()
                        .map(|p| {
                            p.file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string()
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_else(|| "unknown".to_string());

            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::UnsupportedFeature,
                    format!(
                        "Node.js built-in module '{}' cannot be used in native compilation (imported in: {})",
                        import, files
                    ),
                )
                .with_help(
                    "Native compilation does not support Node.js runtime APIs. \
                     Consider using a pure TypeScript implementation or removing this dependency."
                )
                .build(),
            );
        }
    }

    diagnostics
}

/// Scan a source file for compatibility issues (for project files, not just packages)
pub fn scan_project_file_for_issues(path: &Path, source: &str) -> Vec<CompatibilityIssue> {
    scan_source_for_issues(path, source)
}

/// Create diagnostics from package compatibility issues
pub fn compatibility_to_diagnostics(packages: &[PackageCompatibility]) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();

    for package in packages {
        for issue in &package.issues {
            let code = match issue.kind {
                IssueKind::DynamicCode => DiagnosticCode::EvalUsage,
                IssueKind::DynamicImport => DiagnosticCode::DynamicImport,
                IssueKind::AnyType => DiagnosticCode::AnyTypeUsage,
                IssueKind::DynamicPropertyAccess => DiagnosticCode::DynamicPropertyAccess,
                IssueKind::UnsupportedSyntax => DiagnosticCode::UnsupportedFeature,
                IssueKind::MissingTypes => DiagnosticCode::MissingTypeAnnotation,
            };

            let severity_fn = if issue.kind.severity() == "error" {
                Diagnostic::error
            } else {
                Diagnostic::warning
            };

            let location = if let Some(line) = issue.line {
                format!(" ({}:{})", issue.file.display(), line)
            } else {
                String::new()
            };

            diagnostics.push(
                severity_fn(
                    code,
                    format!(
                        "[{}@{}] {}{}",
                        package.name,
                        package.version.as_deref().unwrap_or("?"),
                        issue.message,
                        location
                    ),
                )
                .build(),
            );
        }
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scoped specifier's package name is the first TWO segments; the
    /// remainder is an `exports` subpath, not a node_modules directory
    /// (`@acme/toolkit/fs/safe` was reported as R003 "package not
    /// found" because the full specifier was joined as a path).
    #[test]
    fn package_base_name_splits_scoped_subpath_exports() {
        assert_eq!(package_base_name("@acme/toolkit/fs/safe"), "@acme/toolkit");
        assert_eq!(package_base_name("@acme/toolkit"), "@acme/toolkit");
        assert_eq!(package_base_name("@scope/name/a/b/c"), "@scope/name");
        assert_eq!(package_base_name("lodash/map"), "lodash");
        assert_eq!(package_base_name("lodash"), "lodash");
    }

    /// The R003 path end-to-end: `record_import` resolves a scoped subpath
    /// specifier through `resolve_package`, which must look up
    /// `node_modules/@scope/pkg` — not join the full specifier as a
    /// directory — so an installed package's subpath export is never
    /// reported unresolved.
    #[test]
    fn scoped_subpath_import_resolves_against_installed_package() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pkg_dir = dir.path().join("node_modules/@scope/pkg");
        std::fs::create_dir_all(&pkg_dir).expect("create @scope/pkg");
        std::fs::write(
            pkg_dir.join("package.json"),
            "{\"name\":\"@scope/pkg\",\"version\":\"1.0.0\"}\n",
        )
        .expect("write package.json");

        let mut resolver = DependencyResolver::new(dir.path().to_path_buf());
        assert_eq!(
            resolver.resolve_package("@scope/pkg/sub/path"),
            Some(pkg_dir),
            "subpath specifier resolves to the installed package directory"
        );

        resolver.record_import("@scope/pkg/sub/path", Path::new("src/main.ts"));
        assert!(
            resolver.get_unresolved_imports().is_empty(),
            "installed scoped subpath must not be reported as R003 unresolved: {:?}",
            resolver.get_unresolved_imports()
        );
    }

    /// D005 must key off the *argument*, not the line's formatting. Both call
    /// shapes below appear in the same file in the Vercel CLI
    /// (`commands/routes/shared.ts:293` and `:296`); only the prettier-wrapped
    /// one was reported.
    #[test]
    fn dynamic_import_ignores_multiline_static_specifier() {
        let src = "const { default: a } = await import(\n  './util/a'\n);\n\
                   const { default: b } = await import('./util/b');\n\
                   const { c } = await import(\n  \"./util/c\"\n);\n";
        assert!(
            dynamic_import_lines(src).is_empty(),
            "all three specifiers are static string literals; got: {:?}",
            dynamic_import_lines(src)
        );
    }

    /// A genuinely variable specifier must still be flagged, on the right line.
    #[test]
    fn dynamic_import_still_flags_variable_specifier() {
        let src = "const p = compute();\n\
                   const m = await import(p);\n\
                   const n = await import('./static');\n";
        let lines = dynamic_import_lines(src);
        assert_eq!(
            lines.len(),
            1,
            "exactly one dynamic site expected, got: {:?}",
            lines
        );
        assert!(lines.contains(&2), "expected line 2, got: {:?}", lines);
    }

    /// `import(` inside a string literal is not code in this program. The
    /// Vercel CLI builds a loader script as a template literal and writes it
    /// to disk for a child Node process
    /// (`util/compile-vercel-config.ts:336`).
    #[test]
    fn dynamic_import_ignores_import_inside_string_literal() {
        let src = "const loaderScript = `\n\
                   \x20 import { pathToFileURL } from 'url';\n\
                   \x20 const mod = await import(pathToFileURL(process.argv[2]).href);\n\
                   `;\n\
                   await writeFile(loaderPath, loaderScript, 'utf-8');\n";
        assert!(
            dynamic_import_lines(src).is_empty(),
            "import() inside a template literal must not be scanned; got: {:?}",
            dynamic_import_lines(src)
        );
    }

    /// T002 must honor the canonical `types`/`typings` field, not just the
    /// three legacy hardcoded layouts. All three shapes below were reported as
    /// untyped in the Vercel CLI corpus.
    #[test]
    fn package_types_field_is_honored() {
        let cases: &[(&str, &str)] = &[
            // date-fns
            (
                r#"{"name":"date-fns","types":"./typings.d.ts"}"#,
                "typings.d.ts",
            ),
            // tldts
            (
                r#"{"name":"tldts","types":"dist/types/index.d.ts"}"#,
                "dist/types/index.d.ts",
            ),
            // @inquirer/confirm
            (
                r#"{"name":"confirm","types":"./dist/cjs/types/index.d.ts"}"#,
                "dist/cjs/types/index.d.ts",
            ),
            // legacy `typings` spelling
            (
                r#"{"name":"old","typings":"lib/main.d.ts"}"#,
                "lib/main.d.ts",
            ),
            // extensionless form npm also allows
            (r#"{"name":"ext","types":"./index"}"#, "index.d.ts"),
        ];
        for (manifest, decl_rel) in cases {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            std::fs::write(root.join("package.json"), manifest).unwrap();
            let decl = root.join(decl_rel);
            std::fs::create_dir_all(decl.parent().unwrap()).unwrap();
            std::fs::write(&decl, "export {};\n").unwrap();
            assert!(
                package_declares_types(root),
                "manifest {manifest} with {decl_rel} must count as typed"
            );
        }
    }

    /// A `types` condition inside an `exports` map also counts.
    #[test]
    fn package_exports_types_condition_is_honored() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"m","exports":{".":{"import":{"types":"./d.ts","default":"./m.js"}}}}"#,
        )
        .unwrap();
        assert!(package_declares_types(root));
    }

    /// A genuinely untyped package must still be flagged.
    #[test]
    fn package_without_declarations_is_still_flagged() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"plain","main":"index.js"}"#,
        )
        .unwrap();
        std::fs::write(root.join("index.js"), "module.exports = {};\n").unwrap();
        assert!(
            !package_declares_types(root),
            "package ships no declarations; T002 must still fire"
        );
    }

    /// A `types` field pointing at a file that does not exist must not count.
    #[test]
    fn package_types_field_pointing_nowhere_is_not_honored() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"broken","types":"./missing.d.ts"}"#,
        )
        .unwrap();
        std::fs::write(root.join("index.js"), "module.exports = {};\n").unwrap();
        assert!(!package_declares_types(root));
    }

    /// `import.meta` and static ESM imports must never be mistaken for
    /// dynamic `import()` calls.
    #[test]
    fn dynamic_import_ignores_import_meta_and_static_imports() {
        let src = "import chalk from 'chalk';\n\
                   import { join } from 'node:path';\n\
                   const dir = import.meta.url;\n";
        assert!(
            dynamic_import_lines(src).is_empty(),
            "got: {:?}",
            dynamic_import_lines(src)
        );
    }

    /// #3744: `perry check` must not report a clean build for modern Node
    /// builtins that `perry compile` rejects. The builtin table feeds the
    /// U006 diagnostic only when a name is recognized as a Node builtin AND
    /// is not in the supported allowlist — so an unsupported builtin missing
    /// from `is_node_builtin` silently passes check.
    #[test]
    fn modern_unsupported_builtins_are_flagged() {
        // Compile-rejected modules: recognized as builtins, NOT supported, so
        // `check_node_builtin_imports` surfaces the diagnostic.
        for m in ["sea", "inspector", "node:inspector/promises"] {
            assert!(is_node_builtin(m), "{m} should be a recognized builtin");
            assert!(
                !is_supported_node_builtin(m),
                "{m} is not compile-supported and must be flagged by check"
            );
        }
    }

    /// Modern builtins that `perry compile` accepts must be in BOTH tables so
    /// check neither false-negatives (silently passes an unsupported import)
    /// nor false-positives (flags a working import).
    #[test]
    fn modern_supported_builtins_are_recognized_and_allowed() {
        for m in [
            "async_hooks",
            "diagnostics_channel",
            "http2",
            "node:sea",
            "sqlite",
            "test",
            "node:test/reporters",
            "trace_events",
            "wasi",
        ] {
            assert!(is_node_builtin(m), "{m} should be a recognized builtin");
            assert!(
                is_supported_node_builtin(m),
                "{m} compiles and must not be flagged by check"
            );
        }
    }

    /// Every compile-supported builtin must also be a recognized builtin —
    /// otherwise the allowlist arm is dead (the gate short-circuits on
    /// `is_node_builtin` first).
    #[test]
    fn supported_implies_recognized() {
        for base in [
            "crypto",
            "events",
            "http",
            "http2",
            "https",
            "net",
            "readline",
            "stream",
            "worker_threads",
            "zlib",
            "fs",
            "path",
            "os",
            "util",
            "process",
            "buffer",
            "console",
            "perf_hooks",
            "timers",
            "url",
            "querystring",
            "tls",
            "tty",
            "assert",
            "diagnostics_channel",
            "sqlite",
            "cluster",
            "child_process",
            "dgram",
            "dns",
            "domain",
            "repl",
            "punycode",
            "string_decoder",
            "sys",
            "v8",
            "vm",
            "constants",
            "module",
            "async_hooks",
            "test",
            "trace_events",
            "wasi",
        ] {
            assert!(
                is_node_builtin(base),
                "{base} is in the supported allowlist but not the builtin table"
            );
        }
    }
}
