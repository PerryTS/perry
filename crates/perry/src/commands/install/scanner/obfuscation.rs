//! P0 rule #4: obfuscation heuristics on the package's entry-point JS.
//!
//! v1 has one signal: **embedded base64 blob** — any entry file that
//! contains a quoted-string literal ≥ 1,000 chars of base64-alphabet
//! content. This catches the payload-as-string-constant pattern (the
//! most common droppers use this shape: `eval(atob("...big base64..."))`,
//! though here we flag the blob even if the surrounding code looks
//! innocuous).
//!
//! A "dropper-shape" (small entry file with one very long line) rule
//! was tried in an earlier revision but false-positived on legitimate
//! bundled CJS distributed by mainstream packages (AWS SDK ecosystem:
//! `path-expression-matcher`, `fast-xml-builder`, `bowser`). The shape
//! "small file + one giant line of dense JS" turns out to be the
//! standard bundled-output shape for a lot of npm utility libs, not a
//! distinguishing dropper signal on its own. Removed in favor of the
//! eval+atob / Function+Buffer rules in `patterns.rs`, which catch
//! the most common real-world dropper shape directly.

use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::report::{Finding, Severity};
use super::ScannedPackage;

const MAX_FILE_BYTES: u64 = 512 * 1024;
const BASE64_MIN_LEN: usize = 1_000;

fn base64_blob_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        // Quoted literal of >= 1000 chars in the base64 / base64url
        // alphabet, optionally trailing '='. Matches "...", '...', or
        // `...` delimiters.
        Regex::new(
            r#"(?:"[A-Za-z0-9+/_=-]{1000,}"|'[A-Za-z0-9+/_=-]{1000,}'|`[A-Za-z0-9+/_=-]{1000,}`)"#,
        )
        .expect("hardcoded scanner regex compiles")
    })
}

pub fn check(pkg: &ScannedPackage) -> Vec<Finding> {
    let mut findings = Vec::new();
    for entry in collect_entry_files(pkg) {
        let meta = match fs::metadata(&entry) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.len() > MAX_FILE_BYTES {
            continue;
        }
        let content = match fs::read_to_string(&entry) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let rel = entry
            .strip_prefix(&pkg.dir)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| entry.display().to_string());

        if base64_blob_regex().is_match(&content) {
            findings.push(Finding {
                package: format!("{}@{}", pkg.name, pkg.version),
                package_path: pkg.dir.display().to_string(),
                severity: Severity::P0,
                rule: "obfuscation-base64-blob".to_string(),
                message: format!(
                    "entry file contains a quoted base64-alphabet string ≥ {} chars — \
                     likely an embedded payload",
                    BASE64_MIN_LEN
                ),
                location: Some(rel),
                overridden: false,
            });
        }
    }
    findings
}

// Entry-point resolution — same shape as `patterns::collect_entry_files`,
// kept private here to avoid cross-rule coupling. (If a third rule grows
// up to want the same logic, it'll be worth refactoring out.)
fn collect_entry_files(pkg: &ScannedPackage) -> Vec<PathBuf> {
    use serde_json::Value;

    let mut candidates: Vec<PathBuf> = Vec::new();
    let main = pkg
        .manifest
        .get("main")
        .and_then(|v| v.as_str())
        .unwrap_or("index.js");
    candidates.push(pkg.dir.join(main));
    if let Some(m) = pkg.manifest.get("module").and_then(|v| v.as_str()) {
        candidates.push(pkg.dir.join(m));
    }
    if let Some(bin) = pkg.manifest.get("bin") {
        match bin {
            Value::String(s) => candidates.push(pkg.dir.join(s)),
            Value::Object(map) => {
                for v in map.values() {
                    if let Some(s) = v.as_str() {
                        candidates.push(pkg.dir.join(s));
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(exports) = pkg.manifest.get("exports") {
        collect_exports_paths(exports, &pkg.dir, &mut candidates);
    }
    let mut resolved = Vec::new();
    for cand in candidates {
        if let Some(r) = resolve_entry(&cand) {
            if !resolved.contains(&r) {
                resolved.push(r);
            }
        }
    }
    resolved
}

fn resolve_entry(path: &Path) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.to_path_buf());
    }
    for ext in &["js", "mjs", "cjs"] {
        let with_ext = path.with_extension(ext);
        if with_ext.is_file() {
            return Some(with_ext);
        }
    }
    if path.is_dir() {
        for fname in &["index.js", "index.mjs", "index.cjs"] {
            let p = path.join(fname);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

fn collect_exports_paths(value: &serde_json::Value, base: &Path, out: &mut Vec<PathBuf>) {
    use serde_json::Value;
    match value {
        Value::String(s) if s.starts_with("./") => {
            out.push(base.join(s));
        }
        Value::Object(map) => {
            for v in map.values() {
                collect_exports_paths(v, base, out);
            }
        }
        Value::Array(items) => {
            for v in items {
                collect_exports_paths(v, base, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    fn make_pkg(
        td: &TempDir,
        manifest: serde_json::Value,
        files: &[(&str, &str)],
    ) -> ScannedPackage {
        let dir = td.path().to_path_buf();
        fs::write(dir.join("package.json"), manifest.to_string()).unwrap();
        for (name, body) in files {
            let full = dir.join(name);
            if let Some(parent) = full.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(full, body).unwrap();
        }
        ScannedPackage {
            name: manifest["name"].as_str().unwrap_or("test").into(),
            version: manifest["version"].as_str().unwrap_or("0").into(),
            dir,
            manifest,
        }
    }

    #[test]
    fn does_not_flag_bundled_cjs_long_lines() {
        // Real-world false-positive case: AWS SDK bundled-CJS packages
        // (path-expression-matcher, bowser, fast-xml-builder) ship
        // their main entry as a single long line of dense JS. The
        // earlier "dropper-shape" rule flagged this; we removed it.
        let td = TempDir::new().unwrap();
        let mut body = String::from("module.exports = ");
        body.push_str(&"a".repeat(6000));
        let p = make_pkg(
            &td,
            json!({"name":"bundled-lib","version":"1","main":"index.js"}),
            &[("index.js", body.as_str())],
        );
        assert!(check(&p).is_empty());
    }

    #[test]
    fn flags_base64_blob_double_quoted() {
        let td = TempDir::new().unwrap();
        // 1100 chars of base64 alphabet inside a double-quoted literal.
        let blob: String = "A".repeat(1100);
        let body = format!("const payload = \"{}\";\nconsole.log(payload);", blob);
        let p = make_pkg(
            &td,
            json!({"name":"b","version":"1","main":"index.js"}),
            &[("index.js", body.as_str())],
        );
        assert!(check(&p)
            .iter()
            .any(|f| f.rule == "obfuscation-base64-blob"));
    }

    #[test]
    fn flags_base64_blob_single_quoted() {
        let td = TempDir::new().unwrap();
        let blob: String = "B".repeat(1100);
        let body = format!("const p = '{}';", blob);
        let p = make_pkg(
            &td,
            json!({"name":"b","version":"1","main":"index.js"}),
            &[("index.js", body.as_str())],
        );
        assert!(check(&p)
            .iter()
            .any(|f| f.rule == "obfuscation-base64-blob"));
    }

    #[test]
    fn does_not_flag_short_strings() {
        let td = TempDir::new().unwrap();
        let p = make_pkg(
            &td,
            json!({"name":"s","version":"1","main":"index.js"}),
            &[(
                "index.js",
                "const small = 'aGVsbG8gd29ybGQ='; module.exports = small;",
            )],
        );
        assert!(check(&p).is_empty());
    }

    #[test]
    fn does_not_flag_normal_code() {
        let td = TempDir::new().unwrap();
        let p = make_pkg(
            &td,
            json!({"name":"g","version":"1","main":"index.js"}),
            &[(
                "index.js",
                "module.exports = function (a, b) { return a + b; }",
            )],
        );
        assert!(check(&p).is_empty());
    }
}
