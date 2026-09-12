use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::path::Path;

use anyhow::{bail, Context, Result};

/// Existing package.json literals < perry.json JS expressions < repeated CLI flags.
/// Read before the build-cache probe so creating a config also invalidates it.
pub(super) fn load(root: &Path, cli: &[String]) -> Result<BTreeMap<String, String>> {
    let mut values = BTreeMap::new();
    if let Some(path) = root
        .ancestors()
        .map(|dir| dir.join("package.json"))
        .find(|p| p.is_file())
    {
        let package: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        if let Some(defines) = package
            .get("perry")
            .and_then(|p| p.get("define"))
            .and_then(|d| d.as_object())
        {
            for (key, value) in defines {
                values.insert(key.clone(), value.to_string());
            }
        }
    }
    if let Some(path) = root
        .ancestors()
        .map(|dir| dir.join("perry.json"))
        .find(|p| p.is_file())
    {
        let config: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)
            .with_context(|| format!("invalid {}", path.display()))?;
        if let Some(defines) = config.get("define") {
            let Some(defines) = defines.as_object() else {
                bail!("perry.json define must be an object");
            };
            for (key, value) in defines {
                values.insert(
                    key.clone(),
                    value
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| value.to_string()),
                );
            }
        }
    }
    for value in cli {
        let Some((key, expression)) = value.split_once('=') else {
            bail!("--define expects NAME=EXPR, got {value:?}");
        };
        values.insert(key.to_owned(), expression.to_owned());
    }
    Ok(values)
}

pub(super) fn object_hash(hir_hash: u64, defines: &BTreeMap<String, String>) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hir_hash.hash(&mut hasher);
    defines.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_key_includes_even_defines_not_read_by_this_module() {
        let empty = BTreeMap::new();
        let mut values = BTreeMap::from([("VERSION".into(), "'one'".into())]);
        let first = object_hash(42, &values);
        assert_ne!(first, object_hash(42, &empty));
        values.insert("VERSION".into(), "'two'".into());
        assert_ne!(first, object_hash(42, &values));
        values.insert("VERSION".into(), "'one'".into());
        assert_eq!(first, object_hash(42, &values));
    }
}
