//! A plain object's scalar members are one declaration consumed by both the
//! runtime birth macro and the producer. Encodings and default order are derived.
use super::{facts, info, Token};
use crate::realm_json::Value;
use perry_abi::realm_header::{Integrity, ObjectBirth, ObjectExotic, ObjectLayout, PrototypeMark};
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
pub(super) fn birth(ts: &[Token<'_>]) -> Result<Value, String> {
    if ts.len() < 5 || ts[0].text != "object" || ts[2].text != "," || ts[4].text != ";" {
        return Err("object birth needs its realm path and local binding".into());
    }
    let name = ts[1]
        .string
        .clone()
        .ok_or("object birth needs a literal realm path")?;
    let mut properties = Vec::new();
    let mut keys = Vec::new();
    let mut seen = BTreeSet::new();
    for row in info::split(&ts[5..], ";") {
        if row.len() < 7
            || row[1].text != "="
            || row[2].text != ">"
            || row[4].text != "("
            || row.last().unwrap().text != ")"
        {
            return Err("object members need key => scalar(value)".into());
        }
        let key = row[0]
            .string
            .clone()
            .ok_or("object member needs a literal key")?;
        if !seen.insert(key.clone()) {
            return Err("duplicate object member".into());
        }
        let value = facts::constant(&row[5..row.len() - 1])?;
        let expected = match row[3].text {
            "string" => "string",
            "boolean" => "boolean",
            "number" => "number_bits",
            _ => return Err("unknown object scalar kind".into()),
        };
        if value.get("kind").and_then(Value::string) != Some(expected) {
            return Err("object scalar kind disagrees with its value".into());
        }
        keys.push(key.clone());
        properties.push(object([
            ("key", string(key)),
            ("symbol", Value::Bool(false)),
            ("attributes", number(7)),
            ("value", value),
        ]));
    }
    // Ordinary object's default enumeration: array-index keys first, followed
    // by other strings in declaration order. The wire properties are sorted.
    let index = |key: &str| {
        key.parse::<u32>()
            .ok()
            .filter(|n| *n != u32::MAX && n.to_string() == key)
    };
    keys.sort_by(|a, b| match (index(a), index(b)) {
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    properties.sort_by_key(|p| p.get("key").and_then(Value::string).unwrap().to_owned());
    let descriptors = super::derive::descriptors(&properties);
    Ok(object([
        ("name", string(name)),
        ("kind", string("namespace")),
        ("properties", Value::Array(properties)),
        (
            "own_key_order",
            Value::Array(keys.into_iter().map(string).collect()),
        ),
        ("own_symbol_order", Value::Array(Vec::new())),
        (
            "header",
            super::derive::object_header(
                super::abi_constants()["GC_TYPE_OBJECT"],
                0,
                ObjectBirth {
                    exotic: ObjectExotic::Ordinary,
                    null_prototype: false,
                    layout: ObjectLayout::Unmarked,
                    prototype_mark: PrototypeMark::OnFirstLink,
                    descriptors,
                    typed_array_prototype: false,
                    integrity: Integrity::Extensible,
                },
            ),
        ),
        ("prototype", string("Object.prototype")),
        ("births", Value::Array(vec![string("Object.prototype")])),
    ]))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runtime_scalar_birth_owns_values_and_derives_index_order() {
        let d=birth(&super::super::tokens(r#"object "Example", obj; "z"=>string("value"); "2"=>boolean(false); "1"=>number(-0.0);"#).unwrap()).unwrap();
        assert_eq!(d.get("own_key_order").unwrap().encode(), r#"["1","2","z"]"#);
        assert_eq!(
            d.get("properties").unwrap().array().unwrap()[0]
                .get("value")
                .unwrap()
                .get("value")
                .and_then(Value::string),
            Some("8000000000000000")
        );
        assert!(
            birth(&super::super::tokens(r#"object "Example",obj;"z"=>boolean(1);"#).unwrap())
                .is_err()
        );
    }
}
