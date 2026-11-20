//! A package's `exports` map as `require()` reads it: conditions are tried in
//! the package's own key order (Node's rule), so `{"import": ..., "require":
//! ...}` and `{"default": ..., "require": ...}` pick the entry Node picks.
//! `serde_json::Value` sorts object keys, which put `default` before
//! `require` and sent `require("@cursor/sdk")` to the package's ESM build.

use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};

pub(super) enum OrderedJson {
    String(String),
    Array(Vec<OrderedJson>),
    Object(Vec<(String, OrderedJson)>),
    Other,
}

impl<'de> Deserialize<'de> for OrderedJson {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct OrderedVisitor;
        impl<'de> Visitor<'de> for OrderedVisitor {
            type Value = OrderedJson;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a JSON value")
            }
            fn visit_str<E>(self, v: &str) -> Result<OrderedJson, E> {
                Ok(OrderedJson::String(v.to_owned()))
            }
            fn visit_string<E>(self, v: String) -> Result<OrderedJson, E> {
                Ok(OrderedJson::String(v))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<OrderedJson, A::Error> {
                let mut items = Vec::new();
                while let Some(item) = seq.next_element()? {
                    items.push(item);
                }
                Ok(OrderedJson::Array(items))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<OrderedJson, A::Error> {
                let mut entries = Vec::new();
                while let Some(entry) = map.next_entry()? {
                    entries.push(entry);
                }
                Ok(OrderedJson::Object(entries))
            }
            fn visit_bool<E>(self, _: bool) -> Result<OrderedJson, E> {
                Ok(OrderedJson::Other)
            }
            fn visit_i64<E>(self, _: i64) -> Result<OrderedJson, E> {
                Ok(OrderedJson::Other)
            }
            fn visit_u64<E>(self, _: u64) -> Result<OrderedJson, E> {
                Ok(OrderedJson::Other)
            }
            fn visit_f64<E>(self, _: f64) -> Result<OrderedJson, E> {
                Ok(OrderedJson::Other)
            }
            fn visit_unit<E>(self) -> Result<OrderedJson, E> {
                Ok(OrderedJson::Other)
            }
        }
        deserializer.deserialize_any(OrderedVisitor)
    }
}

#[derive(serde::Deserialize)]
struct ManifestExports {
    exports: Option<OrderedJson>,
}

/// The `exports` field of a `package.json` text, in document order. `None`
/// when the manifest has none (or does not parse).
pub(super) fn manifest_exports(text: &str) -> Option<OrderedJson> {
    serde_json::from_str::<ManifestExports>(text).ok()?.exports
}

/// Resolve subpath `key` (`.` or `./sub`) for `require` with the conditions
/// `node`, `require`, `default`, first match in the package's order.
pub(super) fn resolve_require(exports: &OrderedJson, key: &str) -> Option<String> {
    match exports {
        OrderedJson::String(target) => Some(target.clone()),
        OrderedJson::Array(items) => items.iter().find_map(|item| resolve_require(item, key)),
        OrderedJson::Object(entries) => {
            if let Some((_, target)) = entries.iter().find(|(name, _)| name == key) {
                return resolve_require(target, key);
            }
            entries.iter().find_map(|(condition, target)| {
                matches!(condition.as_str(), "node" | "require" | "default")
                    .then(|| resolve_require(target, key))
                    .flatten()
            })
        }
        OrderedJson::Other => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn require_takes_the_first_matching_condition_in_package_order() {
        let text = r#"{"exports": {".": {"bun": "./b.js", "require": "./cjs/index.js",
            "import": "./esm/index.js", "default": "./esm/index.js"},
            "./sqlite": {"import": "./esm/sqlite.js", "require": "./cjs/sqlite.js"}}}"#;
        let exports = manifest_exports(text).expect("exports");
        assert_eq!(
            resolve_require(&exports, ".").as_deref(),
            Some("./cjs/index.js")
        );
        assert_eq!(
            resolve_require(&exports, "./sqlite").as_deref(),
            Some("./cjs/sqlite.js")
        );
        let default_first =
            manifest_exports(r#"{"exports": {"default": "./d.js", "require": "./r.js"}}"#)
                .expect("exports");
        assert_eq!(
            resolve_require(&default_first, ".").as_deref(),
            Some("./d.js")
        );
    }

    /// End to end through `resolve_request`: an installed package whose map lists
    /// `require` before `default` resolves to the `require` target.
    #[test]
    fn resolve_request_follows_the_condition_order_of_an_installed_package() {
        let root = std::env::temp_dir().join(format!("perry-exports-order-{}", std::process::id()));
        let package = root.join("node_modules").join("ordered");
        std::fs::create_dir_all(package.join("cjs")).unwrap();
        std::fs::create_dir_all(package.join("esm")).unwrap();
        std::fs::write(
            package.join("package.json"),
            r#"{"name": "ordered", "exports": {".": {"import": "./esm/index.js", "require": "./cjs/index.js", "default": "./esm/index.js"}}}"#,
        )
        .unwrap();
        std::fs::write(package.join("cjs/index.js"), "module.exports = 1;").unwrap();
        std::fs::write(package.join("esm/index.js"), "export default 2;").unwrap();
        let resolved = super::super::resolve_request(&root, "ordered");
        let _ = std::fs::remove_dir_all(&root);
        let Ok(resolved) = resolved else {
            panic!("the package resolves");
        };
        assert!(
            resolved.ends_with("cjs/index.js"),
            "require beats default when listed first, got {resolved:?}"
        );
    }
}
