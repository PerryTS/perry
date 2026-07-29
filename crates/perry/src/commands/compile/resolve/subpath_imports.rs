//! Node.js `#` subpath imports: package.json `"imports"` field resolution.
//!
//! Implements the spec's PACKAGE_IMPORTS_RESOLVE / PACKAGE_IMPORTS_EXPORTS_RESOLVE
//! semantics (Node `lib/internal/modules/esm/resolve.js`,
//! https://nodejs.org/api/packages.html#imports) for `#`-prefixed specifiers:
//!
//! - The importing file's **package scope** is located by walking up to the
//!   nearest `package.json` that declares an `"imports"` object (stopping at a
//!   `node_modules` boundary). Perry deviates from Node in one forgiving way:
//!   a `package.json` *without* an `imports` field does not end the walk —
//!   nested `package.json` stubs (`{"type":"module"}` markers in `dist/`
//!   folders) are common and treating them as scope roots would only ever
//!   produce resolution failures.
//! - Exact (non-`*`) keys match first; otherwise `*` pattern keys compete and
//!   the **best match** wins — longest prefix up to the `*`, ties broken by
//!   the longer total key (Node's `patternKeyCompare`).
//! - Targets may be strings, arrays (first entry that resolves wins), or
//!   conditional objects. Conditions are matched in the fixed **priority
//!   order** of [`DEFAULT_CONDITIONS`]
//!   (`perry`/`node`/`import`/`module`/`default`/`require`) — the same model
//!   (and the same `node`-above-`default` ranking) as perry's package-`exports`
//!   resolver `resolve_exports`; the two resolvers must agree (a `{ node,
//!   default: browser }` pair must pick the node build for native
//!   compilation). Node itself matches in object-key order, but serde_json
//!   maps are sorted, so declaration order is not observable here; perry
//!   consumes both module flavors, which is why `require` is accepted
//!   alongside `import`.
//! - String targets must start with `./` (resolved inside the package) or be
//!   bare package specifiers (returned as [`SubpathImportOutcome::External`]
//!   for the caller to resolve through node_modules per spec). Targets or
//!   captured wildcards that traverse `..` / `node_modules` — or that
//!   lexically escape the package directory — are rejected with
//!   [`SubpathImportError::InvalidTarget`] / `InvalidSpecifier`, as are the
//!   invalid specifiers `#`, `#/…`, and anything ending in `/`.
//! - TypeScript-friendliness: `./`-targets are probed through
//!   `resolve_with_extensions`, so a mapping like `"#lib/*": "./src/lib/*"`
//!   resolves `#lib/foo` to `src/lib/foo.ts` (Node itself would not; perry
//!   compiles TS directly — same policy as the tsconfig-`paths` fallback).
//!
//! A target that is well-formed but points at a file missing on disk simply
//! does not resolve (array/condition fall-through continues; the overall
//! result is [`SubpathImportOutcome::NotDefined`]) — consistent with
//! `resolve_package_entry`'s pruned-target tolerance.

use std::cmp::Ordering;
use std::fmt;
use std::path::{Path, PathBuf};

use super::{normalize_path_lexically, resolve_with_extensions};

/// Conditions accepted when matching conditional `"imports"` targets, in
/// priority order. Mirrors perry's package-`exports` resolver
/// (`resolve_exports` / `resolve_exports_candidates`) exactly — including
/// ranking `node` above `default`, so a `{ node, default: browser }`
/// conditional pair picks the node build for native compilation.
pub(crate) const DEFAULT_CONDITIONS: &[&str] =
    &["perry", "node", "import", "module", "default", "require"];

/// Successful outcome of resolving a `#` subpath-import specifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SubpathImportOutcome {
    /// Resolved to a file on disk inside the declaring package.
    File(PathBuf),
    /// The winning target is a bare package specifier (spec: internal targets
    /// not starting with `./` resolve through node_modules). The caller
    /// re-enters ordinary package resolution with this specifier.
    External(String),
    /// No governing `package.json` declares an `imports` object, or the
    /// declared map does not define (or does not resolve) this specifier.
    /// Callers may fall through to other resolution strategies (tsconfig
    /// `paths`) or report the import unresolved.
    NotDefined,
}

/// Hard, spec-defined failures. Unlike [`SubpathImportOutcome::NotDefined`]
/// these should be surfaced to the user rather than silently falling through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SubpathImportError {
    /// `#`, `#/…`, or a specifier ending in `/` (Node: Invalid Module
    /// Specifier), or a matched wildcard capture containing `..` /
    /// `node_modules` segments (Node: Invalid Module Specifier via
    /// `throwInvalidSubpath`).
    InvalidSpecifier { specifier: String, reason: String },
    /// A target string that is neither `./`-relative nor a valid bare package
    /// specifier, or one that traverses outside the package directory
    /// (Node: Invalid Package Target).
    InvalidTarget {
        specifier: String,
        target: String,
        package_json: PathBuf,
        reason: String,
    },
}

impl fmt::Display for SubpathImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SubpathImportError::InvalidSpecifier { specifier, reason } => {
                write!(
                    f,
                    "invalid subpath import specifier '{specifier}': {reason}"
                )
            }
            SubpathImportError::InvalidTarget {
                specifier,
                target,
                package_json,
                reason,
            } => write!(
                f,
                "invalid \"imports\" target '{target}' for '{specifier}' in {}: {reason}",
                package_json.display()
            ),
        }
    }
}

impl std::error::Error for SubpathImportError {}

/// Resolve a `#`-prefixed specifier through the importing package's
/// `package.json` `"imports"` map (spec PACKAGE_IMPORTS_RESOLVE).
///
/// `importer_path` is the file containing the import; the package scope is
/// found by walking up from its directory. `conditions` is the active
/// condition set (callers normally pass [`DEFAULT_CONDITIONS`]).
pub(crate) fn resolve_subpath_import(
    specifier: &str,
    importer_path: &Path,
    conditions: &[&str],
) -> Result<SubpathImportOutcome, SubpathImportError> {
    if !specifier.starts_with('#') {
        return Ok(SubpathImportOutcome::NotDefined);
    }
    // Spec: `#` alone and `#/…` are invalid (the empty name / rooted form);
    // a trailing `/` never names a module either.
    if specifier == "#" || specifier.starts_with("#/") || specifier.ends_with('/') {
        return Err(SubpathImportError::InvalidSpecifier {
            specifier: specifier.to_string(),
            reason: "is not a valid internal imports specifier name".to_string(),
        });
    }

    let mut dir = importer_path.parent();
    while let Some(d) = dir {
        let pkg_json_path = d.join("package.json");
        if pkg_json_path.is_file() {
            if let Some(imports) = read_imports_map(&pkg_json_path) {
                // Nearest package.json WITH an `imports` object wins; its
                // verdict is final (no further walking).
                return resolve_imports_field(specifier, d, &pkg_json_path, &imports, conditions);
            }
        }
        // Never cross a node_modules boundary into an outer package's scope.
        if d.file_name().is_some_and(|n| n == "node_modules") {
            break;
        }
        dir = d.parent();
    }
    Ok(SubpathImportOutcome::NotDefined)
}

/// Read the `"imports"` object from a package.json, if present and an object.
fn read_imports_map(pkg_json_path: &Path) -> Option<serde_json::Map<String, serde_json::Value>> {
    let content = std::fs::read_to_string(pkg_json_path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    match json.get("imports") {
        Some(serde_json::Value::Object(map)) => Some(map.clone()),
        _ => None,
    }
}

/// PACKAGE_IMPORTS_EXPORTS_RESOLVE over a parsed `imports` map: exact key
/// first, then `*` pattern keys via best-match.
fn resolve_imports_field(
    specifier: &str,
    package_dir: &Path,
    package_json: &Path,
    imports: &serde_json::Map<String, serde_json::Value>,
    conditions: &[&str],
) -> Result<SubpathImportOutcome, SubpathImportError> {
    // Exact (non-pattern) key.
    if !specifier.contains('*') {
        if let Some(target) = imports.get(specifier) {
            return Ok(resolve_target(
                specifier,
                package_dir,
                package_json,
                target,
                "",
                false,
                conditions,
            )?
            .unwrap_or(SubpathImportOutcome::NotDefined));
        }
    }

    // Wildcard keys: best match = longest prefix before the `*`, ties broken
    // by longer total key (Node's patternKeyCompare). Keys with more than one
    // `*` are ignored, per spec.
    let mut best: Option<(&String, &serde_json::Value, &str)> = None;
    for (key, target) in imports {
        let Some(star) = key.find('*') else { continue };
        if key[star + 1..].contains('*') {
            continue;
        }
        let prefix = &key[..star];
        let trailer = &key[star + 1..];
        if specifier.len() >= key.len()
            && specifier.starts_with(prefix)
            && specifier.ends_with(trailer)
        {
            let better = match &best {
                None => true,
                Some((best_key, _, _)) => pattern_key_compare(best_key, key) == Ordering::Greater,
            };
            if better {
                let captured = &specifier[star..specifier.len() - trailer.len()];
                best = Some((key, target, captured));
            }
        }
    }
    if let Some((key, target, captured)) = best {
        return Ok(resolve_target(
            key,
            package_dir,
            package_json,
            target,
            captured,
            true,
            conditions,
        )?
        .unwrap_or(SubpathImportOutcome::NotDefined));
    }

    Ok(SubpathImportOutcome::NotDefined)
}

/// Node's `patternKeyCompare`: [`Ordering::Less`] means `a` is the better
/// (more specific) pattern key. Longer base (prefix incl. `*`, or whole key
/// when exact) wins; exact keys outrank patterns at equal base length; among
/// equal-base patterns the longer total key wins.
fn pattern_key_compare(a: &str, b: &str) -> Ordering {
    let a_star = a.find('*');
    let b_star = b.find('*');
    let base_a = a_star.map_or(a.len(), |i| i + 1);
    let base_b = b_star.map_or(b.len(), |i| i + 1);
    if base_a != base_b {
        // Longer base sorts first (is better).
        return base_b.cmp(&base_a);
    }
    match (a_star, b_star) {
        (None, _) => Ordering::Greater,
        (_, None) => Ordering::Less,
        _ => b.len().cmp(&a.len()),
    }
}

/// PACKAGE_TARGET_RESOLVE (internal = true): recursively resolve a target
/// value — string, fallback array, conditional object, or null.
///
/// `Ok(None)` means "did not resolve" (missing condition / null target / file
/// absent on disk) and lets array/condition fall-through continue; `Err` is a
/// hard spec violation.
fn resolve_target(
    key: &str,
    package_dir: &Path,
    package_json: &Path,
    target: &serde_json::Value,
    captured: &str,
    pattern: bool,
    conditions: &[&str],
) -> Result<Option<SubpathImportOutcome>, SubpathImportError> {
    match target {
        serde_json::Value::String(s) => {
            resolve_target_string(key, package_dir, package_json, s, captured, pattern)
        }
        // Fallback array: first entry that resolves wins. Invalid entries are
        // skipped unless the whole array yields nothing but errors, in which
        // case the last error propagates (mirrors Node's lastException).
        serde_json::Value::Array(items) => {
            let mut last_err = None;
            let mut saw_non_error = false;
            for item in items {
                match resolve_target(
                    key,
                    package_dir,
                    package_json,
                    item,
                    captured,
                    pattern,
                    conditions,
                ) {
                    Ok(Some(outcome)) => return Ok(Some(outcome)),
                    Ok(None) => saw_non_error = true,
                    Err(e) => last_err = Some(e),
                }
            }
            match last_err {
                Some(e) if !saw_non_error => Err(e),
                _ => Ok(None),
            }
        }
        // Conditional object: try the active conditions in priority order
        // (the same model as `resolve_exports_with_conditions` — see the
        // [`DEFAULT_CONDITIONS`] doc for why key order is not used). A branch
        // that fails to resolve falls through to the next condition.
        serde_json::Value::Object(map) => {
            for cond in conditions {
                if let Some(value) = map.get(*cond) {
                    if let Some(outcome) = resolve_target(
                        key,
                        package_dir,
                        package_json,
                        value,
                        captured,
                        pattern,
                        conditions,
                    )? {
                        return Ok(Some(outcome));
                    }
                }
            }
            Ok(None)
        }
        serde_json::Value::Null => Ok(None),
        _ => Err(SubpathImportError::InvalidTarget {
            specifier: key.to_string(),
            target: target.to_string(),
            package_json: package_json.to_path_buf(),
            reason: "target must be a string, array, conditional object, or null".to_string(),
        }),
    }
}

/// PACKAGE_TARGET_RESOLVE string case (internal = true): `./`-relative
/// targets resolve inside the package (with perry's extension probing); bare
/// package specifiers are handed back for node_modules resolution; anything
/// else is invalid.
fn resolve_target_string(
    key: &str,
    package_dir: &Path,
    package_json: &Path,
    target: &str,
    captured: &str,
    pattern: bool,
) -> Result<Option<SubpathImportOutcome>, SubpathImportError> {
    let invalid_target = |reason: &str| SubpathImportError::InvalidTarget {
        specifier: key.to_string(),
        target: target.to_string(),
        package_json: package_json.to_path_buf(),
        reason: reason.to_string(),
    };

    if !target.starts_with("./") {
        // Spec (internal targets): absolute and `../` targets are invalid;
        // everything else that looks like a package name resolves through
        // node_modules. `node:` builtins are allowed through as externals.
        if target.starts_with('/') || target.starts_with("../") {
            return Err(invalid_target(
                "target must start with \"./\" or be a bare package specifier \
                 (it must not escape the package directory)",
            ));
        }
        let substituted = if pattern {
            target.replace('*', captured)
        } else {
            target.to_string()
        };
        if is_valid_external_target(&substituted) {
            return Ok(Some(SubpathImportOutcome::External(substituted)));
        }
        return Err(invalid_target(
            "target must start with \"./\" or be a bare package specifier",
        ));
    }

    // Reject `.` / `..` / `node_modules` path segments in the target itself…
    if has_invalid_path_segment(&target[2..]) {
        return Err(invalid_target(
            "target must not contain \".\", \"..\", or \"node_modules\" path segments",
        ));
    }
    // …and in the matched wildcard text (the request could smuggle `../`).
    if pattern && !captured.is_empty() && has_invalid_path_segment(captured) {
        return Err(SubpathImportError::InvalidSpecifier {
            specifier: key.replacen('*', captured, 1),
            reason: "matched wildcard text must not contain \".\", \"..\", or \
                     \"node_modules\" path segments"
                .to_string(),
        });
    }

    let substituted = if pattern {
        target.replace('*', captured)
    } else {
        target.to_string()
    };
    let joined = package_dir.join(substituted.trim_start_matches("./"));
    // Belt and braces: even with segment checks above, verify the lexically
    // normalized path stays inside the package directory.
    let normalized = normalize_path_lexically(&joined);
    if !normalized.starts_with(package_dir) {
        return Err(invalid_target(
            "resolved path escapes the package directory",
        ));
    }

    // Perry compiles TS directly: probe `.ts`/`.tsx`/`.js`/… (and directory
    // `index` files) like the relative-import and tsconfig-paths resolvers do.
    match resolve_with_extensions(&normalized) {
        Some(found) => {
            let canonical = found.canonicalize().unwrap_or(found);
            Ok(Some(SubpathImportOutcome::File(canonical)))
        }
        None => Ok(None),
    }
}

/// True when any `/`- or `\`-separated segment is empty, `.`, `..`, or
/// `node_modules` (case-insensitive) — the segments Node's
/// `invalidSegmentRegEx` rejects in targets and matched subpaths.
fn has_invalid_path_segment(path: &str) -> bool {
    path.split(['/', '\\']).any(|segment| {
        segment.is_empty()
            || segment == "."
            || segment == ".."
            || segment.eq_ignore_ascii_case("node_modules")
    })
}

/// Validate a bare-package external target (post `*` substitution): the shape
/// Node's `packageResolve` would accept — non-empty, not relative/rooted/
/// internal, no `\` or `%`, and no URL scheme other than `node:`.
fn is_valid_external_target(spec: &str) -> bool {
    if spec.is_empty() || spec.starts_with('.') || spec.starts_with('/') || spec.starts_with('#') {
        return false;
    }
    if spec.contains('\\') || spec.contains('%') {
        return false;
    }
    if let Some(rest) = spec.strip_prefix("node:") {
        return !rest.is_empty();
    }
    // Any other scheme-looking specifier (`file:`, `data:`, `https:`) is not
    // a package name. A scoped `@scope/name` or plain name has no `:` before
    // any `/`, so this check only fires on genuine scheme prefixes.
    match (spec.find(':'), spec.find('/')) {
        (Some(colon), Some(slash)) if colon < slash => false,
        (Some(_), None) => false,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a fixture package under a tempdir: write `package.json` with the
    /// given `imports` JSON plus each (relative path, contents) source file.
    /// Returns the importer path `<root>/src/main.ts` (created).
    fn fixture(root: &Path, imports_json: &str, files: &[&str]) -> PathBuf {
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(
            root.join("package.json"),
            format!(r##"{{ "name": "fixture", "imports": {imports_json} }}"##),
        )
        .unwrap();
        for rel in files {
            let path = root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "export const x = 1;\n").unwrap();
        }
        let importer = root.join("src/main.ts");
        std::fs::create_dir_all(importer.parent().unwrap()).unwrap();
        std::fs::write(&importer, "// importer\n").unwrap();
        importer
    }

    fn expect_file(outcome: SubpathImportOutcome) -> PathBuf {
        match outcome {
            SubpathImportOutcome::File(p) => p,
            other => panic!("expected File outcome, got {other:?}"),
        }
    }

    #[test]
    fn exact_key_resolves() {
        let dir = tempfile::tempdir().unwrap();
        let importer = fixture(
            dir.path(),
            r##"{ "#config": "./src/config.ts" }"##,
            &["src/config.ts"],
        );
        let resolved =
            expect_file(resolve_subpath_import("#config", &importer, DEFAULT_CONDITIONS).unwrap());
        assert!(resolved.ends_with("src/config.ts"));
    }

    #[test]
    fn wildcard_resolves_ts_source_without_extension_in_specifier() {
        // Perry-specific: `./src/lib/*` + `#lib/foo` must probe `foo.ts`.
        let dir = tempfile::tempdir().unwrap();
        let importer = fixture(
            dir.path(),
            r##"{ "#lib/*": "./src/lib/*" }"##,
            &["src/lib/foo.ts"],
        );
        let resolved =
            expect_file(resolve_subpath_import("#lib/foo", &importer, DEFAULT_CONDITIONS).unwrap());
        assert!(resolved.ends_with("src/lib/foo.ts"));
    }

    #[test]
    fn wildcard_best_match_longest_prefix_wins() {
        let dir = tempfile::tempdir().unwrap();
        let importer = fixture(
            dir.path(),
            r##"{ "#lib/*": "./src/lib/*", "#lib/deep/*": "./src/deep/*" }"##,
            &["src/lib/deep/util.ts", "src/deep/util.ts"],
        );
        let resolved = expect_file(
            resolve_subpath_import("#lib/deep/util", &importer, DEFAULT_CONDITIONS).unwrap(),
        );
        // `#lib/deep/*` (longer non-wildcard prefix) must beat `#lib/*`.
        assert!(resolved.ends_with("src/deep/util.ts"), "{resolved:?}");
    }

    #[test]
    fn exact_key_outranks_wildcard() {
        let dir = tempfile::tempdir().unwrap();
        let importer = fixture(
            dir.path(),
            r##"{ "#lib/special": "./src/special.ts", "#lib/*": "./src/lib/*" }"##,
            &["src/special.ts", "src/lib/special.ts"],
        );
        let resolved = expect_file(
            resolve_subpath_import("#lib/special", &importer, DEFAULT_CONDITIONS).unwrap(),
        );
        assert!(resolved.ends_with("src/special.ts"), "{resolved:?}");
    }

    #[test]
    fn wildcard_with_trailer_substitutes_capture() {
        let dir = tempfile::tempdir().unwrap();
        let importer = fixture(
            dir.path(),
            r##"{ "#internal/*.js": "./src/internal/*.js" }"##,
            &["src/internal/helper.ts"],
        );
        // `*` captures `helper`; the `.js` target probes the `.ts` source.
        let resolved = expect_file(
            resolve_subpath_import("#internal/helper.js", &importer, DEFAULT_CONDITIONS).unwrap(),
        );
        assert!(resolved.ends_with("src/internal/helper.ts"), "{resolved:?}");
    }

    #[test]
    fn conditional_object_prefers_node_over_default() {
        let dir = tempfile::tempdir().unwrap();
        let importer = fixture(
            dir.path(),
            r##"{ "#env": { "node": "./src/env.node.ts", "default": "./src/env.default.ts" } }"##,
            &["src/env.node.ts", "src/env.default.ts"],
        );
        let resolved =
            expect_file(resolve_subpath_import("#env", &importer, DEFAULT_CONDITIONS).unwrap());
        assert!(resolved.ends_with("src/env.node.ts"), "{resolved:?}");
    }

    #[test]
    fn conditional_object_falls_back_to_default() {
        let dir = tempfile::tempdir().unwrap();
        let importer = fixture(
            dir.path(),
            r##"{ "#env": { "browser": "./src/env.browser.ts", "default": "./src/env.default.ts" } }"##,
            &["src/env.browser.ts", "src/env.default.ts"],
        );
        // `browser` is not in the active condition set → `default` wins.
        let resolved =
            expect_file(resolve_subpath_import("#env", &importer, DEFAULT_CONDITIONS).unwrap());
        assert!(resolved.ends_with("src/env.default.ts"), "{resolved:?}");
    }

    #[test]
    fn nested_condition_objects_resolve() {
        let dir = tempfile::tempdir().unwrap();
        let importer = fixture(
            dir.path(),
            r##"{ "#env": { "node": { "import": "./src/env.mjs.ts", "require": "./src/env.cjs.ts" } } }"##,
            &["src/env.mjs.ts", "src/env.cjs.ts"],
        );
        let resolved =
            expect_file(resolve_subpath_import("#env", &importer, DEFAULT_CONDITIONS).unwrap());
        assert!(resolved.ends_with("src/env.mjs.ts"), "{resolved:?}");
    }

    #[test]
    fn condition_branch_missing_on_disk_falls_through() {
        let dir = tempfile::tempdir().unwrap();
        let importer = fixture(
            dir.path(),
            r##"{ "#env": { "node": "./dist/env.js", "default": "./src/env.ts" } }"##,
            &["src/env.ts"], // dist/env.js intentionally absent
        );
        let resolved =
            expect_file(resolve_subpath_import("#env", &importer, DEFAULT_CONDITIONS).unwrap());
        assert!(resolved.ends_with("src/env.ts"), "{resolved:?}");
    }

    #[test]
    fn array_target_first_that_resolves_wins() {
        let dir = tempfile::tempdir().unwrap();
        let importer = fixture(
            dir.path(),
            r##"{ "#dep": ["./missing/dep.ts", "./src/dep.ts"] }"##,
            &["src/dep.ts"],
        );
        let resolved =
            expect_file(resolve_subpath_import("#dep", &importer, DEFAULT_CONDITIONS).unwrap());
        assert!(resolved.ends_with("src/dep.ts"), "{resolved:?}");
    }

    #[test]
    fn array_of_only_invalid_targets_propagates_error() {
        let dir = tempfile::tempdir().unwrap();
        let importer = fixture(dir.path(), r##"{ "#dep": ["../out.ts", "/abs.ts"] }"##, &[]);
        let err = resolve_subpath_import("#dep", &importer, DEFAULT_CONDITIONS).unwrap_err();
        assert!(matches!(err, SubpathImportError::InvalidTarget { .. }));
    }

    #[test]
    fn bare_package_target_returns_external() {
        let dir = tempfile::tempdir().unwrap();
        let importer = fixture(dir.path(), r##"{ "#dep": "some-pkg" }"##, &[]);
        let outcome = resolve_subpath_import("#dep", &importer, DEFAULT_CONDITIONS).unwrap();
        assert_eq!(
            outcome,
            SubpathImportOutcome::External("some-pkg".to_string())
        );
    }

    #[test]
    fn bare_package_wildcard_target_substitutes_capture() {
        let dir = tempfile::tempdir().unwrap();
        let importer = fixture(
            dir.path(),
            r##"{ "#vendored/*": "@scope/vendored/*" }"##,
            &[],
        );
        let outcome =
            resolve_subpath_import("#vendored/util", &importer, DEFAULT_CONDITIONS).unwrap();
        assert_eq!(
            outcome,
            SubpathImportOutcome::External("@scope/vendored/util".to_string())
        );
    }

    #[test]
    fn node_builtin_target_returns_external() {
        let dir = tempfile::tempdir().unwrap();
        let importer = fixture(dir.path(), r##"{ "#fs": "node:fs" }"##, &[]);
        let outcome = resolve_subpath_import("#fs", &importer, DEFAULT_CONDITIONS).unwrap();
        assert_eq!(
            outcome,
            SubpathImportOutcome::External("node:fs".to_string())
        );
    }

    #[test]
    fn parent_dir_target_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let importer = fixture(dir.path(), r##"{ "#escape": "../outside.ts" }"##, &[]);
        let err = resolve_subpath_import("#escape", &importer, DEFAULT_CONDITIONS).unwrap_err();
        assert!(
            matches!(err, SubpathImportError::InvalidTarget { .. }),
            "{err}"
        );
    }

    #[test]
    fn dotdot_segment_inside_target_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let importer = fixture(
            dir.path(),
            r##"{ "#escape": "./src/../../outside.ts" }"##,
            &[],
        );
        let err = resolve_subpath_import("#escape", &importer, DEFAULT_CONDITIONS).unwrap_err();
        assert!(
            matches!(err, SubpathImportError::InvalidTarget { .. }),
            "{err}"
        );
    }

    #[test]
    fn wildcard_capture_cannot_smuggle_dotdot() {
        let dir = tempfile::tempdir().unwrap();
        let importer = fixture(
            dir.path(),
            r##"{ "#lib/*": "./src/lib/*" }"##,
            &["src/lib/foo.ts"],
        );
        let err = resolve_subpath_import("#lib/../../../etc/passwd", &importer, DEFAULT_CONDITIONS)
            .unwrap_err();
        assert!(
            matches!(err, SubpathImportError::InvalidSpecifier { .. }),
            "{err}"
        );
    }

    #[test]
    fn hash_alone_and_hash_slash_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let importer = fixture(dir.path(), r##"{ "#lib/*": "./src/lib/*" }"##, &[]);
        for bad in ["#", "#/", "#/foo", "#lib/"] {
            let err = resolve_subpath_import(bad, &importer, DEFAULT_CONDITIONS).unwrap_err();
            assert!(
                matches!(err, SubpathImportError::InvalidSpecifier { .. }),
                "{bad} should be invalid, got {err}"
            );
        }
    }

    #[test]
    fn nearest_package_json_with_imports_wins() {
        // Root maps #util to root/src/util.ts; a nested package maps it to
        // its own file. An importer inside the nested package must get the
        // nested mapping.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fixture(root, r##"{ "#util": "./src/util.ts" }"##, &["src/util.ts"]);
        let nested = root.join("packages/inner");
        let nested_importer = fixture(
            &nested,
            r##"{ "#util": "./lib/util.ts" }"##,
            &["lib/util.ts"],
        );
        let resolved = expect_file(
            resolve_subpath_import("#util", &nested_importer, DEFAULT_CONDITIONS).unwrap(),
        );
        assert!(
            resolved.ends_with("packages/inner/lib/util.ts"),
            "{resolved:?}"
        );
    }

    #[test]
    fn package_json_without_imports_does_not_end_the_walk() {
        // A nested `{"type":"module"}` stub must not shadow the root imports.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fixture(root, r##"{ "#util": "./src/util.ts" }"##, &["src/util.ts"]);
        let sub = root.join("src/area");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("package.json"), r##"{ "type": "module" }"##).unwrap();
        let importer = sub.join("main.ts");
        std::fs::write(&importer, "// importer\n").unwrap();
        let resolved =
            expect_file(resolve_subpath_import("#util", &importer, DEFAULT_CONDITIONS).unwrap());
        assert!(resolved.ends_with("src/util.ts"), "{resolved:?}");
    }

    #[test]
    fn no_imports_field_anywhere_is_a_noop() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("package.json"), r##"{ "name": "no-imports" }"##).unwrap();
        let importer = root.join("src/main.ts");
        std::fs::write(&importer, "// importer\n").unwrap();
        assert_eq!(
            resolve_subpath_import("#lib/foo", &importer, DEFAULT_CONDITIONS).unwrap(),
            SubpathImportOutcome::NotDefined
        );
    }

    #[test]
    fn undefined_specifier_is_not_defined() {
        let dir = tempfile::tempdir().unwrap();
        let importer = fixture(
            dir.path(),
            r##"{ "#lib/*": "./src/lib/*" }"##,
            &["src/lib/foo.ts"],
        );
        assert_eq!(
            resolve_subpath_import("#other", &importer, DEFAULT_CONDITIONS).unwrap(),
            SubpathImportOutcome::NotDefined
        );
    }

    #[test]
    fn null_target_is_not_defined() {
        let dir = tempfile::tempdir().unwrap();
        let importer = fixture(dir.path(), r##"{ "#blocked": null }"##, &[]);
        assert_eq!(
            resolve_subpath_import("#blocked", &importer, DEFAULT_CONDITIONS).unwrap(),
            SubpathImportOutcome::NotDefined
        );
    }

    #[test]
    fn node_modules_boundary_stops_the_scope_walk() {
        // An importer inside node_modules/dep (whose package.json has no
        // imports) must NOT resolve through the app package's imports map.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fixture(root, r##"{ "#util": "./src/util.ts" }"##, &["src/util.ts"]);
        let dep = root.join("node_modules/dep");
        std::fs::create_dir_all(&dep).unwrap();
        std::fs::write(dep.join("package.json"), r##"{ "name": "dep" }"##).unwrap();
        let importer = dep.join("index.ts");
        std::fs::write(&importer, "// importer\n").unwrap();
        assert_eq!(
            resolve_subpath_import("#util", &importer, DEFAULT_CONDITIONS).unwrap(),
            SubpathImportOutcome::NotDefined
        );
    }

    #[test]
    fn pattern_key_compare_matches_node_semantics() {
        // Longer base wins regardless of total length.
        assert_eq!(pattern_key_compare("#a/*", "#ab/*"), Ordering::Greater);
        assert_eq!(pattern_key_compare("#ab/*", "#a/*"), Ordering::Less);
        // Equal base: longer total key (more specific trailer) wins.
        assert_eq!(pattern_key_compare("#a/*", "#a/*.js"), Ordering::Greater);
        assert_eq!(pattern_key_compare("#a/*.js", "#a/*"), Ordering::Less);
        assert_eq!(pattern_key_compare("#a/*", "#a/*"), Ordering::Equal);
    }
}
