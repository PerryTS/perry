//! P3 B1 (DESIGN-p3 rev 3.1 §2.2, REVIEW-p3 D2): every non-test use of
//! `prim::NUM` in perry-wpfacts has a row in `NUM_SEEDS.md` citing the runtime
//! function or lowering that produces the value. The counts are compared per
//! file, so moving a line does not break the test; adding a seed does, until
//! its row (and its audit) is added.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// `prim::NUM` uses outside `#[cfg(test)]` modules and test files.
fn source_counts(root: &Path) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path: PathBuf = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if !name.ends_with(".rs")
                || name == "tests.rs"
                || name.ends_with("_tests.rs")
                || name.ends_with("_test.rs")
                || rel.split('/').any(|c| c == "tests")
            {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let mut depth: i64 = 0;
            let mut test_depth: Option<i64> = None;
            let mut pending = false;
            let mut n = 0;
            for line in text.lines() {
                let s = line.trim();
                if s.starts_with("#[cfg(test)]") {
                    pending = true;
                }
                if pending && s.starts_with("mod ") && s.ends_with(';')
                    || pending && s.starts_with("pub mod ") && s.ends_with(';')
                {
                    pending = false;
                }
                if pending && line.contains('{') && test_depth.is_none() {
                    test_depth = Some(depth);
                    pending = false;
                }
                if test_depth.is_none() && line.contains("prim::NUM") && !s.starts_with("//") {
                    n += 1;
                }
                depth += line.matches('{').count() as i64 - line.matches('}').count() as i64;
                if let Some(d) = test_depth {
                    if depth <= d {
                        test_depth = None;
                    }
                }
            }
            if n > 0 {
                out.insert(rel, n);
            }
        }
    }
    out
}

/// Rows of the audit table, per file.
fn table_counts(table: &str) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for line in table.lines() {
        let Some(rest) = line.strip_prefix("| `") else {
            continue;
        };
        let loc = rest.split('`').next().unwrap();
        let file = loc.rsplit_once(':').map_or(loc, |(f, _)| f);
        *out.entry(file.to_string()).or_insert(0) += 1;
    }
    out
}

#[test]
fn every_num_seed_has_an_audit_row() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let table = std::fs::read_to_string(crate_dir.join("NUM_SEEDS.md")).unwrap();
    let rows = table_counts(&table);
    let uses = source_counts(&crate_dir.join("src"));
    assert_eq!(
        uses, rows,
        "prim::NUM uses (left) and NUM_SEEDS.md rows (right) differ: audit the new seed, cite its runtime producer, and add its row"
    );
}

/// Every seed row cites a producer (no empty or placeholder citations).
#[test]
fn every_seed_row_cites_a_producer() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let table = std::fs::read_to_string(crate_dir.join("NUM_SEEDS.md")).unwrap();
    for line in table.lines().filter(|l| l.starts_with("| `")) {
        let cols: Vec<&str> = line.split(" | ").collect();
        assert!(cols.len() >= 3, "malformed row: {line}");
        let cit = cols[2].trim_end_matches(" |").trim();
        assert!(cit.len() > 12, "row without a citation: {line}");
    }
}
