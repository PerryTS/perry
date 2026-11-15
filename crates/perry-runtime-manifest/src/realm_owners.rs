//! Every native cell has exactly one owner: the `native_namespace` its birth
//! declares (the module js_create_native_module_namespace_vt stores in slot 0).
//! A native cell is a `%NativeNamespace:` receiver, a cell whose header has
//! the native-module class id, a cell dispatched by the native-module
//! vtable, or a cell that records a `native_namespace`. A
//! `%NativeNamespace:<m>%` name must spell its declared owner. A cell without
//! a declared owner fails by name; nothing here fills one in: the check reads
//! only what the producer recorded.
use crate::realm_json::Value;
use std::collections::BTreeMap;

const NATIVE_VTABLE: &str = "crate::object::native_module::NATIVE_MODULE_VTABLE_IMPL";
/// `NATIVE_MODULE_CLASS_ID` (perry-runtime object/native_module.rs).
const NATIVE_CLASS_ID: u64 = 0xffff_fffe;

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
        let class_id = declaration.get("header").and_then(|h| h.get("class_id"));
        let native = spelled.is_some()
            || class_id == Some(&Value::Number(NATIVE_CLASS_ID.to_string()))
            || field("exotic_vtable") == Some(NATIVE_VTABLE)
            || field("native_namespace").is_some();
        if !native {
            continue;
        }
        match (field("native_namespace"), spelled) {
            (None, _) => errors.push(format!(
                "{name}: native cell does not declare its native_namespace"
            )),
            (Some(owner), Some(spelled)) if owner != spelled => errors.push(format!(
                "{name}: native cell declares owner {owner} but is named for {spelled}"
            )),
            (Some(owner), _) => {
                owners.insert(name.to_owned(), owner.to_owned());
            }
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
        // The module namespace declares its owner; the ordinary performance
        // singleton has no native namespace owner.
        assert!(!owners.contains_key("performance"));
        for (cell, owner) in [
            ("%NativeNamespace:perf_hooks%", "perf_hooks"),
            ("%NativeNamespace:perf_observer_list%", "perf_observer_list"),
            ("node:path.posix", "path.posix.default"),
        ] {
            assert_eq!(owners.get(cell).map(String::as_str), Some(owner), "{cell}");
        }
    }

    #[test]
    fn a_native_cell_without_one_owner_fails_by_name() {
        let ok =
            parse(r#"[{"name":"%NativeNamespace:x%","native_namespace":"x"},{"name":"plain"}]"#);
        assert_eq!(native_owners(&ok).unwrap().len(), 1);
        for (json, needle) in [
            (
                r#"[{"name":"node:x.a","exotic_vtable":"crate::object::native_module::NATIVE_MODULE_VTABLE_IMPL"}]"#,
                "node:x.a: native cell does not declare its native_namespace",
            ),
            // A native header without a declared owner is not filled in from
            // its name, its `module` or anything else.
            (
                r#"[{"name":"performance","header":{"class_id":4294967294},"module":"perf_hooks"}]"#,
                "performance: native cell does not declare its native_namespace",
            ),
            (
                r#"[{"name":"%NativeNamespace:x%"}]"#,
                "%NativeNamespace:x%: native cell does not declare",
            ),
            (
                r#"[{"name":"%NativeNamespace:x%","native_namespace":"y"}]"#,
                "%NativeNamespace:x%: native cell declares owner y but is named for x",
            ),
        ] {
            let error = native_owners(&parse(json)).unwrap_err();
            assert!(error.contains(needle), "{json}: {error}");
        }
    }
}
