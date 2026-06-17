//! tsconfig `compilerOptions.paths` / `baseUrl` import resolution (#5214).
//!
//! perry's resolver honors `perry.packageAliases` but historically ignored the
//! TypeScript `paths`/`baseUrl` aliasing that real apps lean on heavily
//! (opencode's 743 `@/...` imports, openclaw's 2156 `@openclaw/*`). Without
//! this every such specifier fell through to "Could not resolve import".
//!
//! This module adds a purely **additive fallback**: it is consulted only when
//! a specifier resolves as neither a relative path nor a node package — i.e.
//! exactly the case that would otherwise be unresolved. With no governing
//! tsconfig, or a tsconfig that declares neither `paths` nor `baseUrl`, the
//! fallback is a no-op and existing behavior is unchanged.
//!
//! ## Matching semantics (mirrors the TypeScript compiler)
//! - A `paths` key may contain a single `*`. The wildcard captures the
//!   remainder of the specifier.
//! - Exact (non-wildcard) keys must match the specifier exactly.
//! - When several patterns match, the one with the **longest non-wildcard
//!   prefix** wins (TS's "best match" rule). Exact matches outrank wildcard
//!   matches.
//! - Each winning key maps to an array of target templates; the captured `*`
//!   text is substituted into every template's `*`, and targets are tried in
//!   array order. First on-disk hit wins.
//! - `baseUrl`/`paths` are resolved relative to the directory of the tsconfig
//!   that *declares* them — which, through `extends`, may differ from the
//!   nearest tsconfig.
//! - If `baseUrl` is set but no `paths` match, the bare specifier is tried
//!   relative to `baseUrl` (TS "classic" baseUrl resolution).
//!
//! ## `extends`
//! `extends` is followed for the common cases: a relative path (with or
//! without `.json`) and a bare package reference resolved through
//! `node_modules`. Base and derived `compilerOptions` are merged with the
//! derived config winning; crucially `baseUrl`/`paths` remember the directory
//! of the config that declared them so substitution resolves correctly.
//!
//! Parsed/merged configs are cached per directory in a process-global map,
//! consistent with how the resolver's other helpers are stateless free
//! functions (the higher-level `cached_resolve_import` memoizes the final
//! result on the `CompilationContext`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use super::{resolve_package_entry, resolve_with_extensions};

/// A `compilerOptions.paths` entry plus the directory its targets resolve
/// against (the dir of the tsconfig that declared `paths`).
#[derive(Clone, Debug)]
struct PathsConfig {
    /// Directory the `paths` targets resolve against. Equal to the declaring
    /// tsconfig's dir joined with `baseUrl` when present, else just the
    /// declaring tsconfig's dir (TS resolves `paths` relative to `baseUrl`,
    /// defaulting to the config dir).
    base_dir: PathBuf,
    /// `paths` map: pattern -> list of target templates.
    paths: HashMap<String, Vec<String>>,
}

/// Resolved tsconfig data relevant to module resolution.
#[derive(Clone, Debug, Default)]
struct TsConfig {
    /// `paths` declaration with its own base dir, if any.
    paths: Option<PathsConfig>,
    /// `baseUrl` resolved to an absolute directory, if set.
    base_url: Option<PathBuf>,
}

impl TsConfig {
    fn is_empty(&self) -> bool {
        self.paths.is_none() && self.base_url.is_none()
    }
}

/// Process-global cache: tsconfig.json path -> merged config (or None if it
/// had no relevant fields / failed to parse). Resolution is hot (per import),
/// so this avoids re-reading + re-parsing the same configs.
fn config_cache() -> &'static Mutex<HashMap<PathBuf, Option<TsConfig>>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Option<TsConfig>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Cache: directory -> nearest governing tsconfig path (walking up). `None`
/// means no tsconfig governs that directory.
fn nearest_cache() -> &'static Mutex<HashMap<PathBuf, Option<PathBuf>>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Option<PathBuf>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Try to resolve `import_source` (a bare, non-relative, non-package
/// specifier) through the governing tsconfig of `importer_path`.
///
/// Returns the canonicalized resolved file on success. This is the only
/// public entry point; it is called as a last-resort fallback from
/// `resolve_import` after relative + package resolution fail.
pub(crate) fn resolve_tsconfig_paths(import_source: &str, importer_path: &Path) -> Option<PathBuf> {
    let importer_dir = importer_path.parent()?;
    let tsconfig_path = nearest_tsconfig(importer_dir)?;
    let config = load_merged_config(&tsconfig_path)?;
    resolve_with_config(import_source, &config)
}

/// Resolve a specifier against an already-merged config (split out for unit
/// testing the matching semantics without touching the cache/`extends`
/// machinery).
fn resolve_with_config(import_source: &str, config: &TsConfig) -> Option<PathBuf> {
    // 1. paths patterns (best-match wins).
    if let Some(paths_cfg) = &config.paths {
        if let Some(candidates) = best_match_targets(import_source, &paths_cfg.paths) {
            for target in candidates {
                if let Some(resolved) = resolve_target(&paths_cfg.base_dir, &target) {
                    return Some(resolved);
                }
            }
        }
    }
    // 2. classic baseUrl resolution: bare specifier relative to baseUrl.
    if let Some(base_url) = &config.base_url {
        if let Some(resolved) = resolve_target(base_url, import_source) {
            return Some(resolved);
        }
    }
    None
}

/// Resolve a single substituted target (which may name a file or a directory)
/// against `base_dir`, reusing the existing file/package resolution helpers.
/// Returns a canonicalized path on success.
fn resolve_target(base_dir: &Path, target: &str) -> Option<PathBuf> {
    let candidate = base_dir.join(target);
    // File (with extension probing) first — the common case for `@/foo`.
    if let Some(resolved) = resolve_with_extensions(&candidate) {
        return resolved.canonicalize().ok().or(Some(resolved));
    }
    // Directory target: resolve via package.json entry / index files.
    if candidate.is_dir() {
        if let Some(resolved) = resolve_package_entry(&candidate, None) {
            return resolved.canonicalize().ok().or(Some(resolved));
        }
    }
    None
}

/// Find the best-matching `paths` key for `specifier` and return its target
/// templates with the wildcard capture already substituted.
///
/// Exact (non-wildcard) keys take precedence over wildcard keys; among
/// wildcard keys the longest non-wildcard prefix wins (TS "best match").
fn best_match_targets(
    specifier: &str,
    paths: &HashMap<String, Vec<String>>,
) -> Option<Vec<String>> {
    // Exact match first.
    if let Some(targets) = paths.get(specifier) {
        if !targets.iter().any(|t| t.contains('*')) {
            return Some(targets.clone());
        }
    }

    // Wildcard match: longest non-wildcard prefix wins.
    let mut best: Option<(usize, String, Vec<String>)> = None;
    for (key, targets) in paths {
        let Some(star) = key.find('*') else { continue };
        let prefix = &key[..star];
        let suffix = &key[star + 1..];
        if specifier.len() < prefix.len() + suffix.len() {
            continue;
        }
        if !specifier.starts_with(prefix) || !specifier.ends_with(suffix) {
            continue;
        }
        let captured = &specifier[prefix.len()..specifier.len() - suffix.len()];
        let prefix_len = prefix.len();
        let better = match &best {
            None => true,
            Some((best_len, _, _)) => prefix_len > *best_len,
        };
        if better {
            let substituted = targets
                .iter()
                .map(|t| t.replace('*', captured))
                .collect::<Vec<_>>();
            best = Some((prefix_len, key.clone(), substituted));
        }
    }
    best.map(|(_, _, targets)| targets)
}

/// Locate the nearest `tsconfig.json` walking up from `dir`, memoized per
/// directory.
fn nearest_tsconfig(dir: &Path) -> Option<PathBuf> {
    let key = dir.to_path_buf();
    if let Some(cached) = nearest_cache().lock().unwrap().get(&key) {
        return cached.clone();
    }
    let mut current = Some(dir);
    let mut found = None;
    while let Some(d) = current {
        let candidate = d.join("tsconfig.json");
        if candidate.is_file() {
            found = Some(candidate);
            break;
        }
        current = d.parent();
    }
    nearest_cache().lock().unwrap().insert(key, found.clone());
    found
}

/// Load + merge a tsconfig (following `extends`), memoized per config path.
fn load_merged_config(tsconfig_path: &Path) -> Option<TsConfig> {
    if let Some(cached) = config_cache().lock().unwrap().get(tsconfig_path) {
        return cached.clone();
    }
    // Guard against `extends` cycles.
    let mut seen = Vec::new();
    let merged = build_merged_config(tsconfig_path, &mut seen);
    let stored = merged.filter(|c| !c.is_empty());
    config_cache()
        .lock()
        .unwrap()
        .insert(tsconfig_path.to_path_buf(), stored.clone());
    stored
}

/// Recursively read `tsconfig_path` and its `extends` chain, merging
/// `compilerOptions.baseUrl`/`paths`. Derived config wins; each
/// `baseUrl`/`paths` carries the dir of the config that declared it.
fn build_merged_config(tsconfig_path: &Path, seen: &mut Vec<PathBuf>) -> Option<TsConfig> {
    let canonical = tsconfig_path
        .canonicalize()
        .unwrap_or_else(|_| tsconfig_path.to_path_buf());
    if seen.contains(&canonical) {
        return None; // cycle
    }
    seen.push(canonical);

    let content = std::fs::read_to_string(tsconfig_path).ok()?;
    let json: serde_json::Value = parse_jsonc(&content)?;
    let config_dir = tsconfig_path.parent()?.to_path_buf();

    // Start from the base config (if `extends`), then overlay this config.
    let mut merged = match json.get("extends").and_then(|v| v.as_str()) {
        Some(extends_ref) => resolve_extends(extends_ref, &config_dir)
            .and_then(|base_path| build_merged_config(&base_path, seen))
            .unwrap_or_default(),
        None => TsConfig::default(),
    };

    let compiler_options = json.get("compilerOptions");

    // baseUrl declared here resolves relative to THIS config's dir.
    if let Some(base_url) = compiler_options
        .and_then(|c| c.get("baseUrl"))
        .and_then(|v| v.as_str())
    {
        merged.base_url = Some(config_dir.join(base_url));
    }

    // paths declared here resolve relative to baseUrl (if set in this same
    // config) else this config's dir — matching TS, where `paths` is anchored
    // at `baseUrl`.
    if let Some(paths_val) = compiler_options
        .and_then(|c| c.get("paths"))
        .and_then(|v| v.as_object())
    {
        let mut paths = HashMap::new();
        for (key, val) in paths_val {
            if let Some(arr) = val.as_array() {
                let targets: Vec<String> = arr
                    .iter()
                    .filter_map(|t| t.as_str().map(|s| s.to_string()))
                    .collect();
                if !targets.is_empty() {
                    paths.insert(key.clone(), targets);
                }
            }
        }
        if !paths.is_empty() {
            // Anchor on baseUrl from THIS config if present, else config dir.
            let base_dir = compiler_options
                .and_then(|c| c.get("baseUrl"))
                .and_then(|v| v.as_str())
                .map(|b| config_dir.join(b))
                .unwrap_or_else(|| config_dir.clone());
            merged.paths = Some(PathsConfig { base_dir, paths });
        }
    }

    seen.pop();
    Some(merged)
}

/// Resolve an `extends` reference to a tsconfig path. Handles:
/// - relative paths (`./base`, `../tsconfig.base.json`), with or without
///   `.json`,
/// - bare package references resolved through `node_modules`
///   (`@tsconfig/node20`, `some-pkg/tsconfig.json`).
fn resolve_extends(extends_ref: &str, config_dir: &Path) -> Option<PathBuf> {
    // Relative / absolute path reference.
    if extends_ref.starts_with('.') || extends_ref.starts_with('/') {
        let direct = config_dir.join(extends_ref);
        if direct.is_file() {
            return Some(direct);
        }
        // Append `.json` if missing.
        if direct.extension().is_none() {
            let with_json = config_dir.join(format!("{}.json", extends_ref));
            if with_json.is_file() {
                return Some(with_json);
            }
        }
        // A directory reference points at its `tsconfig.json`.
        if direct.is_dir() {
            let nested = direct.join("tsconfig.json");
            if nested.is_file() {
                return Some(nested);
            }
        }
        return None;
    }

    // Bare package reference: walk up node_modules from config_dir.
    resolve_extends_package(extends_ref, config_dir)
}

/// Resolve a bare-package `extends` (e.g. `@tsconfig/node20` or
/// `pkg/tsconfig.custom.json`) through node_modules.
fn resolve_extends_package(extends_ref: &str, config_dir: &Path) -> Option<PathBuf> {
    let mut current = Some(config_dir);
    while let Some(dir) = current {
        let node_modules = dir.join("node_modules");
        if node_modules.is_dir() {
            let pkg_path = node_modules.join(extends_ref);
            // Direct file (`pkg/tsconfig.custom.json`).
            if pkg_path.is_file() {
                return Some(pkg_path);
            }
            // `pkg/foo` without extension -> `.json`.
            if pkg_path.extension().is_none() {
                let with_json = node_modules.join(format!("{}.json", extends_ref));
                if with_json.is_file() {
                    return Some(with_json);
                }
            }
            // Package directory: read its package.json `tsconfig`/`exports`,
            // else default to `tsconfig.json`.
            if pkg_path.is_dir() {
                let pkg_json = pkg_path.join("package.json");
                if let Ok(content) = std::fs::read_to_string(&pkg_json) {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                        if let Some(tc) = json.get("tsconfig").and_then(|v| v.as_str()) {
                            let p = pkg_path.join(tc);
                            if p.is_file() {
                                return Some(p);
                            }
                        }
                    }
                }
                let default_tc = pkg_path.join("tsconfig.json");
                if default_tc.is_file() {
                    return Some(default_tc);
                }
            }
        }
        current = dir.parent();
    }
    None
}

/// Parse JSON-with-comments (JSONC): tsconfig allows `//` and `/* */`
/// comments and trailing commas. We strip comments + trailing commas, then
/// parse with `serde_json`. String contents are preserved verbatim.
fn parse_jsonc(input: &str) -> Option<serde_json::Value> {
    let stripped = strip_jsonc(input);
    serde_json::from_str(&stripped).ok()
}

/// Strip `//` / `/* */` comments (outside strings) and trailing commas.
fn strip_jsonc(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    let mut in_string = false;
    let mut escaped = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_string {
            out.push(b as char);
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' => {
                in_string = true;
                out.push('"');
                i += 1;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                // line comment
                i += 2;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                // block comment
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
            }
            _ => {
                out.push(b as char);
                i += 1;
            }
        }
    }
    strip_trailing_commas(&out)
}

/// Remove trailing commas before `}` or `]` (legal in JSONC, rejected by
/// serde_json). Skips commas inside string literals.
fn strip_trailing_commas(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut in_string = false;
    let mut escaped = false;
    let mut pending_comma: Option<usize> = None;
    for (idx, &b) in bytes.iter().enumerate() {
        if in_string {
            out.push(b as char);
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => {
                if pending_comma.take().is_some() {
                    out.push(',');
                }
                in_string = true;
                out.push('"');
            }
            b',' => {
                // Defer emitting until we know the next non-whitespace char.
                if pending_comma.is_some() {
                    out.push(',');
                }
                pending_comma = Some(idx);
            }
            b'}' | b']' => {
                // Drop the pending comma — it was a trailing comma.
                pending_comma = None;
                out.push(b as char);
            }
            c if (c as char).is_ascii_whitespace() => {
                // Whitespace between a comma and the next token keeps the comma
                // pending so a following `}`/`]` can still drop it.
                out.push(b as char);
            }
            _ => {
                if pending_comma.take().is_some() {
                    out.push(',');
                }
                out.push(b as char);
            }
        }
    }
    if pending_comma.is_some() {
        out.push(',');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn paths_cfg(base_dir: &str, entries: &[(&str, &[&str])]) -> TsConfig {
        let mut paths = HashMap::new();
        for (k, targets) in entries {
            paths.insert(
                k.to_string(),
                targets.iter().map(|s| s.to_string()).collect(),
            );
        }
        TsConfig {
            paths: Some(PathsConfig {
                base_dir: PathBuf::from(base_dir),
                paths,
            }),
            base_url: None,
        }
    }

    #[test]
    fn wildcard_capture() {
        let cfg = paths_cfg("/proj", &[("@/*", &["./src/*"])]);
        let targets = best_match_targets("@/util", &cfg.paths.as_ref().unwrap().paths).unwrap();
        assert_eq!(targets, vec!["./src/util".to_string()]);
    }

    #[test]
    fn longest_prefix_wins() {
        let cfg = paths_cfg(
            "/proj",
            &[("@/*", &["./src/*"]), ("@/components/*", &["./ui/*"])],
        );
        let targets =
            best_match_targets("@/components/Button", &cfg.paths.as_ref().unwrap().paths).unwrap();
        // The longer non-wildcard prefix `@/components/` must win.
        assert_eq!(targets, vec!["./ui/Button".to_string()]);
    }

    #[test]
    fn exact_key_match() {
        let cfg = paths_cfg("/proj", &[("config", &["./config/index.ts"])]);
        let targets = best_match_targets("config", &cfg.paths.as_ref().unwrap().paths).unwrap();
        assert_eq!(targets, vec!["./config/index.ts".to_string()]);
    }

    #[test]
    fn exact_outranks_wildcard() {
        let cfg = paths_cfg(
            "/proj",
            &[("@/special", &["./special.ts"]), ("@/*", &["./src/*"])],
        );
        let targets = best_match_targets("@/special", &cfg.paths.as_ref().unwrap().paths).unwrap();
        assert_eq!(targets, vec!["./special.ts".to_string()]);
    }

    #[test]
    fn multiple_candidates_preserve_order() {
        let cfg = paths_cfg("/proj", &[("@/*", &["./a/*", "./b/*", "./c/*"])]);
        let targets = best_match_targets("@/x", &cfg.paths.as_ref().unwrap().paths).unwrap();
        assert_eq!(
            targets,
            vec![
                "./a/x".to_string(),
                "./b/x".to_string(),
                "./c/x".to_string()
            ]
        );
    }

    #[test]
    fn no_match_returns_none() {
        let cfg = paths_cfg("/proj", &[("@/*", &["./src/*"])]);
        assert!(best_match_targets("other/thing", &cfg.paths.as_ref().unwrap().paths).is_none());
    }

    #[test]
    fn suffix_wildcard() {
        // TS allows the `*` not to be at the very end: `foo/*/bar`.
        let cfg = paths_cfg("/proj", &[("foo/*/bar", &["./lib/*/bar.ts"])]);
        let targets =
            best_match_targets("foo/mid/bar", &cfg.paths.as_ref().unwrap().paths).unwrap();
        assert_eq!(targets, vec!["./lib/mid/bar.ts".to_string()]);
    }

    #[test]
    fn jsonc_comments_and_trailing_commas() {
        let src = r#"{
            // line comment
            "compilerOptions": {
                /* block comment */
                "baseUrl": ".",
                "paths": {
                    "@/*": ["./src/*"], // trailing inline comment
                },
            },
        }"#;
        let json = parse_jsonc(src).expect("should parse jsonc");
        assert_eq!(json["compilerOptions"]["baseUrl"].as_str(), Some("."));
        assert_eq!(
            json["compilerOptions"]["paths"]["@/*"][0].as_str(),
            Some("./src/*")
        );
    }

    #[test]
    fn jsonc_preserves_string_slashes() {
        // `//` and `,` inside string values must NOT be stripped.
        let src = r#"{"url": "http://example.com/", "list": ["a,b"]}"#;
        let json = parse_jsonc(src).unwrap();
        assert_eq!(json["url"].as_str(), Some("http://example.com/"));
        assert_eq!(json["list"][0].as_str(), Some("a,b"));
    }

    #[test]
    fn baseurl_only_resolution() {
        // Build a temp project: baseUrl=src, no paths. `lib/x` should resolve
        // to src/lib/x.ts via classic baseUrl resolution.
        let tmp =
            std::env::temp_dir().join(format!("perry_tsconfig_baseurl_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src/lib")).unwrap();
        std::fs::write(tmp.join("src/lib/x.ts"), "export const x = 1;").unwrap();
        let cfg = TsConfig {
            paths: None,
            base_url: Some(tmp.join("src")),
        };
        let resolved = resolve_with_config("lib/x", &cfg).expect("baseUrl resolution");
        assert!(resolved.ends_with("x.ts"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn extends_merge() {
        // base tsconfig declares paths; derived extends it and adds baseUrl.
        let tmp =
            std::env::temp_dir().join(format!("perry_tsconfig_extends_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();
        std::fs::write(tmp.join("src/util.ts"), "export const x = 1;").unwrap();
        std::fs::write(
            tmp.join("tsconfig.base.json"),
            r#"{ "compilerOptions": { "baseUrl": ".", "paths": { "@/*": ["./src/*"] } } }"#,
        )
        .unwrap();
        std::fs::write(
            tmp.join("tsconfig.json"),
            r#"{ "extends": "./tsconfig.base.json", "compilerOptions": {} }"#,
        )
        .unwrap();

        let merged = build_merged_config(&tmp.join("tsconfig.json"), &mut Vec::new())
            .expect("merged config");
        // paths inherited from the base config; targets anchored at the base
        // config's dir (which == tmp here).
        let resolved = resolve_with_config("@/util", &merged).expect("resolve via extended paths");
        assert!(resolved.ends_with("util.ts"));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
