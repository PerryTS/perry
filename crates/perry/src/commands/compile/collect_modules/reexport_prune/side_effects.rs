use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub(super) struct Contracts {
    files: HashMap<PathBuf, bool>,
    manifests: HashMap<PathBuf, Option<serde_json::Value>>,
}

impl Contracts {
    pub(super) fn is_pure(&mut self, path: &Path) -> bool {
        if let Some(pure) = self.files.get(path) {
            return *pure;
        }
        let pure = self.lookup(path);
        self.files.insert(path.to_owned(), pure);
        pure
    }

    fn lookup(&mut self, path: &Path) -> bool {
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
                    return false;
                };
                prefix.push(name);
                if name.as_os_str().to_string_lossy().starts_with('@') {
                    let Some(name) = components.next() else {
                        return false;
                    };
                    prefix.push(name);
                }
                package_root = Some(prefix.clone());
            }
        }
        let Some(root) = package_root else {
            return false;
        };
        for dir in path.parent().into_iter().flat_map(Path::ancestors) {
            let manifest = self.manifests.entry(dir.to_owned()).or_insert_with(|| {
                std::fs::read(dir.join("package.json"))
                    .ok()
                    .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            });
            if manifest.is_none() && dir.join("package.json").exists() {
                return false;
            }
            if let Some(value) = manifest.as_ref().and_then(|v| v.get("sideEffects")) {
                return match value {
                    serde_json::Value::Bool(false) => true,
                    serde_json::Value::Array(patterns) => {
                        let Ok(relative) = path.strip_prefix(dir) else {
                            return false;
                        };
                        let relative = relative.to_string_lossy().replace('\\', "/");
                        patterns.iter().all(|pattern| {
                            pattern
                                .as_str()
                                .is_some_and(|pattern| !may_match(pattern, &relative))
                        })
                    }
                    _ => false,
                };
            }
            if dir == root {
                break;
            }
        }
        false
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
