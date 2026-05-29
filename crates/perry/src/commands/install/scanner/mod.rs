//! Scanner: walks `node_modules/`, runs rule modules, produces a
//! verdict + report. Wired into the install pipeline between the
//! installer subprocess and any script execution.
//!
//! Phase 3 introduces the walk and the empty report. P0 rules
//! (Phases 4-7) plug into [`scan_package`].

use anyhow::Result;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

pub mod obfuscation;
pub mod patterns;
pub mod report;
pub mod scripts;
pub mod typosquat;

use report::{Finding, ScanReport, Severity, Verdict};

/// One discovered package on disk.
#[derive(Debug, Clone)]
pub struct ScannedPackage {
    pub name: String,
    pub version: String,
    pub dir: PathBuf,
    /// Parsed `package.json`. Kept around so individual rules don't
    /// re-read + re-parse it.
    pub manifest: Value,
}

/// Walk a `node_modules/` directory and return every package found,
/// recursing into nested `node_modules/` (npm's hoist-fallback layout).
pub fn discover_packages(node_modules: &Path) -> Vec<ScannedPackage> {
    let mut out = Vec::new();
    walk(node_modules, &mut out);
    out
}

fn walk(node_modules: &Path, out: &mut Vec<ScannedPackage>) {
    let entries = match fs::read_dir(node_modules) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            // Most dot-prefixed entries are npm bookkeeping (.bin,
            // .package-lock.json, .cache) and contain no packages. The
            // exceptions are bun's `.bun/` and pnpm's `.pnpm/` —
            // isolated-mode content-addressable stores where each
            // subdir is `<name>@<version>/` and the actual package
            // lives in its own nested `node_modules/<name>/`.
            if name == ".bun" || name == ".pnpm" {
                walk_isolated_store(&entry.path(), out);
            }
            continue;
        }
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if name.starts_with('@') {
            // Scope directory — packages live one level down.
            if let Ok(scope_entries) = fs::read_dir(&path) {
                for scoped in scope_entries.flatten() {
                    let scoped_name = format!("{}/{}", name, scoped.file_name().to_string_lossy());
                    record_and_recurse(&scoped.path(), &scoped_name, out);
                }
            }
        } else {
            record_and_recurse(&path, &name, out);
        }
    }
}

/// Walk a bun / pnpm isolated-mode store: each immediate subdir is
/// `<pkg>@<ver>/`, and the real extracted package sits at
/// `<pkg>@<ver>/node_modules/<pkg>/`. We recurse into each inner
/// node_modules so the scanner sees the real packages.
fn walk_isolated_store(store_root: &Path, out: &mut Vec<ScannedPackage>) {
    let entries = match fs::read_dir(store_root) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let inner = path.join("node_modules");
        if inner.is_dir() {
            walk(&inner, out);
        }
    }
}

fn record_and_recurse(pkg_dir: &Path, dir_name: &str, out: &mut Vec<ScannedPackage>) {
    if !pkg_dir.is_dir() {
        return;
    }
    let manifest_path = pkg_dir.join("package.json");
    if let Ok(content) = fs::read_to_string(&manifest_path) {
        if let Ok(manifest) = serde_json::from_str::<Value>(&content) {
            // Prefer the manifest-declared name (handles renamed deps),
            // but fall back to the directory name when missing.
            let name = manifest
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or(dir_name)
                .to_string();
            let version = manifest
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string();
            out.push(ScannedPackage {
                name,
                version,
                dir: pkg_dir.to_path_buf(),
                manifest,
            });
        }
    }
    let nested = pkg_dir.join("node_modules");
    if nested.is_dir() {
        walk(&nested, out);
    }
}

/// Run all enabled rules against a single package. Each rule module
/// returns its own findings; this function concatenates them.
pub fn scan_package(pkg: &ScannedPackage) -> Vec<Finding> {
    let mut findings = Vec::new();
    findings.extend(scripts::check(pkg));
    findings.extend(patterns::check(pkg));
    findings.extend(typosquat::check(pkg));
    findings.extend(obfuscation::check(pkg));
    findings
}

/// Top-level entry: walk the tree, run rules, build the report.
/// `allow_risky` / `allow_risky_all` downgrade matching P0 findings to
/// "overridden" rather than blocking.
///
/// Convenience wrapper around [`discover_packages`] + [`scan_packages`]
/// for callers that don't need the package list afterwards. Phase 9
/// (lifecycle execution) uses the split form so it can reuse the walk.
// #854: convenience wrapper over discover_packages + scan_packages; callers
// currently use the split form, but this is the documented one-shot entry.
#[allow(dead_code)]
pub fn scan_tree(
    node_modules: &Path,
    allow_risky: &[String],
    allow_risky_all: bool,
) -> Result<ScanReport> {
    let packages = discover_packages(node_modules);
    Ok(scan_packages(&packages, allow_risky, allow_risky_all))
}

/// Run rules over a pre-walked package list, producing the report.
pub fn scan_packages(
    packages: &[ScannedPackage],
    allow_risky: &[String],
    allow_risky_all: bool,
) -> ScanReport {
    let mut findings = Vec::new();
    for pkg in packages {
        for finding in scan_package(pkg) {
            findings.push(finding);
        }
    }

    // Mark each P0 as overridden if the user opted-in for it.
    let mut blocked = 0usize;
    let mut overridden = 0usize;
    for finding in &mut findings {
        if !matches!(finding.severity, Severity::P0) {
            continue;
        }
        let is_overridden = allow_risky_all
            || allow_risky
                .iter()
                .any(|p| package_matches(p, &finding.package));
        if is_overridden {
            finding.overridden = true;
            overridden += 1;
        } else {
            blocked += 1;
        }
    }

    let verdict = if blocked > 0 {
        Verdict::Blocked
    } else if overridden > 0 {
        Verdict::Overridden
    } else {
        Verdict::Clean
    };

    ScanReport {
        scanned_at: chrono::Utc::now().to_rfc3339(),
        package_count: packages.len(),
        findings,
        verdict,
    }
}

/// Match a user-supplied `--allow-risky <pat>` against a finding's
/// package identifier. The identifier is `name@version`. `pat` matches
/// if it equals the bare name (any version) or the full `name@version`.
fn package_matches(pat: &str, package_id: &str) -> bool {
    if pat == package_id {
        return true;
    }
    let bare_name = package_id
        .rsplit_once('@')
        .map(|(n, _)| n)
        .unwrap_or(package_id);
    // For scoped names @scope/pkg@version, rsplit_once on '@' would
    // strip the version correctly because rsplit takes the *last* '@'.
    pat == bare_name
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_pkg(root: &Path, dir: &str, name: &str, version: &str) {
        let pkg_dir = root.join("node_modules").join(dir);
        fs::create_dir_all(&pkg_dir).unwrap();
        let manifest = format!(r#"{{"name":"{}","version":"{}"}}"#, name, version);
        fs::write(pkg_dir.join("package.json"), manifest).unwrap();
    }

    #[test]
    fn discover_flat_packages() {
        let td = TempDir::new().unwrap();
        write_pkg(td.path(), "lodash", "lodash", "4.17.21");
        write_pkg(td.path(), "chalk", "chalk", "5.3.0");
        let pkgs = discover_packages(&td.path().join("node_modules"));
        assert_eq!(pkgs.len(), 2);
        let names: Vec<_> = pkgs.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"lodash"));
        assert!(names.contains(&"chalk"));
    }

    #[test]
    fn discover_scoped_packages() {
        let td = TempDir::new().unwrap();
        write_pkg(td.path(), "@scope/foo", "@scope/foo", "1.0.0");
        let pkgs = discover_packages(&td.path().join("node_modules"));
        assert_eq!(pkgs.len(), 1);
        assert_eq!(pkgs[0].name, "@scope/foo");
    }

    #[test]
    fn discover_nested_packages() {
        let td = TempDir::new().unwrap();
        write_pkg(td.path(), "outer", "outer", "1.0.0");
        // Nested node_modules inside outer/
        let nested = td.path().join("node_modules/outer/node_modules/inner");
        fs::create_dir_all(&nested).unwrap();
        fs::write(
            nested.join("package.json"),
            r#"{"name":"inner","version":"2.0.0"}"#,
        )
        .unwrap();
        let pkgs = discover_packages(&td.path().join("node_modules"));
        assert_eq!(pkgs.len(), 2);
    }

    #[test]
    fn skip_dot_dirs() {
        let td = TempDir::new().unwrap();
        write_pkg(td.path(), "lodash", "lodash", "4.17.21");
        // .bin is npm's symlink farm — should be skipped.
        fs::create_dir_all(td.path().join("node_modules/.bin")).unwrap();
        let pkgs = discover_packages(&td.path().join("node_modules"));
        assert_eq!(pkgs.len(), 1);
    }

    #[test]
    fn discover_bun_isolated_store() {
        // Bun's workspace / isolated mode layout:
        //   node_modules/.bun/lodash@4.17.21/node_modules/lodash/package.json
        // Our walker should descend into .bun/ and pick up the real package.
        let td = TempDir::new().unwrap();
        let inner = td
            .path()
            .join("node_modules/.bun/lodash@4.17.21/node_modules/lodash");
        fs::create_dir_all(&inner).unwrap();
        fs::write(
            inner.join("package.json"),
            r#"{"name":"lodash","version":"4.17.21"}"#,
        )
        .unwrap();
        let pkgs = discover_packages(&td.path().join("node_modules"));
        assert_eq!(pkgs.len(), 1);
        assert_eq!(pkgs[0].name, "lodash");
    }

    #[test]
    fn discover_pnpm_isolated_store() {
        // pnpm's layout is structurally the same as bun's.
        let td = TempDir::new().unwrap();
        let inner = td
            .path()
            .join("node_modules/.pnpm/chalk@5.3.0/node_modules/chalk");
        fs::create_dir_all(&inner).unwrap();
        fs::write(
            inner.join("package.json"),
            r#"{"name":"chalk","version":"5.3.0"}"#,
        )
        .unwrap();
        let pkgs = discover_packages(&td.path().join("node_modules"));
        assert_eq!(pkgs.len(), 1);
        assert_eq!(pkgs[0].name, "chalk");
    }

    #[test]
    fn allow_risky_bare_name_matches_any_version() {
        assert!(package_matches("expres", "expres@0.0.1"));
        assert!(package_matches("expres", "expres@9.9.9"));
        assert!(!package_matches("expres", "express@4.18.2"));
    }

    #[test]
    fn allow_risky_exact_version_matches_only_that() {
        assert!(package_matches("expres@0.0.1", "expres@0.0.1"));
        assert!(!package_matches("expres@0.0.1", "expres@0.0.2"));
    }
}
