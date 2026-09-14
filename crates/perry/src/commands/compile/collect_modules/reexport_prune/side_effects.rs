use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub(super) struct Contracts {
    files: HashMap<PathBuf, Option<bool>>,
    manifests: HashMap<PathBuf, Option<serde_json::Value>>,
}

impl Contracts {
    /// None permits AST inference. An explicit effectful/unknown contract
    /// vetoes omission even when the current source looks inert.
    pub(super) fn is_pure(&mut self, path: &Path) -> Option<bool> {
        if let Some(pure) = self.files.get(path) {
            return *pure;
        }
        let pure = self.lookup(path);
        self.files.insert(path.to_owned(), pure);
        pure
    }

    fn lookup(&mut self, path: &Path) -> Option<bool> {
        // Stop at the owning package, never inherit a parent's contract across
        // nested node_modules. Type-only package.json files inside dist/ may
        // omit sideEffects; the owning package's declaration still applies.
        let mut package_root = None;
        let mut prefix = PathBuf::new();
        let mut components = path.components();
        while let Some(component) = components.next() {
            prefix.push(component);
            if component.as_os_str() == "node_modules" {
                let Some(name) = components.next() else {
                    return Some(false);
                };
                prefix.push(name);
                if name.as_os_str().to_string_lossy().starts_with('@') {
                    let Some(name) = components.next() else {
                        return Some(false);
                    };
                    prefix.push(name);
                }
                package_root = Some(prefix.clone());
            }
        }
        for dir in path.parent().into_iter().flat_map(Path::ancestors) {
            let manifest = self.manifests.entry(dir.to_owned()).or_insert_with(|| {
                std::fs::read(dir.join("package.json"))
                    .ok()
                    .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            });
            if manifest.is_none() && dir.join("package.json").exists() {
                return Some(false);
            }
            if let Some(value) = manifest.as_ref().and_then(|v| v.get("sideEffects")) {
                return Some(match value {
                    serde_json::Value::Bool(false) => true,
                    serde_json::Value::Array(patterns) => {
                        let Ok(relative) = path.strip_prefix(dir) else {
                            return Some(false);
                        };
                        let relative = relative.to_string_lossy().replace('\\', "/");
                        patterns.iter().all(|pattern| {
                            pattern
                                .as_str()
                                .is_some_and(|pattern| !may_match(pattern, &relative))
                        })
                    }
                    _ => false,
                });
            }
            // Canonical workspace-package paths may live outside node_modules.
            // Type-only dist manifests do not hide their owning contract.
            let boundary = package_root.as_deref().map_or_else(
                || {
                    manifest.as_ref().is_some_and(|value| {
                        !value
                            .as_object()
                            .is_some_and(|object| object.len() == 1 && object.contains_key("type"))
                    })
                },
                |root| dir == root,
            );
            if boundary {
                break;
            }
        }
        None
    }
}

/// Standard *, ** and ? globs. Unsupported syntax is treated as matching,
/// including negation/extglobs/braces/classes: uncertainty must retain files.
fn may_match(pattern: &str, path: &str) -> bool {
    if !pattern.is_ascii()
        || !path.is_ascii()
        || pattern.starts_with('/')
        || pattern.contains(['!', '[', ']', '{', '}', '(', ')', '\\'])
    {
        return true;
    }
    let rooted = pattern.starts_with("./");
    let pattern = pattern.strip_prefix("./").unwrap_or(pattern);
    let pattern = if rooted || pattern.contains('/') {
        pattern.to_owned()
    } else {
        format!("**/{pattern}")
    };
    glob(
        &pattern.split('/').collect::<Vec<_>>(),
        &path.split('/').collect::<Vec<_>>(),
    )
}

fn glob(pattern: &[&str], path: &[&str]) -> bool {
    let mut row = vec![false; path.len() + 1];
    row[0] = true;
    for part in pattern {
        let mut next = vec![false; path.len() + 1];
        if *part == "**" {
            next[0] = row[0];
        }
        for i in 1..=path.len() {
            next[i] = if *part == "**" {
                row[i] || next[i - 1]
            } else {
                row[i - 1] && segment(part.as_bytes(), path[i - 1].as_bytes())
            };
        }
        row = next;
    }
    row[path.len()]
}

fn segment(pattern: &[u8], text: &[u8]) -> bool {
    // Dynamic programming bounds adversarial sequences of stars to O(n*m).
    let mut row = vec![false; text.len() + 1];
    row[0] = true;
    for c in pattern {
        let mut next = vec![false; text.len() + 1];
        if *c == b'*' {
            next[0] = row[0];
        }
        for i in 1..=text.len() {
            next[i] = if *c == b'*' {
                row[i] || next[i - 1]
            } else {
                row[i - 1] && (*c == b'?' || *c == text[i - 1])
            };
        }
        row = next;
    }
    row[text.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_contracts_allow_inference_but_explicit_contracts_override_it() {
        let dir = tempfile::tempdir().unwrap();
        let package = dir.path().join("node_modules/fixture");
        std::fs::create_dir_all(package.join("dist")).unwrap();
        let module = package.join("dist/index.js");
        for (manifest, expected) in [
            (r#"{"name":"fixture"}"#, None),
            (r#"{"sideEffects":false}"#, Some(true)),
            (r#"{"sideEffects":true}"#, Some(false)),
            (r#"{"sideEffects":["**/*.js"]}"#, Some(false)),
            (r#"{"sideEffects":["**/*.css"]}"#, Some(true)),
            (r#"{"sideEffects":["[ab].js"]}"#, Some(false)),
            ("invalid json", Some(false)),
        ] {
            std::fs::write(package.join("package.json"), manifest).unwrap();
            assert_eq!(
                Contracts::default().is_pure(&module),
                expected,
                "{manifest}"
            );
        }
    }

    #[test]
    fn contracts_respect_workspace_and_nested_package_boundaries() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"sideEffects":false}"#).unwrap();
        let nested = dir.path().join("node_modules/fixture/dist");
        std::fs::create_dir_all(&nested).unwrap();
        let mut contracts = Contracts::default();
        assert_eq!(contracts.is_pure(&dir.path().join("source.js")), Some(true));
        assert_eq!(contracts.is_pure(&nested.join("index.js")), None);
        std::fs::write(nested.join("package.json"), r#"{"sideEffects":true}"#).unwrap();
        assert_eq!(
            Contracts::default().is_pure(&nested.join("index.js")),
            Some(false)
        );

        let dist = dir.path().join("dist/esm");
        std::fs::create_dir_all(&dist).unwrap();
        std::fs::write(dist.join("package.json"), r#"{"type":"module"}"#).unwrap();
        assert_eq!(
            Contracts::default().is_pure(&dist.join("index.js")),
            Some(true)
        );
    }

    #[test]
    fn glob_contracts_fail_closed() {
        for (pattern, path) in [
            ("*.css", "nested/a.css"),
            ("./init.js", "init.js"),
            ("**/init?.js", "deep/init1.js"),
            ("**/*.js", "a.js"),
            ("!*.js", "a.ts"),
            ("[ab].js", "c.ts"),
            ("?.js", "ä.js"),
        ] {
            assert!(may_match(pattern, path), "{pattern} {path}");
        }
        assert!(!may_match("./init.js", "nested/init.js"));
        assert!(!may_match("*.css", "a.js"));
    }
}
