//! ESM namespace cells inherit export descriptors, key order and integrity from
//! the runtime's namespace-layout birth. Source contains export bindings only.
use super::{birth, facts, info, Token};
use crate::realm_json::Value;
use std::collections::BTreeSet;
fn string(value: impl Into<String>) -> Value {
    Value::String(value.into())
}
fn number(value: u64) -> Value {
    Value::Number(value.to_string())
}
fn object<const N: usize>(fields: [(&str, Value); N]) -> Value {
    Value::Object(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
}
pub(super) fn birth(entry: &[Token<'_>]) -> Result<Value, String> {
    let clauses = info::split(entry, ",");
    let name = facts::path(clauses.first().ok_or("module namespace needs an owner")?)?;
    let module = name
        .strip_prefix("node:")
        .ok_or("module namespace needs its node owner")?;
    let mut members = None;
    let mut extra = Vec::new();
    let mut seen = BTreeSet::new();
    for clause in &clauses[1..] {
        if clause.len() < 3 || clause[1].text != "=" || !seen.insert(clause[0].text) {
            return Err("malformed module export declaration".into());
        }
        match clause[0].text {
            "members" if clause[2].text == "{" && clause.last().unwrap().text == "}" => {
                members = Some(birth::members_with_defaults(
                    &name,
                    &clause[3..clause.len() - 1],
                    3,
                    3,
                )?);
            }
            "births" if clause[2].text == "[" && clause.last().unwrap().text == "]" => {
                for edge in info::split(&clause[3..clause.len() - 1], ",") {
                    extra.push(facts::path(edge)?);
                }
            }
            _ => {
                return Err(
                    "module kind, prototype, tag and default descriptors are derived".into(),
                )
            }
        }
    }
    let mut members = members.ok_or("module namespace needs exports")?;
    if !members.symbols.is_empty() {
        return Err("module tag is derived from its factory".into());
    }
    if members.properties.iter().any(|p| {
        p.get("attributes") != Some(&number(3)) || p.get("get").is_some() || p.get("set").is_some()
    }) {
        return Err("module export descriptors are intrinsic".into());
    }
    members
        .keys
        .sort_by_key(|v| v.string().unwrap().encode_utf16().collect::<Vec<_>>());
    members.properties.push(object([
        ("key", string("@@toStringTag")),
        ("symbol", Value::Bool(true)),
        ("attributes", number(0)),
        (
            "value",
            object([("kind", string("string")), ("value", string("Module"))]),
        ),
    ]));
    for edge in extra {
        if !members.births.insert(edge) {
            return Err("omit derived module birth edges".into());
        }
    }
    members.births.remove(&name);
    let source = include_str!("../../perry-runtime/src/object/namespace_create.rs");
    if !source.contains("(*obj).class_id = MODULE_NAMESPACE_CLASS_ID;") {
        return Err("namespace layout factory no longer stamps its module class".into());
    }
    let constants = info::constants(source)?;
    let gc = info::gc_constants()?;
    let abi = super::abi_constants();
    Ok(object([
        ("name", string(&name)),
        ("kind", string("namespace")),
        ("module", string(module)),
        ("properties", Value::Array(members.properties)),
        ("own_key_order", Value::Array(members.keys)),
        (
            "own_symbol_order",
            Value::Array(vec![string("@@toStringTag")]),
        ),
        (
            "births",
            Value::Array(members.births.into_iter().map(string).collect()),
        ),
        (
            "header",
            object([
                ("gc_type", number(abi["GC_TYPE_OBJECT"])),
                (
                    "class_id",
                    number(
                        *constants
                            .get("MODULE_NAMESPACE_CLASS_ID")
                            .ok_or("missing module namespace class declaration")?,
                    ),
                ),
                (
                    "reserved",
                    number(
                        abi["OBJ_FLAG_PLAIN_ORDINARY"]
                            | gc["OBJ_FLAG_HAS_DESCRIPTORS"]
                            | gc["OBJ_FLAG_NULL_PROTO"]
                            | gc["OBJ_FLAG_NO_EXTEND"],
                    ),
                ),
            ]),
        ),
    ]))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn named_class_constants_and_runtime_bindings_come_from_source() {
        let values = info::constants(
            "const MODULE_NAMESPACE_CLASS_ID:u32=0xFFFF_4E53; const unrelated:&str=\"ignored\";",
        )
        .unwrap();
        assert_eq!(values["MODULE_NAMESPACE_CLASS_ID"], 0xffff4e53);
        let dynamic =
            facts::constant(&super::super::tokens("target(process.arch)").unwrap()).unwrap();
        assert_eq!(
            dynamic.get("value").unwrap().string(),
            Some("target.process.arch")
        );
        assert!(facts::constant(&super::super::tokens("runtime(other.value)").unwrap()).is_err());
    }

    #[test]
    fn exports_have_module_order_and_intrinsic_descriptors() {
        let d = birth(
            &super::super::tokens(
                r#""node:sample", members={ "2"=2; "10"=10; alias=ref(Target); }"#,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            d.get("own_key_order").unwrap().encode(),
            r#"["10","2","alias"]"#
        );
        assert_eq!(d.get("module").unwrap().string(), Some("sample"));
        assert!(d.get("prototype").is_none());
        assert!(birth(
            &super::super::tokens(r#""node:sample", members={ x, attrs=writable|enumerable; }"#)
                .unwrap()
        )
        .is_err());
        assert!(birth(
            &super::super::tokens(r#""node:sample", members={ [Symbol.toStringTag]="Module"; }"#)
                .unwrap()
        )
        .is_err());
    }
}
