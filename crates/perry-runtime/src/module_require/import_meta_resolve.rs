//! Runtime filesystem resolution for import.meta.resolve(specifier[, parent]).
//! Resolving a module does not load or execute it.

use super::*;
use serde_json::Value;
use std::path::{Path, PathBuf};

fn file_or_directory(path: &Path, depth: usize) -> Option<PathBuf> {
    if depth > 16 {
        return None;
    }
    if path.is_file() {
        return std::fs::canonicalize(path).ok();
    }
    if path.extension().is_none() {
        for ext in ["ts", "tsx", "js", "jsx", "mjs", "cjs", "json", "node"] {
            let candidate = path.with_extension(ext);
            if candidate.is_file() {
                return std::fs::canonicalize(candidate).ok();
            }
        }
    }
    if !path.is_dir() {
        return None;
    }
    if let Some(package) = manifest(path) {
        if let Some(main) = package
            .get("module")
            .or_else(|| package.get("main"))
            .and_then(Value::as_str)
        {
            if let Some(found) = file_or_directory(&path.join(main), depth + 1) {
                return Some(found);
            }
        }
    }
    for ext in ["ts", "tsx", "js", "jsx", "mjs", "cjs", "json", "node"] {
        let candidate = path.join(format!("index.{ext}"));
        if candidate.is_file() {
            return std::fs::canonicalize(candidate).ok();
        }
    }
    None
}

fn manifest(path: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(path.join("package.json")).ok()?).ok()
}

fn conditional_target(value: &Value) -> Option<&str> {
    match value {
        Value::String(target) => Some(target),
        Value::Array(targets) => targets.iter().find_map(conditional_target),
        Value::Object(conditions) => ["bun", "import", "node", "default"]
            .iter()
            .find_map(|condition| conditions.get(*condition).and_then(conditional_target)),
        _ => None,
    }
}

fn export_target(exports: &Value, key: &str) -> Option<String> {
    if let Value::Object(map) = exports {
        if map.keys().any(|key| key.starts_with('.')) {
            if let Some(value) = map.get(key) {
                return conditional_target(value).map(str::to_owned);
            }
            let mut patterns: Vec<_> = map
                .iter()
                .filter_map(|(pattern, value)| {
                    let (prefix, suffix) = pattern.split_once('*')?;
                    let matched = key.strip_prefix(prefix)?.strip_suffix(suffix)?;
                    Some((prefix.len(), suffix.len(), value, matched))
                })
                .collect();
            patterns.sort_by_key(|(prefix, suffix, _, _)| std::cmp::Reverse((*prefix, *suffix)));
            let (_, _, value, matched) = patterns.first()?;
            return conditional_target(value).map(|target| target.replace('*', matched));
        }
    }
    (key == ".")
        .then(|| conditional_target(exports))
        .flatten()
        .map(str::to_owned)
}

fn package_entry(root: &Path, subpath: &str) -> Option<PathBuf> {
    if let Some(package) = manifest(root) {
        if let Some(exports) = package.get("exports") {
            let key = if subpath.is_empty() {
                ".".to_owned()
            } else {
                format!("./{subpath}")
            };
            let target = export_target(exports, &key)?;
            if !target.starts_with("./")
                || target
                    .split('/')
                    .any(|part| part == ".." || part == "node_modules")
            {
                return None;
            }
            return file_or_directory(&root.join(target), 0);
        }
    }
    file_or_directory(&root.join(subpath), 0)
}

fn resolve_disk(specifier: &str, parent: &Path) -> Option<PathBuf> {
    let path = Path::new(specifier);
    if path.is_absolute() {
        return file_or_directory(path, 0);
    }
    if specifier.starts_with('.') {
        return file_or_directory(&parent.join(path), 0);
    }
    if specifier.is_empty() {
        return None;
    }
    let split = if specifier.starts_with('@') {
        let scope = specifier.find('/')?;
        specifier[scope + 1..]
            .find('/')
            .map(|index| scope + 1 + index)
    } else {
        specifier.find('/')
    };
    let (name, subpath) = split
        .map(|index| (&specifier[..index], &specifier[index + 1..]))
        .unwrap_or((specifier, ""));
    for ancestor in parent.ancestors() {
        let root = ancestor.join("node_modules").join(name);
        if root.is_dir() {
            return package_entry(&root, subpath);
        }
    }
    None
}

fn decode_path(value: &str) -> PathBuf {
    if value.starts_with("file:") {
        let decoded =
            crate::url::node_compat::js_url_file_url_to_path(string_value(value), undefined());
        PathBuf::from(value_to_string(decoded, "parent"))
    } else {
        PathBuf::from(value)
    }
}

#[no_mangle]
pub extern "C" fn js_import_meta_resolve(specifier: f64, parent: f64, fallback: f64) -> f64 {
    // Read all JS inputs before URL conversion can allocate/collect.
    let specifier = value_to_string(specifier, "specifier");
    let explicit = !JSValue::from_bits(parent.to_bits()).is_undefined();
    let parent = value_to_string(if explicit { parent } else { fallback }, "parent");
    if let Some(name) = supported_require_builtin(&specifier) {
        return string_value(&format!(
            "node:{}",
            name.strip_prefix("node:").unwrap_or(name)
        ));
    }
    let mut base = decode_path(&parent);
    if !explicit
        || (parent.starts_with("file:") && !parent.ends_with('/') && !base.is_dir())
        || base.is_file()
    {
        base.pop();
    }
    if !base.is_absolute() {
        base = std::env::current_dir().unwrap_or_default().join(base);
    }
    let resolved = if specifier.starts_with("file:") {
        file_or_directory(&decode_path(&specifier), 0)
    } else {
        resolve_disk(&specifier, &base)
    };
    let Some(path) = resolved else {
        crate::fs::validate::throw_error_with_code(
            &format!("Cannot find module '{specifier}' from '{parent}'"),
            "ERR_MODULE_NOT_FOUND",
        );
    };
    let path = path.to_string_lossy();
    let path = if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(&path).to_owned()
    };
    string_value(&crate::url::node_compat::path_to_file_url_string(
        &path,
        cfg!(windows),
    ))
}

extern "C" fn resolve_closure(closure: *mut ClosureHeader, specifier: f64, parent: f64) -> f64 {
    let fallback = js_closure_get_capture_f64(closure, 0);
    js_import_meta_resolve(specifier, parent, fallback)
}

#[no_mangle]
pub extern "C" fn js_import_meta_resolve_value(fallback: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let fallback = scope.root_nanbox_f64(fallback);
    let (closure, value) = named_closure(resolve_closure as *const u8, 2, 1, "resolve");
    js_closure_set_capture_f64(closure, 0, fallback.get_nanbox_f64());
    value
}

#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_RESOLVE: extern "C" fn(f64, f64, f64) -> f64 = js_import_meta_resolve;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_RESOLVE_VALUE: extern "C" fn(f64) -> f64 = js_import_meta_resolve_value;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_subpaths_conditions_patterns_and_blocking() {
        let exports = serde_json::json!({".": {"import": "./esm.js", "require": "./cjs.js"}, "./worker": "./parser.worker.js", "./assets/*": "./dist/*", "./private": null});
        assert_eq!(export_target(&exports, "."), Some("./esm.js".into()));
        assert_eq!(
            export_target(&exports, "./worker"),
            Some("./parser.worker.js".into())
        );
        assert_eq!(
            export_target(&exports, "./assets/tree.wasm"),
            Some("./dist/tree.wasm".into())
        );
        assert_eq!(export_target(&exports, "./private"), None);
        assert_eq!(export_target(&exports, "./missing"), None);
    }
}
