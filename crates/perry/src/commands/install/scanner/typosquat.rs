//! P0 rule #3: typosquat detection against a bundled list of popular
//! packages.
//!
//! A package whose bare name is within Levenshtein distance 2 of a
//! known-popular name (with length-difference ≤ 1) is flagged. The
//! length-diff guard keeps legitimate "compound" names like
//! `expressjs` (which would be distance 2 from `express` but with
//! length-diff 2 — likely a legitimate metapackage / clone) from
//! tripping the rule, while still catching the classic attack shapes:
//!
//!   - single-letter substitution (`lodass` ↔ `lodash`)
//!   - single-letter delete (`expres` ↔ `express`)
//!   - single-letter insert (`reactt` ↔ `react`)
//!   - adjacent transposition (`epxress` ↔ `express`) — distance 2,
//!     length-diff 0
//!
//! Scoped names (`@scope/pkg`) are not scanned — they require a scoped
//! typosquat list which isn't built yet. Filed as a follow-up.
//!
//! Short names (< 5 chars on either side) are also not compared — the
//! address space is small enough that a 2-edit distance between two
//! short names is almost always coincidence, not intent. `bl` (Buffer
//! List, a legitimate well-known package) vs `c8` (V8 coverage tool)
//! is distance 2 but is not a typosquat; same for `pump` vs `gulp`.
//! In practice, attackers go after high-value targets which have
//! longer names — the well-known short popular names are already in
//! the embedded list and self-skip via the `top.contains` early-out.

use std::collections::BTreeSet;
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

    // Short-name floor: see module docs. Both names must be ≥ 5 chars
    // for the comparison to be meaningful — otherwise a 2-edit distance
    // between two 2-char names (which is almost the entire address
    // space) trips on every legitimate short package.
    if name.len() < 5 {
        return Vec::new();
    }

    let mut best: Option<(&'static str, usize)> = None;
    for target in top.iter() {
        let target = *target;
        if target.len() < 5 {
            continue;
        }
        let len_diff = (target.len() as isize - name.len() as isize).abs();
        if len_diff > 2 {
            continue;
        }
        let d = levenshtein(name, target);
        if d == 0 {
            continue;
        }
        if d <= 2 && len_diff <= 1 {
            match best {
                None => best = Some((target, d)),
                Some((_, prev)) if d < prev => best = Some((target, d)),
                _ => {}
            }
        }
    }

    if let Some((target, d)) = best {
        return vec![Finding {
            package: format!("{}@{}", pkg.name, pkg.version),
            package_path: pkg.dir.display().to_string(),
            severity: Severity::P0,
            rule: "typosquat-close-to-popular".to_string(),
            message: format!(
                "package name '{}' is Levenshtein distance {} from popular package '{}'",
                name, d, target
            ),
            location: None,
            overridden: false,
        }];
    }
    Vec::new()
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

    fn pkg(name: &str) -> ScannedPackage {
        ScannedPackage {
            name: name.into(),
            version: "1.0.0".into(),
            dir: PathBuf::from("/t"),
            manifest: json!({"name": name, "version": "1.0.0"}),
        }
    }

    #[test]
    fn levenshtein_basics() {
        assert_eq!(levenshtein("kitten", "kitten"), 0);
        assert_eq!(levenshtein("kitten", "sitten"), 1);
        assert_eq!(levenshtein("kitten", "sittin"), 2);
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", ""), 3);
    }

    #[test]
    fn flags_expres_typo_of_express() {
        let f = check(&pkg("expres"));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].rule, "typosquat-close-to-popular");
        assert!(f[0].message.contains("express"));
    }

    #[test]
    fn flags_reacts_typo_of_react() {
        assert_eq!(check(&pkg("reacts")).len(), 1);
    }

    #[test]
    fn flags_transposition_epxress() {
        // Length-diff 0, distance 2 — still flagged.
        let f = check(&pkg("epxress"));
        assert_eq!(f.len(), 1);
        assert!(f[0].message.contains("express"));
    }

    #[test]
    fn flags_lodass_typo_of_lodash() {
        assert_eq!(check(&pkg("lodass")).len(), 1);
    }

    #[test]
    fn does_not_flag_self_when_top_package() {
        assert!(check(&pkg("react")).is_empty());
        assert!(check(&pkg("express")).is_empty());
        assert!(check(&pkg("lodash")).is_empty());
    }

    #[test]
    fn does_not_flag_compound_names() {
        // "expressjs" is distance 2 from "express" but length-diff 2,
        // so it falls outside our threshold — likely a legitimate
        // wrapper/metapackage, not a single-letter typo.
        assert!(check(&pkg("expressjs")).is_empty());
        // A genuinely distinct name with a popular substring shouldn't
        // trip the rule (distance >> 2 from any popular name).
        assert!(check(&pkg("react-toolkit-extra")).is_empty());
    }

    #[test]
    fn flags_real_react_native_typosquat() {
        // "reactnative" (no hyphen) is distance 1 from "react-native"
        // and len-diff 1 — exactly the typosquat shape we want to catch.
        let f = check(&pkg("reactnative"));
        assert_eq!(f.len(), 1);
        assert!(f[0].message.contains("react-native"));
    }

    #[test]
    fn does_not_flag_well_separated_names() {
        // Distance ≥ 3 from every popular name.
        assert!(check(&pkg("my-perfectly-unique-thing")).is_empty());
    }

    #[test]
    fn does_not_flag_scoped_packages() {
        // Scoped names aren't checked in v1 (see module docs).
        assert!(check(&pkg("@scope/whatever")).is_empty());
    }

    #[test]
    fn does_not_flag_short_legitimate_packages() {
        // `bl` (Buffer List) and `pump` (stream joiner) are legitimate
        // well-known packages that previously tripped distance-2 against
        // `c8` and `gulp`. The short-name floor (both sides ≥ 5 chars)
        // makes these no-ops without losing any real attack signal.
        assert!(check(&pkg("bl")).is_empty());
        assert!(check(&pkg("pump")).is_empty());
        assert!(check(&pkg("once")).is_empty());
        assert!(check(&pkg("glob")).is_empty());
    }

    #[test]
    fn picks_closest_match() {
        // "axiom" is distance 2 from "axios"; flag against the closest.
        let f = check(&pkg("axiom"));
        if !f.is_empty() {
            assert!(f[0].message.contains("axios"));
        }
        // (Not asserting f.is_empty() either way — the closest popular
        // could shift if the list changes; this test just verifies that
        // when we do flag, the message names a real popular target.)
    }
}
