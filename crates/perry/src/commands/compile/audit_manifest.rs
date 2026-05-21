//! #495 / #497 / #499 supply-chain bookkeeping helpers.
//!
//! Three small concerns lifted out of the `compile.rs` orchestrator:
//!
//! - `package_name_for_path` — used by the perry-jsruntime refusal diagnostic
//!   (#499) to attribute a JS-runtime importer back to its owning npm package.
//! - `write_audit_manifest` / `write_audit_manifest_logging_failures` — emit
//!   `.perry-cache/audit.json` (#495 behavioral SBOM).
//! - `allowlist_matches` — the host-allowlist pattern matcher for
//!   `perry.nativeLibrary` / `perry.compilePackages` (#497).

use std::fs;

use crate::OutputFormat;

use super::CompilationContext;

/// #499: extract the owning npm package name from a source-file path
/// by locating the rightmost `node_modules/` segment. Scope-aware:
/// `node_modules/@scope/pkg/...` returns `@scope/pkg`. Returns `None`
/// for user-source files outside `node_modules/`.
pub(super) fn package_name_for_path(source_path: &str) -> Option<String> {
    let idx = source_path.rfind("node_modules/")?;
    let after = &source_path[idx + "node_modules/".len()..];
    if let Some(stripped) = after.strip_prefix('@') {
        let mut parts = stripped.splitn(3, '/');
        let scope = parts.next().unwrap_or("");
        let pkg = parts.next().unwrap_or("");
        if scope.is_empty() || pkg.is_empty() {
            None
        } else {
            Some(format!("@{}/{}", scope, pkg))
        }
    } else {
        let pkg = after.split('/').next()?;
        if pkg.is_empty() {
            None
        } else {
            Some(pkg.to_string())
        }
    }
}

/// #495: serialize the per-module behavioral SBOM to
/// `.perry-cache/audit.json` under the current project root. Walks
/// every collected native HIR module (skips JS-runtime modules — they
/// don't have HIR), groups records by stable canonical-path order
/// so the JSON is byte-deterministic across builds.
fn write_audit_manifest(ctx: &CompilationContext) -> std::io::Result<()> {
    let mut manifest = perry_hir::AuditManifest::new();
    // BTreeMap iteration is sorted by key; native_modules is keyed by
    // PathBuf so the resulting Vec is in stable filesystem order.
    for (path, hir_module) in &ctx.native_modules {
        let source = path.to_string_lossy().into_owned();
        let record = perry_hir::audit_module(hir_module, &source);
        manifest.modules.push(record);
    }
    let dir = ctx.project_root.join(".perry-cache");
    fs::create_dir_all(&dir)?;
    let path = dir.join("audit.json");
    let json = serde_json::to_string_pretty(&manifest)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    fs::write(&path, json)?;
    Ok(())
}

/// #497: does `name` match any entry in the host allowlist?
///
/// Pattern syntax:
/// - exact match (`"lodash"`, `"@perryts/perry-prisma"`)
/// - scope wildcard (`"@scope/*"` matches any package under `@scope/`)
/// - universal `"*"` escape hatch (matches every name)
///
/// Empty allowlists never match anything — the supply-chain default
/// is "nothing allowed" per the issue's "Default behavior on
/// greenfield projects" acceptance bullet.
pub(crate) fn allowlist_matches(name: &str, patterns: &[String]) -> bool {
    for pat in patterns {
        if pat == "*" {
            return true;
        }
        if pat == name {
            return true;
        }
        // Scope wildcard `@scope/*` — matches anything under `@scope/`.
        if let Some(prefix) = pat.strip_suffix("/*") {
            // Both `prefix/anything` and the bare prefix (an unscoped
            // package author publishing `@scope` itself, rare but
            // representable) match.
            if let Some(rest) = name.strip_prefix(prefix) {
                if rest.starts_with('/') || rest.is_empty() {
                    return true;
                }
            }
        }
    }
    false
}

/// #495 wrapper: emit the behavioral SBOM at `.perry-cache/audit.json`,
/// best-effort. The SBOM is observational metadata, not a correctness
/// gate, so an I/O failure becomes a warning and the build continues.
pub(super) fn write_audit_manifest_logging_failures(
    ctx: &CompilationContext,
    format: OutputFormat,
) {
    if let Err(e) = write_audit_manifest(ctx) {
        match format {
            OutputFormat::Text => {
                eprintln!("warning: failed to write .perry-cache/audit.json: {}", e);
            }
            OutputFormat::Json => {}
        }
    }
}

#[cfg(test)]
mod js_runtime_gate_tests {
    //! #499 — coverage for the helper that names the offending
    //! package in the perry-jsruntime refusal diagnostic.
    //!
    //! The end-to-end gate fires deep inside `compile_command` after
    //! `collect_modules` runs and exits via `anyhow::bail!`; building
    //! a unit test that drives it requires a full project on disk
    //! (tested via the smoke test in the PR description). Here we
    //! pin the importer-attribution helper that the diagnostic uses
    //! — the part that historically rotted in #467's
    //! `sanitize_app_name` because no test covered it.

    use super::package_name_for_path;

    #[test]
    fn unscoped_package_under_node_modules() {
        assert_eq!(
            package_name_for_path("/repo/node_modules/lodash/lib/index.js"),
            Some("lodash".to_string())
        );
    }

    #[test]
    fn scoped_package_under_node_modules() {
        assert_eq!(
            package_name_for_path("/repo/node_modules/@scope/pkg/src/index.js"),
            Some("@scope/pkg".to_string())
        );
    }

    #[test]
    fn nested_node_modules_returns_innermost() {
        // Bun/pnpm-style nested node_modules: the rightmost segment
        // is the one that actually resolved.
        assert_eq!(
            package_name_for_path("/repo/node_modules/outer/node_modules/inner/lib/index.js"),
            Some("inner".to_string())
        );
    }

    #[test]
    fn user_source_returns_none() {
        assert_eq!(package_name_for_path("/repo/src/main.ts"), None);
    }

    #[test]
    fn malformed_scope_returns_none() {
        // `@/foo` is malformed (empty scope) — don't claim a package
        // name we can't actually quote back to the user.
        assert_eq!(
            package_name_for_path("/repo/node_modules/@/foo/index.js"),
            None
        );
        // Empty segment immediately after `node_modules/` — defensive
        // coverage; the diagnostic falls through to printing only the
        // raw path.
        assert_eq!(package_name_for_path("/repo/node_modules/"), None);
    }
}

#[cfg(test)]
mod allowlist_tests {
    //! #497 — coverage for the host-allowlist matcher used to gate
    //! `perry.nativeLibrary` and `perry.compilePackages`.
    use super::allowlist_matches;

    fn pats(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn empty_allowlist_blocks_everything() {
        // Acceptance bullet: "Default behavior on greenfield
        // projects: nothing allowed." The empty list never matches.
        assert!(!allowlist_matches("lodash", &[]));
        assert!(!allowlist_matches("@scope/pkg", &[]));
        assert!(!allowlist_matches("", &[]));
    }

    #[test]
    fn exact_match() {
        let allow = pats(&["lodash", "@scope/pkg"]);
        assert!(allowlist_matches("lodash", &allow));
        assert!(allowlist_matches("@scope/pkg", &allow));
        assert!(!allowlist_matches("not-listed", &allow));
        assert!(!allowlist_matches("lodash-suffix", &allow));
    }

    #[test]
    fn universal_wildcard() {
        // Emergency escape hatch. Matches every name including the
        // empty string (defensive).
        let allow = pats(&["*"]);
        assert!(allowlist_matches("anything", &allow));
        assert!(allowlist_matches("@scope/whatever", &allow));
        assert!(allowlist_matches("", &allow));
    }

    #[test]
    fn scope_wildcard() {
        // `@scope/*` matches any package under the scope, but NOT
        // arbitrary other scopes or unrelated names.
        let allow = pats(&["@perryts/*"]);
        assert!(allowlist_matches("@perryts/perry-prisma", &allow));
        assert!(allowlist_matches("@perryts/redis", &allow));
        // Subpath-style names within the scope: `@perryts/foo/bar` is
        // *not* a valid npm package name, but if someone slips one
        // through the patterns still match the leading scope.
        assert!(allowlist_matches("@perryts/foo/bar", &allow));
        // Different scope must not match.
        assert!(!allowlist_matches("@other/pkg", &allow));
        // Unscoped names must not match a scoped pattern.
        assert!(!allowlist_matches("perryts-redis", &allow));
        // Bare scope must not match `@perryts-foo` (different scope).
        assert!(!allowlist_matches("@perryts-foo/pkg", &allow));
    }

    #[test]
    fn non_scoped_wildcards_dont_match() {
        // Only the slash-anchored `prefix/*` shape is supported. A
        // freeform `lodash-*` pattern matches nothing — kept simple
        // on purpose so reviewers can read the rule at a glance:
        // exact, universal, or scope-wildcard.
        let allow = pats(&["lodash-*", "*-tests"]);
        assert!(!allowlist_matches("lodash-es", &allow));
        assert!(!allowlist_matches("foo-tests", &allow));
    }

    #[test]
    fn multiple_patterns_or_together() {
        let allow = pats(&["lodash", "@scope/*"]);
        assert!(allowlist_matches("lodash", &allow));
        assert!(allowlist_matches("@scope/anything", &allow));
        assert!(!allowlist_matches("other-pkg", &allow));
    }
}
