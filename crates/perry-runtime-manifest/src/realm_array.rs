//! Fixed array births derive length, index descriptors and their GC kind.
use super::{facts, info, Token};
use crate::realm_json::Value;
use perry_abi::realm_header::{ArrayElements, Integrity};
use std::collections::BTreeMap;
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

/// The runtime allocators of realm array births, after their module path.
const REALM_STRING_ARRAY: &str = "realm_string_array(";
const REALM_ARRAY_ALLOC: &str = "realm_array_alloc(";

/// Whether `expression` is a realm array birth ([`realm`] reads it).
pub(super) fn is_realm_birth(expression: &str) -> bool {
    let call = callee(expression);
    call.starts_with(REALM_STRING_ARRAY) || call.starts_with(REALM_ARRAY_ALLOC)
}

fn callee(expression: &str) -> &str {
    expression
        .trim_start_matches("crate::array::")
        .trim_start_matches("perry_runtime::array::")
}

/// The element kind a declaration names, as the derivation reads it.
fn elements_kind(kind: &str) -> Result<ArrayElements, String> {
    match kind {
        "string" | "object" => Ok(ArrayElements::Pointers),
        "number" => Ok(ArrayElements::Scalars),
        other => Err(format!("unknown array element kind {other}")),
    }
}

/// A `&[&str]` table declared in `source`: the elements of a static array
/// birth, in their order.
fn string_table(source: &str, name: &str) -> Result<Vec<String>, String> {
    let ts = super::tokens(source)?;
    let mut found = None;
    for i in 0..ts.len().saturating_sub(10) {
        let head = ts[i..i + 9].iter().map(|t| t.text).collect::<Vec<_>>();
        if head == ["const", name, ":", "&", "[", "&", "str", "]", "="]
            && ts[i + 9].text == "&"
            && ts.get(i + 10).is_some_and(|t| t.text == "[")
        {
            if found.is_some() {
                return Err(format!("array table {name} is declared twice"));
            }
            let mut items = Vec::new();
            let mut j = i + 11;
            while ts.get(j).is_some_and(|t| t.text != "]") {
                let item = ts[j]
                    .string
                    .clone()
                    .ok_or_else(|| format!("array table {name} holds a non-literal"))?;
                items.push(item);
                j += 1;
                if ts.get(j).is_some_and(|t| t.text == ",") {
                    j += 1;
                }
            }
            found = Some(items);
        }
    }
    found.ok_or_else(|| format!("missing array table {name} beside its birth"))
}

/// A realm array birth through `realm_string_array` (strings, filled in
/// order) or `realm_array_alloc` (an empty birth, or a provider birth whose
/// declared element kind the allocator's argument must repeat). The header
/// comes from `perry_abi::realm_header::array_reserved` of the same facts
/// the runtime birth passes.
pub(super) fn realm(expression: &str, entry: &[Token<'_>], source: &str) -> Result<Value, String> {
    let call = callee(expression);
    let args_text = call
        .split_once('(')
        .and_then(|(_, rest)| rest.strip_suffix(')'))
        .ok_or("realm array birth needs its arguments")?;
    let arg_tokens = super::tokens(args_text)?;
    let args = info::split(&arg_tokens, ",")
        .into_iter()
        .map(info::text)
        .collect::<Vec<_>>();
    let clauses = info::split(entry, ",");
    let name = facts::path(clauses.first().ok_or("array birth needs a realm path")?)?;
    let mut dynamic = false;
    let mut declared_kind = None;
    let mut source_table = None;
    let mut integrity = Integrity::Extensible;
    let mut members = None;
    let mut prototype_kind = false;
    let mut extends = None;
    let mut seen = std::collections::BTreeSet::new();
    for clause in &clauses[1..] {
        if clause.len() > 3
            && clause[0].text == "members"
            && clause[1].text == "="
            && clause[2].text == "{"
            && clause.last().is_some_and(|t| t.text == "}")
            && seen.insert("members")
        {
            members = Some(super::birth::members(&name, &clause[3..clause.len() - 1])?);
            continue;
        }
        if clause.len() >= 3 && clause[0].text == "extends" && clause[1].text == "=" {
            if !seen.insert("extends") {
                return Err(format!("{name}: duplicate array inheritance"));
            }
            extends = Some(facts::path(&clause[2..])?);
            continue;
        }
        if clause.len() != 3 || clause[1].text != "=" || !seen.insert(clause[0].text) {
            return Err(format!("{name}: malformed or duplicate array birth fact"));
        }
        match (clause[0].text, clause[2].text) {
            ("value", "dynamic") => dynamic = true,
            ("elements", kind) => declared_kind = Some(elements_kind(kind)?),
            ("elements_source", table) => source_table = Some(table.to_owned()),
            ("integrity", level) => integrity = super::derive::integrity(level)?,
            ("kind", "prototype") => prototype_kind = true,
            _ => {
                return Err(format!(
                    "{name}: unknown array birth fact {}",
                    clause[0].text
                ))
            }
        }
    }
    let gc = info::constants(include_str!("../../perry-runtime/src/gc/types.rs"))?;
    let gc_type = *gc.get("GC_TYPE_ARRAY").ok_or("missing array ABI")?;
    let base = |kind: &str, elements: ArrayElements| {
        BTreeMap::from([
            ("name".to_owned(), string(name.clone())),
            ("kind".to_owned(), string(kind)),
            ("own_symbol_order".to_owned(), Value::Array(Vec::new())),
            ("prototype".to_owned(), string("Array.prototype")),
            (
                "births".to_owned(),
                Value::Array(vec![string("Array.prototype")]),
            ),
            (
                "header".to_owned(),
                super::derive::array_header(gc_type, elements, integrity),
            ),
        ])
    };
    let elements = if call.starts_with(REALM_STRING_ARRAY) {
        if args.len() != 2 {
            return Err(format!(
                "{name}: realm_string_array takes items and an arena"
            ));
        }
        ArrayElements::Pointers
    } else {
        if args.len() != 3 {
            return Err(format!(
                "{name}: realm_array_alloc takes capacity, kind and arena"
            ));
        }
        let kind = args[1].rsplit("::").next().unwrap_or_default();
        match kind {
            "Scalars" => ArrayElements::Scalars,
            "Pointers" => ArrayElements::Pointers,
            "Mixed" => ArrayElements::Mixed,
            _ => {
                return Err(format!(
                    "{name}: unknown allocator element kind {}",
                    args[1]
                ))
            }
        }
    };
    if let Some(mut members) = members {
        // An array holder with own members (`%Array.prototype%`): an empty
        // scalar birth whose members the declaration lists in their order.
        if dynamic
            || source_table.is_some()
            || declared_kind.is_some()
            || !call.starts_with(REALM_ARRAY_ALLOC)
            || args[0] != "0"
            || elements != ArrayElements::Scalars
        {
            return Err(format!("{name}: an array holder is an empty scalar birth"));
        }
        let prototype = extends.unwrap_or_else(|| "Array.prototype".to_owned());
        if prototype == "Array.prototype" {
            return Err(format!("{name}: omit default array inheritance"));
        }
        members.births.insert(prototype.clone());
        members.births.remove(&name);
        let mut map = base(
            if prototype_kind {
                "prototype"
            } else {
                "namespace"
            },
            elements,
        );
        map.insert("prototype".into(), string(prototype));
        map.insert(
            "births".into(),
            Value::Array(members.births.into_iter().map(string).collect()),
        );
        map.insert("properties".into(), Value::Array(members.properties));
        map.insert("own_key_order".into(), Value::Array(members.keys));
        map.insert("own_symbol_order".into(), Value::Array(members.symbols));
        return Ok(Value::Object(map));
    }
    if prototype_kind || extends.is_some() {
        return Err(format!(
            "{name}: only an array holder declares its kind or inheritance"
        ));
    }
    if dynamic {
        if source_table.is_some() {
            return Err(format!("{name}: a dynamic array has no element table"));
        }
        if declared_kind != Some(elements) {
            return Err(format!(
                "{name}: the declared element kind must be the one the birth allocates"
            ));
        }
        let mut map = base("exotic", elements);
        map.insert("properties".into(), Value::Array(Vec::new()));
        map.insert("own_key_order".into(), Value::Array(Vec::new()));
        map.insert(
            "value".into(),
            object([("kind", string("dynamic")), ("value", string(name.clone()))]),
        );
        return Ok(Value::Object(map));
    }
    if declared_kind.is_some() {
        return Err(format!("{name}: a static array derives its element kind"));
    }
    let items = match source_table {
        Some(table) => {
            let passed = args[0].trim_start_matches('&');
            if call.starts_with(REALM_ARRAY_ALLOC) || passed.rsplit("::").next() != Some(&table) {
                return Err(format!(
                    "{name}: elements_source must name the table the birth iterates"
                ));
            }
            string_table(source, &table)?
        }
        None if call.starts_with(REALM_ARRAY_ALLOC)
            && args[0] == "0"
            && elements == ArrayElements::Scalars =>
        {
            Vec::new()
        }
        None => {
            return Err(format!(
                "{name}: array birth needs its elements or value = dynamic"
            ))
        }
    };
    // Integrity makes each own property non-configurable, and freezing
    // makes it non-writable; length is never enumerable or configurable.
    let element_attrs = match integrity {
        Integrity::Extensible | Integrity::NonExtensible => 7,
        Integrity::Sealed => 3,
        Integrity::Frozen => 2,
    };
    let length_attrs = if integrity == Integrity::Frozen { 0 } else { 1 };
    let mut properties = items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            object([
                ("key", string(i.to_string())),
                ("symbol", Value::Bool(false)),
                ("attributes", number(element_attrs)),
                (
                    "value",
                    object([("kind", string("string")), ("value", string(item.clone()))]),
                ),
            ])
        })
        .collect::<Vec<_>>();
    properties.push(object([
        ("key", string("length")),
        ("symbol", Value::Bool(false)),
        ("attributes", number(length_attrs)),
        (
            "value",
            object([
                ("kind", string("number_bits")),
                (
                    "value",
                    string(format!("{:016x}", (items.len() as f64).to_bits())),
                ),
            ]),
        ),
    ]));
    properties.sort_by_key(|p| p.get("key").and_then(Value::string).unwrap().to_owned());
    let mut keys = (0..items.len())
        .map(|i| string(i.to_string()))
        .collect::<Vec<_>>();
    keys.push(string("length"));
    let mut map = base("namespace", elements);
    map.insert("properties".into(), Value::Array(properties));
    map.insert("own_key_order".into(), Value::Array(keys));
    Ok(Value::Object(map))
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
