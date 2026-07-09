//! #502 — compile-time URL/host egress allowlist.
//!
//! Walks the HIR for every egress call site
//! (`fetch(url)` / `net.connect(host, port)` / `net.createConnection`),
//! cross-references the literal URL/host against the host's
//! `perry.allowedHosts` allowlist, and reports refusals via the
//! returned `EgressViolation` records. The driver consumes those and
//! aborts the build with a single diagnostic that names every
//! offending site at once — better UX than failing on the first one
//! and asking the user to re-run.
//!
//! The pass is opt-in: an empty allowlist means "feature disabled,
//! anything goes" rather than "default-deny". Default-deny would
//! break every existing build that calls `fetch(...)` without
//! migration; the issue's spirit is "host that *wants* the static
//! egress guarantee opts in by setting `allowedHosts`". Once set,
//! the gate is strict.
//!
//! ## Pattern syntax
//!
//! Each entry in `allowedHosts` is matched in order:
//!
//! - **Exact host**: `"api.example.com"` matches that hostname
//!   on any scheme/port/path.
//! - **Subdomain wildcard**: `"*.cdn.example.com"` matches every
//!   direct or transitive subdomain of `cdn.example.com`. The
//!   bare suffix itself does NOT match — `*.foo.com` does not
//!   match `foo.com`.
//! - **URL prefix**: `"https://api.acme.com/v1/*"` matches any
//!   URL beginning with the literal prefix. Useful for restricting
//!   which paths a dep can reach on a host you generally allow.
//! - **Universal**: `"*"` matches everything (escape hatch for
//!   incremental migration).
//!
//! ## What gets recorded as a violation
//!
//! - **Literal URL not matching any pattern** → violation.
//! - **Non-literal URL/host** (variable, expression, template-with-vars)
//!   → violation unless `allowDynamicHosts: true` is set in the
//!   host `package.json`. The static guarantee — "grep-ing the
//!   binary's egress is reliable" — depends on rejecting
//!   non-literal hosts by default.
//!
//! ## What's NOT covered yet
//!
//! - `http.get(url)` / `https.request(...)` / `WebSocket(url)`
//!   — these lower through the general-shape `NativeMethodCall`
//!   variant which makes URL-arg extraction harder. The MVP covers
//!   the highest-volume egress shape (`fetch` + `net.connect`) and
//!   leaves the rest as a follow-up under the same shape.

use crate::ir::{Expr, Module, Stmt};
use crate::walker::walk_expr_children;

/// One refused egress call site. The driver collects these from the
/// walker and emits a single diagnostic that lists every site at
/// once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressViolation {
    /// Source file the call appears in. Matches the module path
    /// passed to `audit_module_egress`.
    pub source: String,
    /// The lowered call-site shape — `"fetch"`, `"net.connect"`,
    /// `"net.createConnection"`, etc. Used in the diagnostic so
    /// reviewers know which entrypoint is at fault.
    pub kind: &'static str,
    /// The raw URL or host string when the argument was a string
    /// literal; `None` when the argument was non-literal (variable,
    /// expression, template with substitutions).
    pub literal: Option<String>,
    /// Why this site is refused.
    pub reason: EgressRefusalReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EgressRefusalReason {
    /// Literal URL/host did not match any pattern in the allowlist.
    LiteralNotAllowed,
    /// URL/host was not a string literal and `allowDynamicHosts`
    /// was false.
    NonLiteralAndDynamicForbidden,
}

/// Walk a single HIR module, collecting egress violations against
/// the host's allowlist. Pure analyser — returns the violations and
/// lets the caller decide how to surface them.
///
/// `allowed_hosts.is_empty()` short-circuits: the entire pass is
/// disabled until the host opts in (see module docs).
pub fn audit_module_egress(
    hir_module: &Module,
    source: &str,
    allowed_hosts: &[String],
    allow_dynamic_hosts: bool,
) -> Vec<EgressViolation> {
    if allowed_hosts.is_empty() {
        return Vec::new();
    }
    let mut ctx = WalkCtx {
        source: source.to_string(),
        allowed_hosts,
        allow_dynamic_hosts,
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
    allowed_hosts: &'a [String],
    allow_dynamic_hosts: bool,
    violations: Vec<EgressViolation>,
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
            if let Some(catch_clause) = catch {
                for s in &catch_clause.body {
                    visit_stmt(s, ctx);
                }
            }
            if let Some(finally_b) = finally {
                for s in finally_b {
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
        Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => {}
    }
}

fn visit_expr(expr: &Expr, ctx: &mut WalkCtx) {
    match expr {
        Expr::FetchWithOptions { url, .. } => check_url(ctx, "fetch", url),
        Expr::FetchGetWithAuth { url, .. } => check_url(ctx, "fetch (with auth)", url),
        Expr::FetchPostWithAuth { url, .. } => check_url(ctx, "fetch POST (with auth)", url),
        Expr::NetCreateConnection { host: Some(h), .. } => {
            check_host(ctx, "net.createConnection", h)
        }
        Expr::NetConnect { host: Some(h), .. } => check_host(ctx, "net.connect", h),
        // No host argument means localhost / unix socket — implicitly
        // allowed; nothing the allowlist could meaningfully gate.
        Expr::NetCreateConnection { host: None, .. } | Expr::NetConnect { host: None, .. } => {}
        _ => {}
    }
    walk_expr_children(expr, &mut |child| visit_expr(child, ctx));
}

fn check_url(ctx: &mut WalkCtx, kind: &'static str, url: &Expr) {
    match url {
        // NOTE: do not collapse this into a match-guard (`Expr::String(s) if ...`).
        // An allowlisted literal must be accepted here, not fall through to the
        // dynamic-host arm below. clippy's collapsible_if/match-guard rewrite
        // changes the semantics; keep the explicit nested `if`.
        Expr::String(s) => {
            if !url_matches_allowlist(s, ctx.allowed_hosts) {
                ctx.violations.push(EgressViolation {
                    source: ctx.source.clone(),
                    kind,
                    literal: Some(s.clone()),
                    reason: EgressRefusalReason::LiteralNotAllowed,
                });
            }
        }
        _ if !ctx.allow_dynamic_hosts => {
            ctx.violations.push(EgressViolation {
                source: ctx.source.clone(),
                kind,
                literal: None,
                reason: EgressRefusalReason::NonLiteralAndDynamicForbidden,
            });
        }
        _ => {}
    }
}

fn check_host(ctx: &mut WalkCtx, kind: &'static str, host: &Expr) {
    match host {
        // NOTE: do not collapse this into a match-guard (`Expr::String(s) if ...`).
        // An allowlisted literal must be accepted here, not fall through to the
        // dynamic-host arm below. clippy's collapsible_if/match-guard rewrite
        // changes the semantics; keep the explicit nested `if`.
        Expr::String(s) => {
            if !host_matches_allowlist(s, ctx.allowed_hosts) {
                ctx.violations.push(EgressViolation {
                    source: ctx.source.clone(),
                    kind,
                    literal: Some(s.clone()),
                    reason: EgressRefusalReason::LiteralNotAllowed,
                });
            }
        }
        _ if !ctx.allow_dynamic_hosts => {
            ctx.violations.push(EgressViolation {
                source: ctx.source.clone(),
                kind,
                literal: None,
                reason: EgressRefusalReason::NonLiteralAndDynamicForbidden,
            });
        }
        _ => {}
    }
}

/// Does `url` (a full URL string) satisfy any of the allowlist
/// patterns? See module docs for pattern shapes.
pub fn url_matches_allowlist(url: &str, patterns: &[String]) -> bool {
    let host = host_of_url(url).unwrap_or(url);
    for pat in patterns {
        if pat == "*" {
            return true;
        }
        // URL-prefix pattern: `https://api.acme.com/v1/*` — the
        // pattern contains a scheme + `/*` suffix. Match if `url`
        // starts with the literal prefix.
        if pat.contains("://") {
            if let Some(prefix) = pat.strip_suffix("/*") {
                if url.starts_with(prefix) {
                    return true;
                }
                continue;
            }
            // Exact URL match (no glob) — rare but representable.
            if pat == url {
                return true;
            }
            continue;
        }
        // Otherwise treat as a host pattern.
        if host_matches_pattern(host, pat) {
            return true;
        }
    }
    false
}

/// Does the bare host string `host` match any of the allowlist
/// patterns? Used for `net.connect(port, host)` where the argument
/// is already a hostname, not a URL.
pub fn host_matches_allowlist(host: &str, patterns: &[String]) -> bool {
    for pat in patterns {
        if pat == "*" {
            return true;
        }
        // URL-shaped patterns (`https://.../*`) don't match a bare
        // host argument — `net.connect("api.example.com")` against
        // a `https://api.example.com/v1/*` allowlist entry should NOT
        // match because the entry is path-bound. Reviewers expect
        // path-bound entries to gate only path-bearing call sites.
        if pat.contains("://") {
            continue;
        }
        if host_matches_pattern(host, pat) {
            return true;
        }
    }
    false
}

fn host_matches_pattern(host: &str, pat: &str) -> bool {
    if pat == host {
        return true;
    }
    if let Some(suffix) = pat.strip_prefix("*.") {
        // `*.cdn.example.com` matches `foo.cdn.example.com`,
        // `a.b.cdn.example.com`, etc. — but NOT the bare suffix.
        return host.ends_with(&format!(".{}", suffix));
    }
    false
}

/// Extract the host component of a URL string. Best-effort: returns
/// `None` if the input doesn't look like an absolute URL with a
/// `scheme://host[...]` shape. Used by `url_matches_allowlist` to
/// reduce a full URL to its host before host-pattern matching.
fn host_of_url(url: &str) -> Option<&str> {
    let after_scheme = url.split_once("://")?.1;
    // Strip path/query/fragment + optional port.
    let host_with_port = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(after_scheme);
    let host = host_with_port
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(host_with_port);
    let host = host.split(':').next().unwrap_or(host);
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pats(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn empty_allowlist_disables_pass() {
        let m = Module::new("test");
        let v = audit_module_egress(&m, "/repo/main.ts", &[], false);
        assert!(v.is_empty());
    }

    #[test]
    fn host_pattern_exact_match() {
        assert!(host_matches_allowlist(
            "api.example.com",
            &pats(&["api.example.com"]),
        ));
        assert!(!host_matches_allowlist(
            "api.example.com",
            &pats(&["other.example.com"]),
        ));
    }

    #[test]
    fn host_pattern_subdomain_wildcard() {
        let allow = pats(&["*.cdn.example.com"]);
        // Subdomains match.
        assert!(host_matches_allowlist("a.cdn.example.com", &allow));
        assert!(host_matches_allowlist("a.b.cdn.example.com", &allow));
        // Bare suffix does NOT match.
        assert!(!host_matches_allowlist("cdn.example.com", &allow));
        // Unrelated hosts don't match.
        assert!(!host_matches_allowlist("evil.com", &allow));
        // Look-alike does NOT match (suffix must be preceded by `.`).
        assert!(!host_matches_allowlist("evilcdn.example.com", &allow));
    }

    #[test]
    fn url_pattern_extracts_host() {
        let allow = pats(&["api.example.com"]);
        assert!(url_matches_allowlist(
            "https://api.example.com/v1/users",
            &allow
        ));
        assert!(url_matches_allowlist(
            "http://api.example.com:8080/v1/users",
            &allow
        ));
        // Userinfo + port don't confuse the extractor.
        assert!(url_matches_allowlist(
            "https://user:pass@api.example.com:443/v1/x?y=1",
            &allow
        ));
        // Different host fails.
        assert!(!url_matches_allowlist("https://evil.com/v1/users", &allow));
    }

    #[test]
    fn url_prefix_pattern() {
        let allow = pats(&["https://api.acme.com/v1/*"]);
        assert!(url_matches_allowlist(
            "https://api.acme.com/v1/users",
            &allow
        ));
        assert!(url_matches_allowlist(
            "https://api.acme.com/v1/posts/42",
            &allow
        ));
        // Different path fails (path-bound prefix).
        assert!(!url_matches_allowlist(
            "https://api.acme.com/v2/users",
            &allow
        ));
        // Different scheme fails.
        assert!(!url_matches_allowlist(
            "http://api.acme.com/v1/users",
            &allow
        ));
    }

    #[test]
    fn universal_escape_hatch() {
        let allow = pats(&["*"]);
        assert!(url_matches_allowlist("https://anywhere.com/x", &allow));
        assert!(host_matches_allowlist("evil.com", &allow));
    }

    #[test]
    fn url_pattern_doesnt_match_bare_host() {
        // `net.connect("api.example.com")` against a URL-prefix
        // allowlist entry should NOT match — the entry restricts
        // paths, the call has no path.
        let allow = pats(&["https://api.example.com/v1/*"]);
        assert!(!host_matches_allowlist("api.example.com", &allow));
    }

    #[test]
    fn fetch_literal_records_violation() {
        let mut m = Module::new("test");
        m.init.push(Stmt::Expr(Expr::FetchWithOptions {
            url: Box::new(Expr::String("https://evil.com/x".into())),
            method: Box::new(Expr::String("GET".into())),
            body: Box::new(Expr::Undefined),
            headers: vec![],
            headers_dynamic: None,
            signal: None,
        }));
        let v = audit_module_egress(&m, "/repo/main.ts", &pats(&["api.example.com"]), false);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].kind, "fetch");
        assert_eq!(v[0].literal.as_deref(), Some("https://evil.com/x"));
        assert_eq!(v[0].reason, EgressRefusalReason::LiteralNotAllowed);
    }

    #[test]
    fn fetch_literal_matching_passes() {
        let mut m = Module::new("test");
        m.init.push(Stmt::Expr(Expr::FetchWithOptions {
            url: Box::new(Expr::String("https://api.example.com/x".into())),
            method: Box::new(Expr::String("GET".into())),
            body: Box::new(Expr::Undefined),
            headers: vec![],
            headers_dynamic: None,
            signal: None,
        }));
        let v = audit_module_egress(&m, "/repo/main.ts", &pats(&["api.example.com"]), false);
        assert!(v.is_empty());
    }

    #[test]
    fn fetch_dynamic_url_blocked_by_default() {
        let mut m = Module::new("test");
        m.init.push(Stmt::Expr(Expr::FetchWithOptions {
            url: Box::new(Expr::LocalGet(0)),
            method: Box::new(Expr::String("GET".into())),
            body: Box::new(Expr::Undefined),
            headers: vec![],
            headers_dynamic: None,
            signal: None,
        }));
        let v = audit_module_egress(&m, "/repo/main.ts", &pats(&["api.example.com"]), false);
        assert_eq!(v.len(), 1);
        assert_eq!(
            v[0].reason,
            EgressRefusalReason::NonLiteralAndDynamicForbidden
        );
        assert!(v[0].literal.is_none());
    }

    #[test]
    fn fetch_dynamic_url_allowed_when_opted_in() {
        let mut m = Module::new("test");
        m.init.push(Stmt::Expr(Expr::FetchWithOptions {
            url: Box::new(Expr::LocalGet(0)),
            method: Box::new(Expr::String("GET".into())),
            body: Box::new(Expr::Undefined),
            headers: vec![],
            headers_dynamic: None,
            signal: None,
        }));
        let v = audit_module_egress(
            &m,
            "/repo/main.ts",
            &pats(&["api.example.com"]),
            true, // allowDynamicHosts
        );
        assert!(v.is_empty());
    }

    #[test]
    fn net_connect_host_checked() {
        let mut m = Module::new("test");
        m.init.push(Stmt::Expr(Expr::NetConnect {
            port: Box::new(Expr::Number(443.0)),
            host: Some(Box::new(Expr::String("evil.com".into()))),
            connect_listener: None,
        }));
        let v = audit_module_egress(&m, "/repo/main.ts", &pats(&["api.example.com"]), false);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].kind, "net.connect");
    }

    #[test]
    fn net_connect_no_host_implicit_localhost_allowed() {
        let mut m = Module::new("test");
        m.init.push(Stmt::Expr(Expr::NetConnect {
            port: Box::new(Expr::Number(8080.0)),
            host: None,
            connect_listener: None,
        }));
        let v = audit_module_egress(&m, "/repo/main.ts", &pats(&["api.example.com"]), false);
        assert!(v.is_empty());
    }
}
