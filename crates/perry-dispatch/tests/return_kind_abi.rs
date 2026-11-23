//! ABI test: a `ReturnKind::F64` row must be backed by native functions that
//! really return `f64` on every backend.
//!
//! Codegen reads an `F64` result out of the floating-point return register.
//! If a backend's `extern "C"` function returns `i64` instead, the value sits
//! in the integer register and callers read unrelated FP state. That is what
//! broke `audioStart()`: every backend except watchOS returned `i64 1`, the row
//! said `F64`, and callers saw `4.25e-314` / `0` while capture was running.
//! Integer-returning symbols must use `I64AsF64` (or `I64AsBool`).

use perry_dispatch::{MethodRow, ReturnKind, PERRY_SYSTEM_TABLE};
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Return types of every `pub extern "C" fn <symbol>(…)` found in the
/// `perry-ui-*` backend crates, as (crate-relative path, return type).
fn native_returns(symbol: &str, files: &[(PathBuf, String)]) -> Vec<(String, String)> {
    let needle = format!("pub extern \"C\" fn {symbol}(");
    let mut found = Vec::new();
    for (path, src) in files {
        let mut rest = src.as_str();
        while let Some(i) = rest.find(&needle) {
            let after = &rest[i + needle.len()..];
            let close = after.find(')').unwrap_or(0);
            let sig_tail = &after[close + 1..];
            let body = sig_tail.find('{').unwrap_or(sig_tail.len());
            let ret = sig_tail[..body].trim();
            let ret = ret.strip_prefix("->").map(str::trim).unwrap_or("()");
            found.push((path.display().to_string(), ret.to_string()));
            rest = &after[close..];
        }
    }
    found
}

fn check_table(name: &str, table: &[MethodRow], files: &[(PathBuf, String)]) -> Vec<String> {
    let mut bad = Vec::new();
    for row in table {
        if !matches!(row.ret, ReturnKind::F64) {
            continue;
        }
        for (path, ret) in native_returns(row.runtime, files) {
            if ret != "f64" {
                bad.push(format!(
                    "{name} row '{}' is ReturnKind::F64 but {path} defines {}() -> {ret}",
                    row.method, row.runtime
                ));
            }
        }
    }
    bad
}

#[test]
fn f64_rows_are_backed_by_f64_natives() {
    let crates_dir = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut paths = Vec::new();
    for e in std::fs::read_dir(crates_dir).unwrap().flatten() {
        if e.file_name().to_string_lossy().starts_with("perry-ui-") {
            rust_files(&e.path().join("src"), &mut paths);
        }
    }
    let files: Vec<(PathBuf, String)> = paths
        .into_iter()
        .filter_map(|p| std::fs::read_to_string(&p).ok().map(|s| (p, s)))
        .collect();
    assert!(!files.is_empty(), "no perry-ui-* backend sources found");

    let bad = check_table("PERRY_SYSTEM_TABLE", PERRY_SYSTEM_TABLE, &files);
    assert!(
        bad.is_empty(),
        "return-kind / native ABI mismatches:\n{}",
        bad.join("\n")
    );
}
