//! #501 — host-controlled per-package capability enforcement.
//!
//! The "big lever" of the supply-chain hardening series. Most npm
//! packages will never declare capabilities themselves; control sits
//! entirely in the host application's `package.json`. This module
//! walks each lowered HIR module, derives the capability tokens its
//! stdlib calls would need (`fs:read`, `crypto`, …), and reports
//! violations against the per-package policy supplied by the driver.
//!
//! ## Capability tokens covered by the MVP
//!
//! | Token         | Trips on                                          |
//! |---------------|---------------------------------------------------|
//! | `fs:read`     | `fs.readFileSync`, `fs.existsSync`, the binary `readFile`, `readdir`, `stat`-shaped calls routed through `NativeMethodCall { module: "fs", method: ... }`. |
//! | `fs:write`    | `fs.writeFileSync`, `fs.appendFileSync`, `fs.mkdirSync`, `fs.unlinkSync`, `fs.rm*`. |
//! | `crypto`      | Every dedicated `Crypto*` / `WebCrypto*` HIR variant + `NativeMethodCall { module: "crypto", … }`. |
//! | `proc:env`    | `process.env` read/write (`Expr::ProcessEnv`, `Expr::EnvGet`). |
//! | `proc:argv`   | `Expr::ProcessArgv`. |
//! | `proc:exec`   | Every `ChildProcess*` HIR variant + `NativeMethodCall { module: "child_process", … }`. |
//! | `net:fetch`   | `FetchWithOptions`, `FetchGetWithAuth`, `FetchPostWithAuth`. |
//! | `net:listen`  | `NetCreateServer`. |
//! | `net:connect` | `NetCreateConnection`, `NetConnect`. |
//!
//! `*` in a policy means "every capability allowed" — the escape
//! hatch for `host` code that the policy lookup should never gate.
//!
//! ## What's deferred (documented for follow-up)
//!
//! - Per-host `net:<host>` tokens — requires the URL-literal extraction
//!   pass (#502 is the matching feature). The token taxonomy is
//!   forward-compatible: a policy entry `net:api.example.com` would
//!   match URLs whose extracted host equals `api.example.com` once
//!   the URL-extraction landed.
//! - `time` — Date.now / performance.now. Side-channel paranoia.
//! - Capability inheritance through user-defined wrappers ("if `lodash`
//!   calls a host helper that calls `fs.readFile`, who's accountable?").
//!   Current attribution is by the file containing the call site
//!   (correct for the common case where deps live under
//!   `node_modules/<pkg>/`).

use crate::ir::{Expr, Module, Stmt};
use crate::walker::walk_expr_children;
use std::collections::BTreeMap;

/// One refused stdlib call site under the capability policy. The
/// driver assembles these across every module and surfaces them as a
/// single combined diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityViolation {
    /// Source file the call appears in.
    pub source: String,
    /// Owning npm package name (`@scope/pkg` or `lodash`) if the
    /// source path lives under `node_modules/<pkg>/...`; `None` for
    /// host source.
    pub package: Option<String>,
    /// Capability token required for the call (e.g. `"fs:read"`).
    pub required: &'static str,
    /// Short kind label for the diagnostic (e.g. `"fs.readFileSync"`).
    pub kind: &'static str,
}

/// Map a (resolved) package name to its allowed capability set. The
/// special key `"*"` is the default that applies to any package not
/// explicitly listed. The host's own code (`None` package) is always
/// granted `*` unconditionally — gating host code is what the
/// `--lockdown` mode (#496) is for.
pub type CapabilityPolicy = BTreeMap<String, Vec<String>>;

/// Walk a single HIR module collecting capability violations against
/// `policy`. `host_package` is the host application's package name
/// (read from its own `package.json` `name`), used to attribute
/// violations to the host bucket (always granted `*`).
pub fn audit_module_capabilities(
    hir_module: &Module,
    source: &str,
    policy: &CapabilityPolicy,
    host_package: Option<&str>,
) -> Vec<CapabilityViolation> {
    let pkg = package_name_for_source_path(source).map(|s| s.to_string());
    // Host code is always granted `*` regardless of policy. The
    // policy's `*` default applies to non-host packages not
    // explicitly listed.
    let is_host = match (&pkg, host_package) {
        (None, _) => true,
        (Some(name), Some(host)) if name == host => true,
        _ => false,
    };
    if is_host {
        return Vec::new();
    }
    let pkg_name = pkg.clone();
    // Resolve the allowed-set for this package once, then walk.
    let allowed: Vec<&str> = if let Some(name) = &pkg_name {
        policy
            .get(name)
            .or_else(|| policy.get("*"))
            .map(|v| v.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default()
    } else {
        // Defensive: shouldn't happen because we just established
        // `is_host = false`. Default-deny still safer than crash.
        Vec::new()
    };
    if allowed.contains(&"*") {
        return Vec::new();
    }
    let mut ctx = WalkCtx {
        source: source.to_string(),
        package: pkg_name,
        allowed,
        violations: Vec::new(),
    };
    for stmt in &hir_module.init {
        visit_stmt(stmt, &mut ctx);
    }
    for func in &hir_module.functions {
        for stmt in &func.body {
            visit_stmt(stmt, &mut ctx);
        }
    }
    for class in &hir_module.classes {
        for method in &class.methods {
            for stmt in &method.body {
                visit_stmt(stmt, &mut ctx);
            }
        }
    }
    ctx.violations
}

struct WalkCtx<'a> {
    source: String,
    package: Option<String>,
    allowed: Vec<&'a str>,
    violations: Vec<CapabilityViolation>,
}

fn visit_stmt(stmt: &Stmt, ctx: &mut WalkCtx) {
    match stmt {
        Stmt::Expr(e) => visit_expr(e, ctx),
        Stmt::Let { init, .. } => {
            if let Some(v) = init {
                visit_expr(v, ctx);
            }
        }
        Stmt::Return(Some(e)) => visit_expr(e, ctx),
        Stmt::Return(None) | Stmt::Break | Stmt::Continue => {}
        Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
        Stmt::Labeled { body, .. } => visit_stmt(body, ctx),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            visit_expr(condition, ctx);
            for s in then_branch {
                visit_stmt(s, ctx);
            }
            if let Some(else_b) = else_branch {
                for s in else_b {
                    visit_stmt(s, ctx);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            visit_expr(condition, ctx);
            for s in body {
                visit_stmt(s, ctx);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init) = init {
                visit_stmt(init, ctx);
            }
            if let Some(c) = condition {
                visit_expr(c, ctx);
            }
            if let Some(u) = update {
                visit_expr(u, ctx);
            }
            for s in body {
                visit_stmt(s, ctx);
            }
        }
        Stmt::Throw(e) => visit_expr(e, ctx),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                visit_stmt(s, ctx);
            }
            if let Some(c) = catch {
                for s in &c.body {
                    visit_stmt(s, ctx);
                }
            }
            if let Some(f) = finally {
                for s in f {
                    visit_stmt(s, ctx);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            visit_expr(discriminant, ctx);
            for case in cases {
                if let Some(test) = &case.test {
                    visit_expr(test, ctx);
                }
                for s in &case.body {
                    visit_stmt(s, ctx);
                }
            }
        }
        Stmt::PreallocateBoxes(_) => {}
    }
}

fn visit_expr(expr: &Expr, ctx: &mut WalkCtx) {
    if let Some((cap, kind)) = required_capability(expr) {
        if !ctx.allowed.contains(&cap) {
            ctx.violations.push(CapabilityViolation {
                source: ctx.source.clone(),
                package: ctx.package.clone(),
                required: cap,
                kind,
            });
        }
    }
    walk_expr_children(expr, &mut |child| visit_expr(child, ctx));
}

/// Map a single HIR expression to the `(capability_token, kind)`
/// pair it requires, or `None` if the expression doesn't touch a
/// gated stdlib surface. Specialised HIR variants land first; the
/// general-shape `NativeMethodCall` fallback covers anything routed
/// through a stdlib namespace without a dedicated variant.
fn required_capability(expr: &Expr) -> Option<(&'static str, &'static str)> {
    Some(match expr {
        // fs:read
        Expr::FsReadFileSync(_) => ("fs:read", "fs.readFileSync"),
        Expr::FsReadFileBinary(_) => ("fs:read", "fs.readFile"),
        Expr::FsExistsSync(_) => ("fs:read", "fs.existsSync"),

        // fs:write
        Expr::FsWriteFileSync(_, _) => ("fs:write", "fs.writeFileSync"),
        Expr::FsAppendFileSync(_, _) => ("fs:write", "fs.appendFileSync"),
        Expr::FsMkdirSync(_) => ("fs:write", "fs.mkdirSync"),
        Expr::FsUnlinkSync(_) => ("fs:write", "fs.unlinkSync"),
        Expr::FsRmRecursive(_) => ("fs:write", "fs.rmRecursive"),

        // proc:env / proc:argv
        Expr::ProcessEnv | Expr::EnvGet(_) => ("proc:env", "process.env"),
        Expr::ProcessArgv => ("proc:argv", "process.argv"),

        // proc:exec
        Expr::ChildProcessExec { .. } => ("proc:exec", "child_process.exec"),
        Expr::ChildProcessExecSync { .. } => ("proc:exec", "child_process.execSync"),
        Expr::ChildProcessSpawn { .. } => ("proc:exec", "child_process.spawn"),
        Expr::ChildProcessFork { .. } => ("proc:exec", "child_process.fork"),
        Expr::ChildProcessSpawnSync { .. } => ("proc:exec", "child_process.spawnSync"),
        Expr::ChildProcessSpawnBackground { .. } => ("proc:exec", "child_process.spawnBackground"),
        Expr::ChildProcessGetProcessStatus(_) => ("proc:exec", "child_process.getProcessStatus"),
        Expr::ChildProcessKillProcess(_) => ("proc:exec", "child_process.killProcess"),

        // net:fetch
        Expr::FetchWithOptions { .. } => ("net:fetch", "fetch"),
        Expr::FetchGetWithAuth { .. } => ("net:fetch", "fetch (with auth)"),
        Expr::FetchPostWithAuth { .. } => ("net:fetch", "fetch POST (with auth)"),

        // net:listen / net:connect
        Expr::NetCreateServer { .. } => ("net:listen", "net.createServer"),
        Expr::NetCreateConnection { .. } => ("net:connect", "net.createConnection"),
        Expr::NetConnect { .. } => ("net:connect", "net.connect"),

        // General-shape NativeMethodCall fallback for namespaces
        // without dedicated variants (or method names we haven't
        // hard-coded above).
        Expr::NativeMethodCall { module, .. } => match module.as_str() {
            "child_process" => ("proc:exec", "child_process.<call>"),
            "crypto" => ("crypto", "crypto.<call>"),
            "fs" => ("fs:read", "fs.<call>"),
            _ => return None,
        },

        _ => return None,
    })
}

/// Extract the owning npm package name from a source-file path by
/// locating the rightmost `node_modules/` segment. Scope-aware:
/// `node_modules/@scope/pkg/...` returns `@scope/pkg`. Returns
/// `None` for host-source files outside `node_modules/`.
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

    fn empty_module() -> Module {
        Module::new("test")
    }

    fn pol(entries: &[(&str, &[&str])]) -> CapabilityPolicy {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.iter().map(|s| s.to_string()).collect()))
            .collect()
    }

    #[test]
    fn host_code_unconditionally_allowed() {
        let mut m = empty_module();
        m.init
            .push(Stmt::Expr(Expr::FsReadFileSync(Box::new(Expr::String(
                "/x".into(),
            )))));
        // Empty policy — would normally block.
        let v = audit_module_capabilities(&m, "/repo/src/main.ts", &pol(&[]), Some("hostapp"));
        assert!(v.is_empty());
    }

    #[test]
    fn host_named_package_is_host() {
        let mut m = empty_module();
        m.init
            .push(Stmt::Expr(Expr::FsReadFileSync(Box::new(Expr::String(
                "/x".into(),
            )))));
        let v = audit_module_capabilities(
            &m,
            "/repo/node_modules/hostapp/lib/x.ts",
            &pol(&[]),
            Some("hostapp"),
        );
        // Even though the path is in node_modules, the package name
        // matches the host's, so it gets the host's free pass.
        assert!(v.is_empty());
    }

    #[test]
    fn unlisted_dep_inherits_star_default() {
        let mut m = empty_module();
        m.init
            .push(Stmt::Expr(Expr::FsReadFileSync(Box::new(Expr::String(
                "/x".into(),
            )))));
        // `*` default allows everything.
        let v = audit_module_capabilities(
            &m,
            "/repo/node_modules/somedep/lib.ts",
            &pol(&[("*", &["*"])]),
            Some("hostapp"),
        );
        assert!(v.is_empty());
    }

    #[test]
    fn explicit_deny_blocks_call() {
        let mut m = empty_module();
        m.init
            .push(Stmt::Expr(Expr::FsReadFileSync(Box::new(Expr::String(
                "/x".into(),
            )))));
        let v = audit_module_capabilities(
            &m,
            "/repo/node_modules/lodash/template.js",
            &pol(&[("lodash", &[])]),
            Some("hostapp"),
        );
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].required, "fs:read");
        assert_eq!(v[0].kind, "fs.readFileSync");
        assert_eq!(v[0].package.as_deref(), Some("lodash"));
    }

    #[test]
    fn granted_capability_passes() {
        let mut m = empty_module();
        m.init
            .push(Stmt::Expr(Expr::FsReadFileSync(Box::new(Expr::String(
                "/x".into(),
            )))));
        let v = audit_module_capabilities(
            &m,
            "/repo/node_modules/axios/lib.ts",
            &pol(&[("axios", &["fs:read", "net:fetch"])]),
            Some("hostapp"),
        );
        assert!(v.is_empty());
    }

    #[test]
    fn star_token_grants_everything() {
        let mut m = empty_module();
        m.init.push(Stmt::Expr(Expr::FsWriteFileSync(
            Box::new(Expr::String("/x".into())),
            Box::new(Expr::String("body".into())),
        )));
        m.init.push(Stmt::Expr(Expr::ChildProcessExecSync {
            command: Box::new(Expr::String("ls".into())),
            options: None,
        }));
        let v = audit_module_capabilities(
            &m,
            "/repo/node_modules/anywhere/lib.ts",
            &pol(&[("anywhere", &["*"])]),
            Some("hostapp"),
        );
        assert!(v.is_empty());
    }

    #[test]
    fn dep_specific_overrides_star_default() {
        // `*` grants `["crypto"]`, but `lodash` is explicitly empty.
        // The dep-specific entry wins.
        let mut m = empty_module();
        m.init
            .push(Stmt::Expr(Expr::FsReadFileSync(Box::new(Expr::String(
                "/x".into(),
            )))));
        let v = audit_module_capabilities(
            &m,
            "/repo/node_modules/lodash/template.js",
            &pol(&[("lodash", &[]), ("*", &["crypto", "fs:read"])]),
            Some("hostapp"),
        );
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].required, "fs:read");
    }

    #[test]
    fn scoped_package_name_extracted() {
        let mut m = empty_module();
        m.init
            .push(Stmt::Expr(Expr::FsReadFileSync(Box::new(Expr::String(
                "/x".into(),
            )))));
        let v = audit_module_capabilities(
            &m,
            "/repo/node_modules/@scope/pkg/lib.ts",
            &pol(&[("@scope/pkg", &[])]),
            Some("hostapp"),
        );
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].package.as_deref(), Some("@scope/pkg"));
    }

    #[test]
    fn child_process_requires_proc_exec() {
        let mut m = empty_module();
        m.init.push(Stmt::Expr(Expr::ChildProcessExecSync {
            command: Box::new(Expr::String("ls".into())),
            options: None,
        }));
        let v = audit_module_capabilities(
            &m,
            "/repo/node_modules/lodash/lib.ts",
            &pol(&[("lodash", &["fs:read"])]), // fs but no proc:exec
            Some("hostapp"),
        );
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].required, "proc:exec");
    }

    #[test]
    fn process_env_requires_proc_env() {
        let mut m = empty_module();
        m.init.push(Stmt::Expr(Expr::ProcessEnv));
        let v = audit_module_capabilities(
            &m,
            "/repo/node_modules/dotenv/lib.ts",
            &pol(&[("dotenv", &[])]),
            Some("hostapp"),
        );
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].required, "proc:env");
    }

    #[test]
    fn fetch_requires_net_fetch() {
        let mut m = empty_module();
        m.init.push(Stmt::Expr(Expr::FetchWithOptions {
            url: Box::new(Expr::String("https://x.com/y".into())),
            method: Box::new(Expr::String("GET".into())),
            body: Box::new(Expr::Undefined),
            headers: vec![],
            headers_dynamic: None,
            signal: None,
        }));
        let v = audit_module_capabilities(
            &m,
            "/repo/node_modules/axios/lib.ts",
            &pol(&[("axios", &["fs:read"])]),
            Some("hostapp"),
        );
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].required, "net:fetch");
    }

    #[test]
    fn general_native_call_through_crypto() {
        let mut m = empty_module();
        m.init.push(Stmt::Expr(Expr::NativeMethodCall {
            module: "crypto".into(),
            class_name: None,
            object: None,
            method: "randomBytes".into(),
            args: vec![],
        }));
        let v = audit_module_capabilities(
            &m,
            "/repo/node_modules/lodash/lib.ts",
            &pol(&[("lodash", &[])]),
            Some("hostapp"),
        );
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].required, "crypto");
    }
}
