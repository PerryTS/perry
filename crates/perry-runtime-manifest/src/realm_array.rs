//! Fixed array births derive length, index descriptors and their GC kind.
use super::{facts, info, Token};
use crate::realm_json::Value;
use perry_abi::realm_header::{ArrayElements, Integrity};
fn string(s: impl Into<String>) -> Value {
    Value::String(s.into())
}
fn number(n: u64) -> Value {
    Value::Number(n.to_string())
}
fn object<const N: usize>(fields: [(&str, Value); N]) -> Value {
    Value::Object(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
}
pub(super) fn empty(expression: &str, entry: &[Token<'_>]) -> Result<Value, String> {
    if expression != "module_array_value(&[])" {
        return Err("unsupported empty array birth".into());
    }
    let clauses = info::split(entry, ",");
    if clauses.len() != 1 {
        return Err("empty array properties and length are derived".into());
    }
    let name = facts::path(clauses[0])?;
    let gc = info::gc_constants()?;
    Ok(object([
        ("name", string(name)),
        ("kind", string("namespace")),
        (
            "properties",
            Value::Array(vec![object([
                ("key", string("length")),
                ("symbol", Value::Bool(false)),
                ("attributes", number(1)),
                (
                    "value",
                    object([
                        ("kind", string("number_bits")),
                        ("value", string(format!("{:016x}", 0.0f64.to_bits()))),
                    ]),
                ),
            ])]),
        ),
        ("own_key_order", Value::Array(vec![string("length")])),
        ("own_symbol_order", Value::Array(Vec::new())),
        ("prototype", string("Array.prototype")),
        ("births", Value::Array(vec![string("Array.prototype")])),
        (
            "header",
            super::derive::array_header(
                *gc.get("GC_TYPE_ARRAY").ok_or("missing array ABI")?,
                ArrayElements::Scalars,
                Integrity::Extensible,
            ),
        ),
    ]))
}
/// realm-v1 exposes the Set holder and its prototype; element storage is not a
/// field in this manifest version. The owning runtime expression retains FLAGS.
pub(super) fn set(entry: &[Token<'_>]) -> Result<Value, String> {
    let clauses = info::split(entry, ",");
    if clauses.len() != 1 {
        return Err("Set birth shape is derived from its factory".into());
    }
    let name = facts::path(clauses[0])?;
    let gc = info::gc_constants()?;
    Ok(object([
        ("name", string(name)),
        ("kind", string("namespace")),
        ("properties", Value::Array(Vec::new())),
        ("own_key_order", Value::Array(Vec::new())),
        ("own_symbol_order", Value::Array(Vec::new())),
        ("prototype", string("Set.prototype")),
        ("births", Value::Array(vec![string("Set.prototype")])),
        (
            "header",
            object([
                ("gc_type", number(gc["GC_TYPE_SET"])),
                ("reserved", number(0)),
            ]),
        ),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn set_birth_derives_holder_shape_without_array_length() {
        let d = set(&super::super::tokens("Example.flags").unwrap()).unwrap();
        assert_eq!(d.get("prototype").unwrap().string(), Some("Set.prototype"));
        assert_eq!(d.get("properties").unwrap().encode(), "[]");
        assert!(set(&super::super::tokens("Example.flags, length = 0").unwrap()).is_err());
    }
    #[test]
    fn empty_array_birth_has_only_derived_intrinsics() {
        let d = empty(
            "module_array_value(&[])",
            &super::super::tokens("Example.empty").unwrap(),
        )
        .unwrap();
        assert_eq!(d.get("own_key_order").unwrap().encode(), r#"["length"]"#);
        assert!(empty(
            "module_array_value(&[dynamic])",
            &super::super::tokens("Example.empty").unwrap()
        )
        .is_err());
        assert!(empty(
            "module_array_value(&[])",
            &super::super::tokens("Example.empty, length=0").unwrap()
        )
        .is_err());
    }
}
