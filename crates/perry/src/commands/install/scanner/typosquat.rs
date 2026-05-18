//! P0 rule #3: typosquat detection.
//!
//! v1 of this rule flagged any name within Levenshtein distance 2 of a
//! popular package. That fired on legitimate widely-used packages whose
//! names happen to sit near a popular target (`gaxios` near `axios`,
//! `prompts` near `prompt`, `enquirer` near `inquirer`, `bn.js` near
//! `big.js`, `safer-buffer` near `safe-buffer`, ...). Each false-block
//! erodes developer trust in the scanner, so v2 trades a bit of recall
//! for substantially better precision.
//!
//! v2 algorithm:
//! 1. If the candidate name itself appears in our popular-packages list,
//!    suppress (a package isn't a squat of itself, and "well-known"
//!    packages by definition aren't squats of others).
//! 2. Otherwise, find the closest popular target within Levenshtein 2
//!    and length-diff ≤ 1. If no target matches, no finding.
//! 3. If a target matches, score the candidate's local-metadata
//!    completeness: presence of `repository`, `license` field, a
//!    `LICENSE` file, and a `README`. Real squats are almost always
//!    sparse on at least one of these; legitimate "looks-like-a-squat"
//!    packages ship full metadata.
//!    - All 4 signals present → suppress (treat as legitimate).
//!    - 2 or 3 present → P1 (warn, don't block).
//!    - ≤ 1 present → P0 (block).
//!
//! The eval-and-decode rule (`patterns.rs`), the lifecycle-script gate,
//! and the exfil-URL rules still fire on the actual *payload*, so a
//! suppressed name here is not a free pass for malware.
//!
//! Scoped names (`@scope/pkg`) are not scanned — they require a scoped
//! typosquat list which isn't built yet. Filed as a follow-up.
//!
//! Short names (< 5 chars on either side) are also skipped — the
//! address space is small enough that a 2-edit distance between two
//! short names is almost always coincidence.

use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::sync::OnceLock;

use super::report::{Finding, Severity};
use super::ScannedPackage;

const TOP_PACKAGES_TXT: &str = include_str!("data/top_packages.txt");

fn top_packages() -> &'static BTreeSet<&'static str> {
    static SET: OnceLock<BTreeSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| {
        TOP_PACKAGES_TXT
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect()
    })
}

pub fn check(pkg: &ScannedPackage) -> Vec<Finding> {
    let name = pkg.name.as_str();
    if name.starts_with('@') {
        return Vec::new();
    }
    let top = top_packages();
    if top.contains(name) {
        return Vec::new();
    }
    if name.len() < 5 {
        return Vec::new();
    }

    let (target, _dist) = match nearest_popular(name, top) {
        Some(t) => t,
        None => return Vec::new(),
    };

    let comp = local_completeness(pkg);
    let severity = match comp.score() {
        4 => return Vec::new(),
        2 | 3 => Severity::P1,
        _ => Severity::P0,
    };

    let message = match severity {
        Severity::P0 => format!(
            "name resembles popular package \"{target}\"\n\
             If this is the package you intended, re-run with --allow-risky {name}.\n\
             Otherwise verify the spelling against https://www.npmjs.com/package/{name}"
        ),
        Severity::P1 => format!(
            "name resembles popular package \"{target}\"\n\
             Verify it is the package you intended: https://www.npmjs.com/package/{name}"
        ),
    };

    vec![Finding {
        package: format!("{}@{}", pkg.name, pkg.version),
        package_path: pkg.dir.display().to_string(),
        severity,
        rule: "typosquat-close-to-popular".to_string(),
        message,
        location: None,
        overridden: false,
    }]
}

/// Find the popular target the candidate is closest to, subject to the
/// distance / length-diff thresholds. Returns `None` if nothing matches.
fn nearest_popular(name: &str, top: &BTreeSet<&'static str>) -> Option<(&'static str, usize)> {
    let mut best: Option<(&'static str, usize)> = None;
    for &target in top.iter() {
        if target.len() < 5 {
            continue;
        }
        let len_diff = (target.len() as isize - name.len() as isize).abs();
        if len_diff > 1 {
            continue;
        }
        let d = levenshtein(name, target);
        if d == 0 || d > 2 {
            continue;
        }
        match best {
            None => best = Some((target, d)),
            Some((_, prev)) if d < prev => best = Some((target, d)),
            _ => {}
        }
    }
    best
}

/// Local-metadata signals indicating a legitimately-maintained package.
/// We don't go to the registry — every field here is observable from the
/// package's manifest + its on-disk files.
struct Completeness {
    has_repository: bool,
    has_license_field: bool,
    has_license_file: bool,
    has_readme: bool,
}

impl Completeness {
    fn score(&self) -> u8 {
        self.has_repository as u8
            + self.has_license_field as u8
            + self.has_license_file as u8
            + self.has_readme as u8
    }
}

fn local_completeness(pkg: &ScannedPackage) -> Completeness {
    Completeness {
        has_repository: has_repository_field(&pkg.manifest),
        has_license_field: has_nonempty_string_field(&pkg.manifest, "license"),
        has_license_file: has_file_with_prefix(&pkg.dir, &["LICENSE", "LICENCE", "license"]),
        has_readme: has_file_with_prefix(&pkg.dir, &["README", "readme", "Readme"]),
    }
}

fn has_repository_field(manifest: &Value) -> bool {
    match manifest.get("repository") {
        Some(Value::String(s)) => !s.trim().is_empty(),
        Some(Value::Object(o)) => o
            .get("url")
            .and_then(|v| v.as_str())
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false),
        _ => false,
    }
}

fn has_nonempty_string_field(manifest: &Value, key: &str) -> bool {
    matches!(manifest.get(key), Some(Value::String(s)) if !s.trim().is_empty())
}

fn has_file_with_prefix(dir: &Path, prefixes: &[&str]) -> bool {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return false,
    };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        for prefix in prefixes {
            if name.starts_with(prefix) {
                if let Ok(meta) = entry.metadata() {
                    if meta.is_file() {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Classic two-row Levenshtein implementation. O(n·m) time, O(min(n,m))
/// space. Plenty fast for package names (single-digit char counts).
fn levenshtein(a: &str, b: &str) -> usize {
    if a == b {
        return 0;
    }
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (a, b) = if a.len() < b.len() { (b, a) } else { (a, b) };
    let n = a.len();
    let m = b.len();
    if m == 0 {
        return n;
    }
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut curr = vec![0usize; m + 1];
    for i in 1..=n {
        curr[0] = i;
        for j in 1..=m {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (curr[j - 1] + 1).min(prev[j] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[m]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn pkg_bare(name: &str) -> ScannedPackage {
        ScannedPackage {
            name: name.into(),
            version: "1.0.0".into(),
            dir: PathBuf::from("/nonexistent"),
            manifest: json!({"name": name, "version": "1.0.0"}),
        }
    }

    /// Build a package on disk with a chosen subset of legitimacy signals.
    fn pkg_with_metadata(
        td: &TempDir,
        name: &str,
        with_repo: bool,
        with_license_field: bool,
        with_license_file: bool,
        with_readme: bool,
    ) -> ScannedPackage {
        let dir = td.path().join(name.replace('/', "__"));
        fs::create_dir_all(&dir).unwrap();
        let mut manifest = json!({"name": name, "version": "1.0.0"});
        if with_repo {
            manifest["repository"] = json!({"type": "git", "url": "git+https://github.com/x/y.git"});
        }
        if with_license_field {
            manifest["license"] = json!("MIT");
        }
        if with_license_file {
            fs::write(dir.join("LICENSE"), "MIT license text").unwrap();
        }
        if with_readme {
            fs::write(dir.join("README.md"), "# package").unwrap();
        }
        fs::write(dir.join("package.json"), manifest.to_string()).unwrap();
        ScannedPackage {
            name: name.into(),
            version: "1.0.0".into(),
            dir,
            manifest,
        }
    }

    #[test]
    fn levenshtein_basics() {
        assert_eq!(levenshtein("kitten", "kitten"), 0);
        assert_eq!(levenshtein("kitten", "sitten"), 1);
        assert_eq!(levenshtein("kitten", "sittin"), 2);
        assert_eq!(levenshtein("kitten", "sitting"), 3);
    }

    #[test]
    fn flags_bare_squat_no_metadata() {
        // No repo, no license, no LICENSE file, no README — the classic
        // empty squat shape. Should P0-block.
        let f = check(&pkg_bare("expres"));
        assert_eq!(f.len(), 1);
        assert!(matches!(f[0].severity, Severity::P0));
        assert!(f[0].message.contains("express"));
        assert!(f[0].message.contains("--allow-risky expres"));
        assert!(f[0].message.contains("npmjs.com/package/expres"));
    }

    #[test]
    fn suppresses_squat_shaped_but_fully_legitimate() {
        // "gaxios" looks like an "axios" squat (distance 1) but ships
        // full metadata. Must not be flagged — Google's HTTP client.
        let td = TempDir::new().unwrap();
        let p = pkg_with_metadata(&td, "gaxios", true, true, true, true);
        assert!(
            check(&p).is_empty(),
            "fully legitimate package with squat-shaped name should be suppressed"
        );
    }

    #[test]
    fn warns_partially_legitimate_no_block() {
        // Repo + license field but no LICENSE file or README — borderline.
        // Should P1-warn rather than P0-block, so install still proceeds.
        let td = TempDir::new().unwrap();
        let p = pkg_with_metadata(&td, "expres", true, true, false, false);
        let f = check(&p);
        assert_eq!(f.len(), 1);
        assert!(matches!(f[0].severity, Severity::P1));
    }

    #[test]
    fn blocks_when_only_one_signal_present() {
        // Just a license field, nothing else — looks empty enough to block.
        let td = TempDir::new().unwrap();
        let p = pkg_with_metadata(&td, "expres", false, true, false, false);
        let f = check(&p);
        assert_eq!(f.len(), 1);
        assert!(matches!(f[0].severity, Severity::P0));
    }

    #[test]
    fn self_skips_when_in_top_list() {
        // A package whose own name is in the popular list is never a squat.
        assert!(check(&pkg_bare("react")).is_empty());
        assert!(check(&pkg_bare("express")).is_empty());
        assert!(check(&pkg_bare("lodash")).is_empty());
    }

    #[test]
    fn does_not_flag_well_separated_names() {
        // Distance ≥ 3 from every popular name → nothing.
        assert!(check(&pkg_bare("my-perfectly-unique-thing")).is_empty());
    }

    #[test]
    fn does_not_flag_scoped_packages() {
        assert!(check(&pkg_bare("@scope/whatever")).is_empty());
    }

    #[test]
    fn does_not_flag_short_names() {
        assert!(check(&pkg_bare("bl")).is_empty());
        assert!(check(&pkg_bare("pump")).is_empty());
        assert!(check(&pkg_bare("once")).is_empty());
        assert!(check(&pkg_bare("glob")).is_empty());
    }

    /// Regression suite: every entry here is a widely-used real package
    /// that v1 false-flagged. After v2 (full metadata + presence in
    /// top_packages.txt), none of these should fire. If you have to
    /// remove an entry from this list, you're regressing a known fix.
    #[test]
    fn known_false_positives_no_longer_fire() {
        let td = TempDir::new().unwrap();
        for name in &[
            "gaxios",
            "prompts",
            "enquirer",
            "bn.js",
            "retry",
            "fp-ts",
            "expect",
            "reusify",
            "crypt",
            "aes-js",
            "safer-buffer",
            "call-bound",
            "is-typedarray",
            "scrypt-js",
        ] {
            let p = pkg_with_metadata(&td, name, true, true, true, true);
            let f = check(&p);
            assert!(
                f.is_empty(),
                "legitimate package {name} flagged as typosquat: {f:?}"
            );
        }
    }

    #[test]
    fn flags_real_react_native_typosquat_shape() {
        // No metadata → block. "reactnative" (no hyphen) is the classic
        // squat shape against react-native.
        let f = check(&pkg_bare("reactnative"));
        assert_eq!(f.len(), 1);
        assert!(matches!(f[0].severity, Severity::P0));
        assert!(f[0].message.contains("react-native"));
    }

    #[test]
    fn picks_closest_match() {
        let f = check(&pkg_bare("axiom"));
        if !f.is_empty() {
            assert!(f[0].message.contains("axios"));
        }
    }
}
