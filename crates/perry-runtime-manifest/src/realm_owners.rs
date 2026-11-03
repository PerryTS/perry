//! Every native cell has exactly one owner: the module its birth declares.
//! A native cell is a `%NativeNamespace:` receiver, a cell dispatched by the
//! native-module vtable, or a cell that records a `native_namespace`. Its
//! owner facts are the `native_namespace` it declares, the `module` it is
//! reached through, and the module a `%NativeNamespace:<m>%` name spells.
//! They must name one module; a cell with none fails by name. Nothing here
//! derives an owner: the check reads only what the producer recorded.
use crate::realm_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const NATIVE_VTABLE: &str = "crate::object::native_module::NATIVE_MODULE_VTABLE_IMPL";

pub(crate) fn native_owners(declarations: &[Value]) -> Result<BTreeMap<String, String>, String> {
    let mut owners = BTreeMap::new();
    let mut errors = Vec::new();
    for declaration in declarations {
        let Some(name) = declaration.get("name").and_then(Value::string) else {
            continue;
        };
        let field = |key: &str| declaration.get(key).and_then(Value::string);
        let spelled = name
            .strip_prefix("%NativeNamespace:")
            .and_then(|rest| rest.strip_suffix('%'));
        let native = spelled.is_some()
            || field("exotic_vtable") == Some(NATIVE_VTABLE)
            || field("native_namespace").is_some();
        if !native {
            continue;
        }
        let facts = [field("native_namespace"), field("module"), spelled]
            .into_iter()
            .flatten()
            .collect::<BTreeSet<_>>();
        match facts.len() {
            0 => errors.push(format!("{name}: native cell has no owner")),
            1 => {
                if field("native_namespace").is_none() {
                    errors.push(format!(
                        "{name}: native cell does not declare its native_namespace"
                    ));
                } else {
                    owners.insert(
                        name.to_owned(),
                        facts.into_iter().next().unwrap().to_owned(),
                    );
                }
            }
            _ => errors.push(format!("{name}: native cell has owners {facts:?}")),
        }
    }
    if errors.is_empty() {
        Ok(owners)
    } else {
        Err(errors.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Vec<Value> {
        crate::realm_json::parse(json)
            .unwrap()
            .array()
            .unwrap()
            .to_vec()
    }

    #[test]
    fn every_native_cell_of_the_runtime_has_exactly_one_owner() {
        let src =
            std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../perry-runtime/src"));
        let manifest = crate::realm_source::read(src).unwrap();
        let declarations = manifest.get("declarations").and_then(Value::array).unwrap();
        let owners = native_owners(declarations).unwrap_or_else(|e| panic!("{e}"));
        assert!(
            owners
                .keys()
                .filter(|n| n.starts_with("%NativeNamespace:"))
                .count()
                > 30,
            "the native receivers are checked"
        );
    }

    #[test]
    fn a_native_cell_without_one_owner_fails_by_name() {
        let ok =
            parse(r#"[{"name":"%NativeNamespace:x%","native_namespace":"x"},{"name":"plain"}]"#);
        assert_eq!(native_owners(&ok).unwrap().len(), 1);
        for (json, needle) in [
            (
                r#"[{"name":"node:x.a","exotic_vtable":"crate::object::native_module::NATIVE_MODULE_VTABLE_IMPL"}]"#,
                "node:x.a: native cell has no owner",
            ),
            (
                r#"[{"name":"%NativeNamespace:x%"}]"#,
                "%NativeNamespace:x%: native cell does not declare",
            ),
            (
                r#"[{"name":"node:x.b","native_namespace":"x","module":"y"}]"#,
                "node:x.b: native cell has owners",
            ),
            (
                r#"[{"name":"%NativeNamespace:x%","native_namespace":"y"}]"#,
                "%NativeNamespace:x%: native cell has owners",
            ),
        ] {
            let error = native_owners(&parse(json)).unwrap_err();
            assert!(error.contains(needle), "{json}: {error}");
        }
    }
}
