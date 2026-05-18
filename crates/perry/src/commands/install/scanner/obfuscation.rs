//! P0 rule #4: obfuscation heuristics on the package's entry-point JS.
//!
//! v1 flagged any quoted ≥ 1,000-char base64-alphabet blob as a likely
//! payload. That false-positived on legitimate packages that embed
//! large encoded data tables: Unicode normalization data
//! (`@adraffy/ens-normalize`), WebAssembly module bytes
//! (`cjs-module-lexer`), and crypto / ABI / signature lookup tables
//! (`ox`). The blob shape alone isn't distinctive — what makes a blob
//! suspicious is whether it ends up in a *code execution* sink.
//!
//! v2 only flags when a base64 blob and a code-execution sink (eval,
//! `new Function`, `Function(`, `vm.runInThisContext`,
//! `vm.runInNewContext`, `vm.compileFunction`) both appear in the same
//! file. This kills the lookup-table false positives while still
//! catching the spread-across-the-file decode-and-execute shape that
//! the tighter `patterns.rs::src-eval-atob` / `src-eval-buffer-base64`
//! rules (which require eval and decode to be within a few hundred
//! chars) would miss.
//!
//! `atob` and `Buffer.from(..., 'base64')` are *not* treated as
//! execution sinks on their own — both have many legitimate decode-only
//! uses (token parsing, image data, embedded font payloads, Unicode
//! tables). They only count when paired with eval/Function nearby, and
//! that adjacency case is already covered by `patterns.rs`.

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
        Regex::new(
            r#"(?:"[A-Za-z0-9+/_=-]{1000,}"|'[A-Za-z0-9+/_=-]{1000,}'|`[A-Za-z0-9+/_=-]{1000,}`)"#,
        )
        .expect("hardcoded scanner regex compiles")
    })
}

fn exec_sink_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(
            r"(?i)\b(eval|new\s+Function|Function|vm\.runInThisContext|vm\.runInNewContext|vm\.compileFunction)\s*\(",
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

        if base64_blob_regex().is_match(&content) && exec_sink_regex().is_match(&content) {
            findings.push(Finding {
                package: format!("{}@{}", pkg.name, pkg.version),
                package_path: pkg.dir.display().to_string(),
                severity: Severity::P0,
                rule: "obfuscation-base64-blob".to_string(),
                message: format!(
                    "entry file contains a quoted base64-alphabet string ≥ {} chars \
                     alongside an eval/Function call — likely a decode-and-execute payload",
                    BASE64_MIN_LEN
                ),
                location: Some(rel),
                overridden: false,
            });
        }
    }
    findings
}

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
    fn flags_base64_blob_with_eval_sink() {
        let td = TempDir::new().unwrap();
        let blob: String = "A".repeat(1100);
        let body = format!(
            "const payload = \"{blob}\";\n\
             eval(decode(payload));"
        );
        let p = make_pkg(
            &td,
            json!({"name":"b","version":"1","main":"index.js"}),
            &[("index.js", body.as_str())],
        );
        assert!(check(&p).iter().any(|f| f.rule == "obfuscation-base64-blob"));
    }

    #[test]
    fn flags_base64_blob_with_new_function_sink() {
        let td = TempDir::new().unwrap();
        let blob: String = "B".repeat(1100);
        let body = format!(
            "const data = '{blob}';\n\
             const fn = new Function('return ' + decode(data));\n\
             fn();"
        );
        let p = make_pkg(
            &td,
            json!({"name":"b","version":"1","main":"index.js"}),
            &[("index.js", body.as_str())],
        );
        assert!(check(&p).iter().any(|f| f.rule == "obfuscation-base64-blob"));
    }

    #[test]
    fn does_not_flag_blob_alone() {
        // Lookup-table shape: large base64 blob, decoded with atob but
        // *not* fed to eval/Function. Mirrors @adraffy/ens-normalize and
        // similar Unicode-data packages.
        let td = TempDir::new().unwrap();
        let blob: String = "C".repeat(1100);
        let body = format!(
            "const TABLE = atob(\"{blob}\");\n\
             module.exports = function lookup(c) {{ return TABLE.charCodeAt(c); }};"
        );
        let p = make_pkg(
            &td,
            json!({"name":"normalizer","version":"1","main":"index.js"}),
            &[("index.js", body.as_str())],
        );
        assert!(
            check(&p).is_empty(),
            "decode-only blob (no execution sink) should not fire"
        );
    }

    #[test]
    fn does_not_flag_wasm_blob() {
        // cjs-module-lexer shape: embedded WASM bytes, instantiated via
        // WebAssembly.* APIs. No eval/Function.
        let td = TempDir::new().unwrap();
        let blob: String = "D".repeat(1100);
        let body = format!(
            "const bytes = Buffer.from(\"{blob}\", 'base64');\n\
             WebAssembly.instantiate(bytes).then(m => module.exports = m.instance.exports);"
        );
        let p = make_pkg(
            &td,
            json!({"name":"wasm-lib","version":"1","main":"index.js"}),
            &[("index.js", body.as_str())],
        );
        assert!(
            check(&p).is_empty(),
            "WebAssembly host call should not count as an execution sink"
        );
    }

    #[test]
    fn does_not_flag_short_strings() {
        let td = TempDir::new().unwrap();
        let p = make_pkg(
            &td,
            json!({"name":"s","version":"1","main":"index.js"}),
            &[(
                "index.js",
                "const small = 'aGVsbG8gd29ybGQ='; eval(atob(small));",
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

    #[test]
    fn does_not_flag_bundled_cjs_long_lines() {
        // Real-world false-positive case: bundled CJS with long-line
        // dense JS but no base64-shape blob and no eval.
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
    fn flags_when_blob_and_sink_are_far_apart() {
        // Spread shape: blob at top of file, eval at bottom. The tighter
        // patterns.rs rules (with their 200-400 char windows) miss this,
        // which is exactly why this rule exists.
        let td = TempDir::new().unwrap();
        let blob: String = "E".repeat(1100);
        let body = format!(
            "const payload = \"{blob}\";\n\
             {filler}\n\
             eval(decode(payload));",
            filler = "// some comment\n".repeat(500)
        );
        let p = make_pkg(
            &td,
            json!({"name":"sneaky","version":"1","main":"index.js"}),
            &[("index.js", body.as_str())],
        );
        assert!(check(&p).iter().any(|f| f.rule == "obfuscation-base64-blob"));
    }
}
