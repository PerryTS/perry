//! The package.json exports field: conditions, subpath patterns and candidates.

use super::*;

/// Resolve exports field from package.json
pub(super) fn resolve_exports_with_conditions(
    exports: &serde_json::Value,
    subpath: &str,
    conditions: &[&str],
) -> Option<String> {
    match exports {
        serde_json::Value::String(s) => Some(s.clone()),
        // Node "exports" fallback arrays: return the first element that resolves.
        serde_json::Value::Array(items) => items
            .iter()
            .find_map(|item| resolve_exports_with_conditions(item, subpath, conditions)),
        serde_json::Value::Object(map) => {
            // Try the specific subpath first
            if let Some(entry) = map.get(subpath) {
                return resolve_exports_with_conditions(entry, subpath, conditions);
            }

            // Try wildcard patterns (e.g., "./*" -> "./src/*.ts")
            for (key, value) in map.iter() {
                if key.contains('*') {
                    // Convert "./*" to a prefix/suffix match
                    let parts: Vec<&str> = key.splitn(2, '*').collect();
                    if parts.len() == 2 {
                        let prefix = parts[0];
                        let suffix = parts[1];
                        if subpath.starts_with(prefix) && subpath.ends_with(suffix) {
                            let matched = &subpath[prefix.len()..subpath.len() - suffix.len()];
                            if let Some(template) =
                                resolve_exports_with_conditions(value, subpath, conditions)
                            {
                                return Some(template.replace('*', matched));
                            }
                        }
                    }
                }
            }

            // Try common conditions (for both main entry and subpath entries)
            // This handles the case where we've matched a subpath and now need to resolve the conditions.
            // "perry" is checked first so packages can ship a TypeScript source entry
            // intended for Perry compilation alongside a pre-built JS entry for Node/Bun.
            for condition in conditions {
                if let Some(entry) = map.get(*condition) {
                    return resolve_exports_with_conditions(entry, subpath, conditions);
                }
            }

            None
        }
        _ => None,
    }
}

/// Resolve exports field from package.json for executable module entries.
///
/// `node` is ranked ABOVE `default` (and above `import`/`module`) because perry
/// compiles a Node target: a package whose `exports` offers a `{ node, default }`
/// conditional pair ships its full Node API under `node` and a reduced
/// browser/edge build under `default`. Picking `default` drops Node-only
/// exports — e.g. `unicorn-magic` exposes `toPath`/`traversePathUp` only in its
/// `node` entry (`./node.js`); resolving `./default.js` left them undefined, so
/// `npm-run-path` (→ execa) failed to LINK (`undefined symbol
/// perry_fn_…unicorn_magic…__toPath`). Matches the `resolve_subpath_import`
/// condition order (chalk's `#supports-color` `{ node, default }`); the two
/// resolvers must agree.
// First-match convenience wrapper. Current call sites use
// `resolve_exports_candidates` / `resolve_exports_with_conditions` directly, so
// this has no non-test caller and would trip `-D dead-code`; kept as the
// documented single-result entry point (mirrors `resolve_subpath_import`).
#[allow(dead_code)]
pub(in crate::commands::compile) fn resolve_exports(
    exports: &serde_json::Value,
    subpath: &str,
) -> Option<String> {
    resolve_exports_with_conditions(exports, subpath, subpath_imports::default_conditions())
}

/// Like [`resolve_exports`], but returns EVERY condition branch's resolution
/// in priority order instead of only the first. Callers that check disk
/// existence (`resolve_package_entry`) walk the list so a pruned target
/// (Next.js standalone file tracing) falls through to the next condition.
pub(in crate::commands::compile) fn resolve_exports_candidates(
    exports: &serde_json::Value,
    subpath: &str,
) -> Vec<String> {
    // `node` ranked above `default` (perry compiles a Node target): a
    // `{ node, default }` conditional pair must resolve the Node build, not the
    // reduced browser/edge `default`. Candidates are collected in this order
    // and the caller picks the first that exists on disk, so listing `node`
    // first makes the Node entry win. (unicorn-magic exposes toPath/
    // traversePathUp only in `./node.js`; resolving `./default.js` left them
    // undefined → npm-run-path → execa LINK failure.) Mirrors `resolve_exports`
    // / `resolve_subpath_import`.

    fn collect(value: &serde_json::Value, subpath: &str, out: &mut Vec<String>) {
        match value {
            serde_json::Value::String(s) if !out.contains(s) => {
                out.push(s.clone());
            }
            // Node "exports" fallback arrays (e.g. y18n's
            // `[{ "import": "./index.mjs", "require": "./build/index.cjs" }, "./build/index.cjs"]`).
            // Each element is tried in order; we gather every resolution so the
            // disk-existence walk in `resolve_package_entry` can pick the first
            // that is present.
            serde_json::Value::Array(items) => {
                for item in items {
                    collect(item, subpath, out);
                }
            }
            serde_json::Value::Object(map) => {
                if let Some(entry) = map.get(subpath) {
                    collect(entry, subpath, out);
                    return;
                }
                let mut patterns: Vec<_> = map
                    .iter()
                    .filter_map(|(key, entry)| {
                        let (prefix, suffix) = key.split_once('*')?;
                        let end = subpath.len().checked_sub(suffix.len())?;
                        (subpath.starts_with(prefix)
                            && subpath.ends_with(suffix)
                            && prefix.len() <= end)
                            .then_some((prefix, suffix, end, entry))
                    })
                    .collect();
                patterns.sort_by(|a, b| {
                    b.0.len()
                        .cmp(&a.0.len())
                        .then_with(|| (b.0.len() + b.1.len()).cmp(&(a.0.len() + a.1.len())))
                });
                for (prefix, _, end, entry) in patterns {
                    let matched = &subpath[prefix.len()..end];
                    let mut templates = Vec::new();
                    collect(entry, subpath, &mut templates);
                    for template in templates {
                        let resolved = template.replace('*', matched);
                        if !out.contains(&resolved) {
                            out.push(resolved);
                        }
                    }
                }
                for condition in subpath_imports::default_conditions() {
                    if let Some(entry) = map.get(*condition) {
                        collect(entry, subpath, out);
                    }
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    collect(exports, subpath, &mut out);
    out
}
