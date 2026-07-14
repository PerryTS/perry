//! #495 — behavioral SBOM emitted at compile time.
//!
//! Walks the HIR for each source module and collects a per-module
//! manifest of stdlib symbols actually called. The manifest is the
//! foundation for the rest of the supply-chain hardening series:
//!
//! - `#501` consumes it to enforce host-controlled per-package
//!   capabilities (e.g. "this dep must not call `child_process.*`").
//! - `#496` (`--lockdown`) flags violations from the same data.
//! - Reviewers can diff a `package.json` change's effect on the
//!   binary's behavioral surface without re-running the build.
//!
//! Scope of this first cut (MVP): stdlib symbol calls only. Literal
//! hosts/URLs (#502) and native-library symbol references (FFI
//! registry) are tracked separately and will graft onto the same
//! manifest in follow-up PRs. The JSON shape is versioned so future
//! additions don't break consumers.

use crate::ir::{Expr, Module, Stmt};
use crate::walker::walk_expr_children;
use std::collections::BTreeMap;

/// Per-module audit record. Keys are sorted (BTreeMap) so the
/// serialized JSON is byte-deterministic across builds — critical
/// for the `perry audit --diff` workflow.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ModuleAudit {
    /// Source path the module was lowered from. Absolute / canonical
    /// when known; matches `LoweringContext::source_file_path`.
    pub source: String,
    /// npm package name when `source` resolves through
    /// `node_modules/<pkg>/...`. `None` for host source.
    pub package: Option<String>,
    /// stdlib namespace → sorted unique method names called by this
    /// module. Method names match the `NativeMethodCall::method`
    /// field — i.e. the symbol as it appears in user source after
    /// alias resolution.
    pub stdlib: BTreeMap<String, Vec<String>>,
}

/// Top-level audit manifest. Version is bumped if the JSON shape
/// changes incompatibly (`stdlib` is open for extension within v1).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct AuditManifest {
    pub version: u32,
    pub modules: Vec<ModuleAudit>,
}

impl AuditManifest {
    pub fn new() -> Self {
        Self {
            version: 1,
            modules: Vec::new(),
        }
    }
}

/// Walk a single HIR `Module` and return its `ModuleAudit`. The walk
/// visits `init` (top-level statements), every function body, and
/// every method on every class. `NativeMethodCall::module` /
/// `::method` pairs are folded into the `stdlib` map.
pub fn audit_module(hir_module: &Module, source: &str) -> ModuleAudit {
    let mut record = ModuleAudit {
        source: source.to_string(),
        package: package_name_for_source_path(source).map(|s| s.to_string()),
        stdlib: BTreeMap::new(),
    };

    // The HIR may carry a `Stmt::Expr(expr)` shape where the expr
    // itself contains nested calls; the walker recurses through
    // all Expr children, so we only need to visit each top-level
    // Expr once.
    for stmt in &hir_module.init {
        visit_stmt(stmt, &mut record);
    }
    for func in &hir_module.functions {
        for stmt in &func.body {
            visit_stmt(stmt, &mut record);
        }
    }
    for class in &hir_module.classes {
        for method in &class.methods {
            for stmt in &method.body {
                visit_stmt(stmt, &mut record);
            }
        }
    }

    // Deduplicate + sort within each namespace bucket so the
    // serialized JSON is stable across builds.
    for methods in record.stdlib.values_mut() {
        methods.sort();
        methods.dedup();
    }

    record
}

fn visit_stmt(stmt: &Stmt, out: &mut ModuleAudit) {
    match stmt {
        Stmt::Expr(e) => visit_expr(e, out),
        Stmt::Let { init, .. } => {
            if let Some(v) = init {
                visit_expr(v, out);
            }
        }
        Stmt::Return(Some(e)) => visit_expr(e, out),
        Stmt::Return(None) | Stmt::Break | Stmt::Continue => {}
        Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
        Stmt::Labeled { body, .. } => visit_stmt(body, out),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            visit_expr(condition, out);
            for s in then_branch {
                visit_stmt(s, out);
            }
            if let Some(else_b) = else_branch {
                for s in else_b {
                    visit_stmt(s, out);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            visit_expr(condition, out);
            for s in body {
                visit_stmt(s, out);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init) = init {
                visit_stmt(init, out);
            }
            if let Some(c) = condition {
                visit_expr(c, out);
            }
            if let Some(u) = update {
                visit_expr(u, out);
            }
            for s in body {
                visit_stmt(s, out);
            }
        }
        Stmt::Throw(e) => visit_expr(e, out),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                visit_stmt(s, out);
            }
            if let Some(catch_clause) = catch {
                for s in &catch_clause.body {
                    visit_stmt(s, out);
                }
            }
            if let Some(finally_b) = finally {
                for s in finally_b {
                    visit_stmt(s, out);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            visit_expr(discriminant, out);
            for case in cases {
                if let Some(test) = &case.test {
                    visit_expr(test, out);
                }
                for s in &case.body {
                    visit_stmt(s, out);
                }
            }
        }
        // PreallocateBoxes carries only LocalIds, no Expr / Stmt children.
        Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => {}
    }
}

fn visit_expr(expr: &Expr, out: &mut ModuleAudit) {
    // General-shape native calls (`mysql2.createConnection`,
    // `child_process.execSync`, `crypto.randomUUID`, …) carry the
    // namespace and method by name on the variant.
    if let Expr::NativeMethodCall { module, method, .. } = expr {
        record_call(out, module, method);
    }
    // The HIR also has dedicated variants for hot stdlib symbols
    // (folded at lowering time for codegen specialization). The
    // audit needs to see those too — otherwise a host that only
    // calls `fs.readFileSync` would appear to make zero stdlib
    // calls, defeating the SBOM. Mapping is mechanical:
    // `Expr::Fs<Method>` → ("fs", "<method>"). Keep this exhaustive
    // for the namespaces that matter to supply-chain review (`fs`,
    // `path`, `process`); extend opportunistically for others.
    if let Some((module, method)) = specialized_stdlib_call(expr) {
        record_call(out, module, method);
    }
    walk_expr_children(expr, &mut |child| visit_expr(child, out));
}

/// Map specialized `Expr::Fs*` / `Expr::Path*` / `Expr::Process*` /
/// `Expr::Tty*` variants to the `(namespace, method)` pair that the
/// equivalent un-specialized call would have produced. Returning
/// `None` is the catch-all — the walker still descends into children
/// for those variants so nested calls aren't missed.
fn specialized_stdlib_call(expr: &Expr) -> Option<(&'static str, &'static str)> {
    Some(match expr {
        // fs — paths involving the filesystem are the highest-signal
        // capability check for supply-chain review.
        Expr::FsReadFileSync(_) => ("fs", "readFileSync"),
        Expr::FsWriteFileSync(_, _) => ("fs", "writeFileSync"),
        Expr::FsExistsSync(_) => ("fs", "existsSync"),
        Expr::FsMkdirSync(_) => ("fs", "mkdirSync"),
        Expr::FsUnlinkSync(_) => ("fs", "unlinkSync"),
        Expr::FsAppendFileSync(_, _) => ("fs", "appendFileSync"),
        Expr::FsReadFileBinary(_) => ("fs", "readFile"),
        Expr::FsRmRecursive(_) => ("fs", "rm"),
        // path — pure-string transforms, lower security-signal but
        // included so `perry audit --sbom` shows the full surface.
        Expr::PathJoin(_, _) | Expr::PathResolveJoin(_, _) | Expr::PathWin32Join(_, _) => {
            ("path", "join")
        }
        Expr::PathDirname(_) => ("path", "dirname"),
        Expr::PathBasename(_) | Expr::PathBasenameExt(_, _) => ("path", "basename"),
        Expr::PathExtname(_) => ("path", "extname"),
        Expr::PathResolve(_) => ("path", "resolve"),
        Expr::PathIsAbsolute(_) => ("path", "isAbsolute"),
        Expr::PathRelative(_, _) => ("path", "relative"),
        Expr::PathNormalize(_) => ("path", "normalize"),
        Expr::PathParse(_) => ("path", "parse"),
        Expr::PathFormat(_) => ("path", "format"),
        Expr::PathSep => ("path", "sep"),
        Expr::PathDelimiter => ("path", "delimiter"),
        Expr::PathToNamespacedPath(_) => ("path", "toNamespacedPath"),
        Expr::PathMatchesGlob(_, _) => ("path", "matchesGlob"),
        Expr::PathWin32 { method, .. } => match method {
            crate::ir::PathWin32Method::Dirname => ("path", "win32.dirname"),
            crate::ir::PathWin32Method::Basename | crate::ir::PathWin32Method::BasenameExt => {
                ("path", "win32.basename")
            }
            crate::ir::PathWin32Method::Extname => ("path", "win32.extname"),
            crate::ir::PathWin32Method::IsAbsolute => ("path", "win32.isAbsolute"),
            crate::ir::PathWin32Method::Normalize => ("path", "win32.normalize"),
            crate::ir::PathWin32Method::Parse => ("path", "win32.parse"),
            crate::ir::PathWin32Method::Format => ("path", "win32.format"),
            crate::ir::PathWin32Method::Relative => ("path", "win32.relative"),
            crate::ir::PathWin32Method::Resolve | crate::ir::PathWin32Method::ResolveJoin => {
                ("path", "win32.resolve")
            }
            crate::ir::PathWin32Method::ToNamespacedPath => ("path", "win32.toNamespacedPath"),
            crate::ir::PathWin32Method::MatchesGlob => ("path", "win32.matchesGlob"),
        },
        // process — `process.env` etc. are accessed via dedicated
        // HIR variants. The SBOM should reflect that the binary
        // touches them.
        Expr::ProcessEnv => ("process", "env"),
        Expr::ProcessUptime => ("process", "uptime"),
        Expr::ProcessCwd => ("process", "cwd"),
        Expr::ProcessArgv => ("process", "argv"),
        Expr::ProcessAbort => ("process", "abort"),
        Expr::ProcessUmask(_) => ("process", "umask"),
        Expr::ProcessThreadCpuUsage(_) => ("process", "threadCpuUsage"),
        Expr::ProcessAvailableMemory => ("process", "availableMemory"),
        Expr::ProcessConstrainedMemory => ("process", "constrainedMemory"),
        Expr::ProcessPosixCredential(crate::ir::PosixCredentialKind::Uid) => ("process", "getuid"),
        Expr::ProcessPosixCredential(crate::ir::PosixCredentialKind::Euid) => {
            ("process", "geteuid")
        }
        Expr::ProcessPosixCredential(crate::ir::PosixCredentialKind::Gid) => ("process", "getgid"),
        Expr::ProcessPosixCredential(crate::ir::PosixCredentialKind::Egid) => {
            ("process", "getegid")
        }
        Expr::ProcessEmitWarning(_) => ("process", "emitWarning"),
        Expr::ProcessCpuUsage(_) => ("process", "cpuUsage"),
        Expr::ProcessResourceUsage => ("process", "resourceUsage"),
        Expr::ProcessActiveResourcesInfo => ("process", "getActiveResourcesInfo"),
        Expr::ProcessHrtime(_) => ("process", "hrtime"),
        Expr::ProcessTitle => ("process", "title"),
        Expr::ProcessSetTitle(_) => ("process", "title"),
        Expr::ProcessStdinIsTTY => ("process", "stdin.isTTY"),
        Expr::ProcessStdoutIsTTY => ("process", "stdout.isTTY"),
        Expr::ProcessStderrIsTTY => ("process", "stderr.isTTY"),
        Expr::ProcessStdoutColumns => ("process", "stdout.columns"),
        Expr::ProcessStdoutRows => ("process", "stdout.rows"),
        // tty — TTY tests for terminal detection.
        Expr::TtyIsAtty(_) => ("tty", "isatty"),
        // url — file-URL conversion.
        Expr::FileURLToPath(_) => ("url", "fileURLToPath"),
        _ => return None,
    })
}

fn record_call(out: &mut ModuleAudit, module: &str, method: &str) {
    out.stdlib
        .entry(module.to_string())
        .or_default()
        .push(method.to_string());
}

/// Extract the owning npm package name from a source-file path by
/// locating the rightmost `node_modules/` segment. Scope-aware.
/// Mirrors the logic shared with the supply-chain gates — duplicated
/// here so this module doesn't pull in a perry-driver dep.
fn package_name_for_source_path(source_path: &str) -> Option<&str> {
    let idx = source_path.rfind("node_modules/")?;
    let after = &source_path[idx + "node_modules/".len()..];
    if let Some(stripped) = after.strip_prefix('@') {
        let mut parts = stripped.splitn(3, '/');
        let scope = parts.next().unwrap_or("");
        let pkg = parts.next().unwrap_or("");
        if scope.is_empty() || pkg.is_empty() {
            return None;
        }
        let end = idx + "node_modules/".len() + 1 + scope.len() + 1 + pkg.len();
        Some(&source_path[idx + "node_modules/".len()..end])
    } else {
        let pkg = after.split('/').next()?;
        if pkg.is_empty() {
            None
        } else {
            Some(pkg)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Expr, Module, Stmt};

    fn empty_module(name: &str) -> Module {
        Module::new(name)
    }

    fn native_call(module: &str, method: &str) -> Expr {
        Expr::NativeMethodCall {
            module: module.to_string(),
            class_name: None,
            object: None,
            method: method.to_string(),
            args: vec![],
        }
    }

    #[test]
    fn empty_module_has_no_records() {
        let m = empty_module("test");
        let rec = audit_module(&m, "/repo/src/test.ts");
        assert!(rec.stdlib.is_empty());
        assert_eq!(rec.source, "/repo/src/test.ts");
        assert!(rec.package.is_none());
    }

    #[test]
    fn top_level_native_call_recorded() {
        let mut m = empty_module("test");
        m.init.push(Stmt::Expr(native_call("fs", "readFileSync")));
        let rec = audit_module(&m, "/repo/src/test.ts");
        assert_eq!(rec.stdlib.get("fs"), Some(&vec!["readFileSync".into()]));
    }

    #[test]
    fn duplicate_calls_dedupe() {
        let mut m = empty_module("test");
        m.init.push(Stmt::Expr(native_call("fs", "readFileSync")));
        m.init.push(Stmt::Expr(native_call("fs", "readFileSync")));
        m.init.push(Stmt::Expr(native_call("fs", "writeFileSync")));
        let rec = audit_module(&m, "/repo/src/test.ts");
        // Sorted + deduped: ["readFileSync", "writeFileSync"].
        assert_eq!(
            rec.stdlib.get("fs"),
            Some(&vec!["readFileSync".into(), "writeFileSync".into()])
        );
    }

    #[test]
    fn package_name_extracted_from_node_modules_path() {
        let m = empty_module("test");
        let rec = audit_module(&m, "/repo/node_modules/lodash/lib/x.ts");
        assert_eq!(rec.package.as_deref(), Some("lodash"));
    }

    #[test]
    fn scoped_package_name_extracted() {
        let m = empty_module("test");
        let rec = audit_module(&m, "/repo/node_modules/@scope/pkg/src/x.ts");
        assert_eq!(rec.package.as_deref(), Some("@scope/pkg"));
    }

    #[test]
    fn nested_node_modules_returns_innermost() {
        let m = empty_module("test");
        let rec = audit_module(&m, "/repo/node_modules/outer/node_modules/inner/lib/x.ts");
        assert_eq!(rec.package.as_deref(), Some("inner"));
    }

    #[test]
    fn user_source_has_no_package() {
        let m = empty_module("test");
        let rec = audit_module(&m, "/repo/src/main.ts");
        assert!(rec.package.is_none());
    }

    #[test]
    fn nested_call_recorded() {
        // The walker recurses through Expr children — a NativeMethodCall
        // buried under e.g. a Stmt::If condition still surfaces.
        let mut m = empty_module("test");
        m.init.push(Stmt::If {
            condition: native_call("process", "uptime"),
            then_branch: vec![Stmt::Expr(native_call("fs", "readFileSync"))],
            else_branch: None,
        });
        let rec = audit_module(&m, "/repo/src/test.ts");
        assert_eq!(rec.stdlib.get("process"), Some(&vec!["uptime".into()]));
        assert_eq!(rec.stdlib.get("fs"), Some(&vec!["readFileSync".into()]));
    }

    #[test]
    fn serializes_to_stable_json() {
        let mut m = empty_module("test");
        m.init.push(Stmt::Expr(native_call("fs", "writeFileSync")));
        m.init.push(Stmt::Expr(native_call("fs", "readFileSync")));
        let rec = audit_module(&m, "/repo/src/test.ts");
        let manifest = AuditManifest {
            version: 1,
            modules: vec![rec],
        };
        let json = serde_json::to_string(&manifest).unwrap();
        // BTreeMap + sort_unstable on the method vec means the
        // output ordering is independent of insertion order.
        assert!(
            json.contains("\"fs\":[\"readFileSync\",\"writeFileSync\"]"),
            "unexpected: {json}"
        );
    }
}
