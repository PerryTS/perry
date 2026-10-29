//! Object members beside the expression which actually allocates their holder.
//! The expression remains runtime code; its allocation family supplies the ABI.
use super::{facts, info, Token};
use crate::realm_json::Value;
use std::collections::{BTreeMap, BTreeSet};

fn string(s: impl Into<String>) -> Value {
    Value::String(s.into())
}
fn number(n: u64) -> Value {
    Value::Number(n.to_string())
}
fn object<const N: usize>(fields: [(&str, Value); N]) -> Value {
    Value::Object(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
}
fn reference(name: String) -> Value {
    object([("kind", string("reference")), ("value", string(name))])
}
fn key(ts: &[Token<'_>]) -> Result<(String, bool), String> {
    if ts.first().is_some_and(|t| t.text == "[") && ts.last().is_some_and(|t| t.text == "]") {
        let inner = &ts[1..ts.len() - 1];
        if inner.len() == 1 {
            if let Some(s) = &inner[0].string {
                return Ok((s.clone(), true));
            }
        }
        let text = info::text(inner);
        return text
            .strip_prefix("Symbol.")
            .map(|s| (format!("@@{s}"), true))
            .ok_or("invalid symbol member".into());
    }
    if ts.len() != 1 {
        return Err("member key must be an identifier, string or symbol".into());
    }
    Ok((
        ts[0].string.clone().unwrap_or_else(|| ts[0].text.into()),
        false,
    ))
}
fn target(owner: &str, key: &str, symbol: bool) -> String {
    if !symbol && key == "constructor" {
        if let Some(owner) = owner.strip_suffix(".prototype") {
            return owner.into();
        }
    }
    if symbol {
        format!("{owner}[{}]", string(key).encode())
    } else {
        format!("{owner}.{key}")
    }
}

pub(super) struct Members {
    pub properties: Vec<Value>,
    pub keys: Vec<Value>,
    pub symbols: Vec<Value>,
    pub births: BTreeSet<String>,
    pub descriptors: bool,
    pub shorthand: BTreeSet<String>,
}
pub(super) fn members(owner: &str, ts: &[Token<'_>]) -> Result<Members, String> {
    members_with_defaults(owner, ts, 5, 0)
}

pub(super) fn members_with_defaults(
    owner: &str,
    ts: &[Token<'_>],
    reference_attrs: u64,
    constant_attrs: u64,
) -> Result<Members, String> {
    let mut result = Members {
        properties: Vec::new(),
        keys: Vec::new(),
        symbols: Vec::new(),
        births: BTreeSet::new(),
        descriptors: false,
        shorthand: BTreeSet::new(),
    };
    let mut seen = BTreeSet::new();
    let flags = BTreeMap::from([
        ("writable".into(), 1),
        ("enumerable".into(), 2),
        ("configurable".into(), 4),
        ("none".into(), 0),
    ]);
    for row in info::split(ts, ";") {
        let parts = info::split(row, ",");
        let main = parts.first().ok_or("empty object member")?;
        let halves = info::split(main, "=");
        let (label, symbol) = key(halves[0])?;
        if !seen.insert((symbol, label.clone())) {
            return Err("duplicate object member".into());
        }
        let mut accessor = false;
        let mut property = BTreeMap::from([
            ("key".into(), string(&label)),
            ("symbol".into(), Value::Bool(symbol)),
        ]);
        let default_attrs;
        if halves.len() == 1 {
            if parts.len() == 1 && !symbol {
                result.shorthand.insert(label.clone());
            }
            let value = target(owner, &label, symbol);
            result.births.insert(value.clone());
            property.insert("value".into(), reference(value));
            default_attrs = reference_attrs;
        } else if halves.len() == 2 {
            let value = halves[1];
            if value.first().is_some_and(|t| t.text == "ref")
                && value.get(1).is_some_and(|t| t.text == "(")
                && value.last().is_some_and(|t| t.text == ")")
            {
                let name = facts::path(&value[2..value.len() - 1])?;
                result.births.insert(name.clone());
                property.insert("value".into(), reference(name));
                default_attrs = reference_attrs;
            } else if value.first().is_some_and(|t| t.text == "accessor")
                && value.get(1).is_some_and(|t| t.text == "(")
                && value.last().is_some_and(|t| t.text == ")")
            {
                accessor = true;
                for side in info::split(&value[2..value.len() - 1], ",") {
                    if side.len() < 3
                        || side[1].text != "="
                        || !matches!(side[0].text, "get" | "set")
                    {
                        return Err("accessor member needs named get/set halves".into());
                    }
                    let name = facts::path(&side[2..])?;
                    result.births.insert(name.clone());
                    if property.insert(side[0].text.into(), string(name)).is_some() {
                        return Err("duplicate accessor half".into());
                    }
                }
                if property.len() == 2 {
                    return Err("accessor must name at least one half".into());
                }
                default_attrs = 4;
            } else {
                property.insert("value".into(), facts::constant(value)?);
                default_attrs = if symbol && label == "@@toStringTag" {
                    4
                } else {
                    constant_attrs
                };
            }
        } else {
            return Err("malformed object member value".into());
        }
        let attrs = if parts.len() == 1 {
            default_attrs
        } else if parts.len() == 2
            && parts[1].len() >= 3
            && parts[1][0].text == "attrs"
            && parts[1][1].text == "="
        {
            let attrs = info::integer(&info::text(&parts[1][2..]), &flags)?;
            if attrs > 7 || attrs == default_attrs {
                return Err("invalid or redundant member attributes".into());
            }
            attrs
        } else {
            return Err("unknown object member fact".into());
        };
        result.descriptors |= !symbol && (accessor || attrs != 7);
        property.insert("attributes".into(), number(attrs));
        result.properties.push(Value::Object(property));
        if symbol {
            result.symbols.push(string(label));
        } else {
            result.keys.push(string(label));
        }
    }
    let index = |v: &Value| {
        v.string().and_then(|s| {
            s.parse::<u32>()
                .ok()
                .filter(|n| *n != u32::MAX && n.to_string() == s)
        })
    };
    result.keys.sort_by(|a, b| match (index(a), index(b)) {
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    sort_properties(&mut result.properties);
    Ok(result)
}

/// Numeric-key/string-value rows are already the runtime's human constants.
fn pair_members(owner: &str, source: &str, table: &str) -> Result<Members, String> {
    let ts = super::tokens(source)?;
    let start = ts
        .windows(2)
        .position(|w| w[0].text == "const" && w[1].text == table)
        .ok_or("missing object constant table")?;
    let equals = start
        + ts[start..]
            .iter()
            .position(|t| t.text == "=")
            .ok_or("missing constant table values")?;
    let open = equals + 2;
    if ts[equals + 1].text != "&" || ts[open].text != "[" {
        return Err("constant table must be a slice".into());
    }
    let end = open
        + 1
        + ts[open + 1..]
            .iter()
            .position(|t| t.text == "]")
            .ok_or("unterminated constant table")?;
    let mut rows = Vec::new();
    for row in info::split(&ts[open + 1..end], ",") {
        if row.first().is_none_or(|t| t.text != "(") || row.last().is_none_or(|t| t.text != ")") {
            return Err("invalid constant pair".into());
        }
        let args = info::split(&row[1..row.len() - 1], ",");
        if args.len() != 2 {
            return Err("constant pair requires key and value".into());
        }
        let key = info::integer(&info::text(args[0]), &BTreeMap::new())?.to_string();
        let value = facts::constant(args[1])?;
        if value.get("kind").and_then(Value::string) != Some("string") {
            return Err("constant pair requires a string value".into());
        }
        rows.push(format!(
            "{}={},attrs=writable|enumerable|configurable",
            string(key).encode(),
            value.get("value").unwrap().encode()
        ));
    }
    members(owner, &super::tokens(&rows.join(";"))?)
}

fn sort_properties(properties: &mut [Value]) {
    properties.sort_by_key(|p| {
        let symbol = p.get("symbol") == Some(&Value::Bool(true));
        let key = p.get("key").unwrap().string().unwrap();
        let display = if symbol {
            if let Some(name) = key.strip_prefix("@@") {
                format!("Symbol(Symbol.{name})")
            } else if let Some(name) = key
                .strip_prefix("@for:")
                .or_else(|| key.strip_prefix("@runtime:"))
            {
                format!("Symbol({name})")
            } else {
                key.to_owned()
            }
        } else {
            key.to_owned()
        };
        (symbol, display.encode_utf16().collect::<Vec<_>>())
    });
}

pub(super) fn birth(expression: &[Token<'_>], entry: &[Token<'_>]) -> Result<Value, String> {
    birth_at(expression, entry, "")
}
pub(super) fn birth_at(
    expression: &[Token<'_>],
    entry: &[Token<'_>],
    source: &str,
) -> Result<Value, String> {
    let expression = info::text(expression);
    if expression
        .starts_with("crate::object::layout_desc::alloc_layout_prototype(std::ptr::addr_of!(")
    {
        return super::layout_birth::prototype(&expression, entry);
    }
    if expression.starts_with("module_set_value(") && expression.ends_with(')') {
        return super::array::set(entry);
    }
    for (factory, arity) in [
        ("perry_build_runtime_record", 2),
        ("perry_alloc_runtime_record", 1),
    ] {
        if let Some(args) = expression
            .strip_prefix(&format!("crate::object::runtime_layouts::{factory}("))
            .and_then(|s| s.strip_suffix(')'))
        {
            let args = super::tokens(args)?;
            let args = info::split(&args, ",");
            if args.len() != arity {
                return Err("runtime record requires its layout and values".into());
            }
            let layout = info::text(args[0]);
            let layout = layout
                .strip_prefix("std::ptr::addr_of!(crate::object::runtime_layouts::")
                .and_then(|s| s.strip_suffix(')'))
                .ok_or("runtime record requires its declared layout")?;
            return super::record::birth(&super::tokens(&format!("builder,{layout}"))?, entry);
        }
    }
    if expression == "super::layout_desc::perry_alloc_layout(desc)" {
        return super::module_namespace::birth(entry);
    }
    if expression == "module_array_value(&[])" {
        return super::array::empty(&expression, entry);
    }
    // This is the existing factory, not a second user-selectable layout word.
    let allocation = include_str!("../../perry-runtime/src/object/global_this/proto_room.rs");
    let factory = expression == "super::proto_room::alloc_builtin_prototype()"
        || expression.starts_with("super::proto_room::alloc_builtin_prototype_with_room(")
        || expression == "crate::object::object_alloc_prototype_old(0,0)";
    let allocator = expression
        .trim_start_matches("crate::object::")
        .trim_start_matches("perry_runtime::object::");
    let ordinary = allocator.starts_with("js_object_alloc(") && allocator.ends_with(')');
    let resolve_class = |expression: &str| -> Result<u64, String> {
        if let Ok(value) = info::integer(expression, &BTreeMap::new()) {
            return Ok(value);
        }
        let mut constants = info::constants(source)?;
        if let Ok(value) = info::integer(expression, &constants) {
            return Ok(value);
        }
        constants.extend(info::constants(include_str!(
            "../../perry-runtime/src/object/native_module.rs"
        ))?);
        info::integer(expression, &constants)
    };
    let allocation_class = if ordinary {
        let args = super::tokens(&allocator["js_object_alloc(".len()..allocator.len() - 1])?;
        resolve_class(&info::text(info::split(&args, ",")[0]))?
    } else {
        0
    };
    let null = allocator.starts_with("js_object_alloc_null_proto(0,") && allocator.ends_with(')');
    let native = expression == "js_object_alloc(NATIVE_MODULE_CLASS_ID,1)";
    let shaped = allocator.starts_with("js_object_alloc_with_shape(") && allocator.ends_with(')');
    if (!factory && !ordinary && !native && !null && !shaped)
        || !allocation.contains("crate::object::object_alloc_prototype_old(0, room)")
    {
        return Err(format!("unsupported object birth expression: {expression}"));
    }
    let clauses = info::split(entry, ",");
    let name = facts::path(clauses.first().ok_or("object birth needs a realm path")?)?;
    let mut kind = if native { "namespace" } else { "prototype" }.to_owned();
    let mut prototype = if null {
        String::new()
    } else {
        "Object.prototype".to_owned()
    };
    let mut extra = Vec::new();
    let mut declared = None;
    let mut seen = BTreeSet::new();
    for clause in &clauses[1..] {
        if clause.len() < 3 || clause[1].text != "=" || !seen.insert(clause[0].text) {
            return Err("malformed or duplicate object birth fact".into());
        }
        match clause[0].text {
            "kind" => kind = facts::path(&clause[2..])?,
            "extends" => {
                prototype = facts::path(&clause[2..])?;
                if prototype == "Object.prototype" {
                    return Err("omit default object inheritance".into());
                }
            }
            "members" if clause[2].text == "{" && clause.last().unwrap().text == "}" => {
                declared = Some(if native {
                    members_with_defaults(&name, &clause[3..clause.len() - 1], 7, 7)?
                } else {
                    members(&name, &clause[3..clause.len() - 1])?
                })
            }
            "members"
                if clause.len() == 6
                    && clause[2].text == "pairs"
                    && clause[3].text == "("
                    && clause[5].text == ")" =>
            {
                declared = Some(pair_members(&name, source, clause[4].text)?);
            }
            "births" if clause[2].text == "[" && clause.last().unwrap().text == "]" => {
                for path in info::split(&clause[3..clause.len() - 1], ",") {
                    extra.push(facts::path(path)?);
                }
            }
            _ => return Err(format!("unknown object birth fact {}", clause[0].text)),
        }
    }
    if !matches!(kind.as_str(), "prototype" | "namespace" | "exotic") {
        return Err("object birth kind must be prototype, namespace or exotic".into());
    }
    let mut members = declared.ok_or("object birth needs its members")?;
    if !prototype.is_empty() {
        members.births.insert(prototype.clone());
    }
    for edge in extra {
        if !members.births.insert(edge) {
            return Err("omit derived object birth edges".into());
        }
    }
    members.births.remove(&name);
    let gc = info::constants(include_str!("../../perry-runtime/src/gc/types.rs"))?;
    let mut reserved = if native {
        0
    } else {
        (if factory {
            gc["OBJ_FLAG_NEVER_MOVE"]
        } else {
            0
        }) | if members.descriptors {
            gc["OBJ_FLAG_HAS_DESCRIPTORS"]
        } else {
            0
        }
    };
    if shaped {
        reserved |=
            info::constants(include_str!("../../perry-abi/src/lib.rs"))?["OBJ_FLAG_PLAIN_ORDINARY"];
    }
    if null {
        reserved |= gc["OBJ_FLAG_NULL_PROTO"];
    }
    if kind == "namespace" && super::namespace::sealed_namespace(&name)? {
        reserved |= gc["OBJ_FLAG_NEVER_MOVE"];
    }
    if factory {
        if let Some(owner) = name.strip_suffix(".prototype") {
            let families = super::families::read(include_str!(
                "../../perry-runtime/src/object/global_this_tables.rs"
            ))?;
            if families
                .get(owner)
                .and_then(|family| family.get("instance_identity"))
                .and_then(Value::string)
                .is_some_and(|identity| {
                    identity.starts_with("BuiltinInstanceIdentity::Class(crate::typedarray::")
                })
            {
                if !include_str!("../../perry-runtime/src/object/global_this/populate.rs")
                    .contains("(*gc)._reserved |= crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO;")
                {
                    return Err(
                        "typed-array prototype birth no longer declares its runtime classification"
                            .into(),
                    );
                }
                reserved |= gc["OBJ_FLAG_TYPED_ARRAY_PROTO"];
            }
        }
    }
    let class_id = if native {
        *info::constants(include_str!(
            "../../perry-runtime/src/object/native_module.rs"
        ))?
        .get("NATIVE_MODULE_CLASS_ID")
        .ok_or("missing native namespace class declaration")?
    } else if shaped {
        let args =
            super::tokens(&allocator["js_object_alloc_with_shape(".len()..allocator.len() - 1])?;
        let key = info::text(info::split(&args, ",")[0]);
        let compact = info::text(&super::tokens(source)?);
        if compact.contains(&format!(".class_id={key};")) {
            resolve_class(&key)?
        } else {
            0
        }
    } else {
        allocation_class
    };
    let mut declaration = object([
        ("name", string(&name)),
        ("kind", string(&kind)),
        ("prototype", string(&prototype)),
        ("properties", Value::Array(members.properties)),
        ("own_key_order", Value::Array(members.keys)),
        ("own_symbol_order", Value::Array(members.symbols)),
        (
            "births",
            Value::Array(members.births.into_iter().map(string).collect()),
        ),
        (
            "header",
            object([
                ("gc_type", number(super::abi_constants()["GC_TYPE_OBJECT"])),
                ("class_id", number(class_id)),
                ("reserved", number(reserved)),
            ]),
        ),
    ]);
    let Value::Object(map) = &mut declaration else {
        unreachable!()
    };
    if prototype.is_empty() {
        map.remove("prototype");
    }
    if kind == "exotic" {
        map.insert(
            "value".into(),
            object([("kind", string("dynamic")), ("value", string(name))]),
        );
    }
    Ok(declaration)
}

/// Ordinary objects that are used as prototypes are marked by proto_validity.
/// The realm inheritance edges therefore imply their immovability flag.
pub(super) fn mark_inherited_objects(declarations: &mut [Value]) -> Result<(), String> {
    let parents = declarations
        .iter()
        .filter_map(|d| d.get("prototype").and_then(Value::string))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let gc = info::constants(include_str!("../../perry-runtime/src/gc/types.rs"))?;
    for declaration in declarations {
        let name = declaration
            .get("name")
            .and_then(Value::string)
            .ok_or("missing realm name")?;
        if !parents.contains(name) {
            continue;
        }
        let Some(header) = declaration.get("header") else {
            continue;
        };
        if header.get("gc_type") != Some(&number(2)) || header.get("class_id") != Some(&number(0)) {
            continue;
        }
        let Value::Object(map) = declaration else {
            unreachable!()
        };
        let Value::Object(header) = map.get_mut("header").unwrap() else {
            unreachable!()
        };
        let word = header["reserved"]
            .encode()
            .parse::<u64>()
            .map_err(|e| e.to_string())?;
        header.insert("reserved".into(), number(word | gc["OBJ_FLAG_NEVER_MOVE"]));
    }
    Ok(())
}

pub(super) fn extend_callable(
    mut declaration: Value,
    members: Option<Members>,
) -> Result<Value, String> {
    let Some(mut members) = members else {
        return Ok(declaration);
    };
    let name = declaration
        .get("name")
        .and_then(Value::string)
        .ok_or("callable has no name")?
        .to_owned();
    let reposition_prototype = members.shorthand.contains("prototype")
        && declaration
            .get("own_key_order")
            .and_then(Value::array)
            .is_some_and(|keys| keys.contains(&string("prototype")));
    if reposition_prototype && members.keys.first().and_then(Value::string) == Some("prototype") {
        return Err("omit default intrinsic prototype insertion order".into());
    }
    if reposition_prototype {
        members.properties.retain(|p| {
            !(p.get("symbol") == Some(&Value::Bool(false))
                && p.get("key").and_then(Value::string) == Some("prototype"))
        });
        members.births.remove(&format!("{name}.prototype"));
    }
    let Value::Object(map) = &mut declaration else {
        return Err("invalid callable object".into());
    };
    let Value::Array(properties) = map
        .get_mut("properties")
        .ok_or("callable has no properties")?
    else {
        return Err("invalid callable properties".into());
    };
    let mut seen = properties
        .iter()
        .map(|p| {
            (
                p.get("symbol") == Some(&Value::Bool(true)),
                p.get("key").and_then(Value::string).unwrap().to_owned(),
            )
        })
        .collect::<BTreeSet<_>>();
    for property in members.properties {
        let key = (
            property.get("symbol") == Some(&Value::Bool(true)),
            property
                .get("key")
                .and_then(Value::string)
                .unwrap()
                .to_owned(),
        );
        if !seen.insert(key) {
            return Err("a callable member repeats a derived intrinsic property".into());
        }
        properties.push(property);
    }
    sort_properties(properties);
    for (field, added) in [
        ("own_key_order", members.keys),
        ("own_symbol_order", members.symbols),
    ] {
        let Value::Array(keys) = map.get_mut(field).ok_or("callable has no key order")? else {
            return Err("invalid key order".into());
        };
        if field == "own_key_order" && reposition_prototype {
            keys.retain(|key| key.string() != Some("prototype"));
        }
        for key in added {
            if keys.contains(&key) {
                return Err("callable member repeats an intrinsic key".into());
            }
            keys.push(key);
        }
    }
    let Value::Array(births) = map.get_mut("births").ok_or("callable has no births")? else {
        return Err("invalid callable births".into());
    };
    let mut edges = births
        .iter()
        .filter_map(Value::string)
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    edges.extend(members.births);
    edges.remove(&name);
    *births = edges.into_iter().map(string).collect();
    Ok(declaration)
}

#[cfg(test)]
mod tests {
    #[test]
    fn allocation_class_and_constant_members_follow_existing_runtime_source() {
        let source = r#"const CLASS: u32 = 42; const STATUS: &[(u32,&str)] = &[(404,"Missing"),(200,"OK")];"#;
        let d = super::birth_at(
            &super::super::tokens("js_object_alloc(CLASS,0)").unwrap(),
            &super::super::tokens("Example,kind=namespace,extends=Example,members={}").unwrap(),
            source,
        )
        .unwrap();
        assert_eq!(
            d.get("header").unwrap().get("class_id").unwrap().encode(),
            "42"
        );
        assert_eq!(d.get("births").unwrap().encode(), "[]");
        let d = super::birth_at(
            &super::super::tokens("js_object_alloc_with_shape(0,2,keys,0)").unwrap(),
            &super::super::tokens("Example,kind=namespace,members=pairs(STATUS)").unwrap(),
            source,
        )
        .unwrap();
        assert_eq!(d.get("own_key_order").unwrap().encode(), r#"["200","404"]"#);
        assert_eq!(
            d.get("header").unwrap().get("reserved").unwrap().encode(),
            "512"
        );
        assert!(super::birth_at(
            &super::super::tokens("js_object_alloc(MISSING,0)").unwrap(),
            &super::super::tokens("Example,members={}").unwrap(),
            source
        )
        .is_err());
    }

    use super::super::tokens;
    use super::*;
    #[test]
    fn null_births_and_runtime_objects_derive_their_storage_facts() {
        let d = birth(
            &tokens("crate::object::js_object_alloc_null_proto(0,0)").unwrap(),
            &tokens(r#""node:module._cache", kind=namespace, members={}"#).unwrap(),
        )
        .unwrap();
        assert!(d.get("prototype").is_none());
        assert_eq!(d.get("births").unwrap().encode(), "[]");
        assert_eq!(d.get("header").unwrap().get("reserved"), Some(&number(64)));
        let d = birth(
            &tokens("js_object_alloc(0,limit)").unwrap(),
            &tokens("process.env, kind=exotic, members={}").unwrap(),
        )
        .unwrap();
        assert_eq!(
            d.get("value").unwrap().get("value").and_then(Value::string),
            Some("process.env")
        );
    }

    #[test]
    fn inheritance_pins_ordinary_parents_but_not_native_receivers() {
        let ordinary = birth(
            &tokens("js_object_alloc(0,0)").unwrap(),
            &tokens("Parent, members={}").unwrap(),
        )
        .unwrap();
        let child = birth(
            &tokens("js_object_alloc(0,0)").unwrap(),
            &tokens("Child, extends=Parent, members={}").unwrap(),
        )
        .unwrap();
        let native = birth(
            &tokens("js_object_alloc(NATIVE_MODULE_CLASS_ID,1)").unwrap(),
            &tokens("Native, extends=Child, members={}").unwrap(),
        )
        .unwrap();
        let leaf = birth(
            &tokens("js_object_alloc(0,0)").unwrap(),
            &tokens("Leaf, extends=Native, members={}").unwrap(),
        )
        .unwrap();
        let mut declarations = [ordinary, child, native, leaf];
        mark_inherited_objects(&mut declarations).unwrap();
        let words = declarations
            .iter()
            .map(|d| d.get("header").unwrap().get("reserved").unwrap().encode())
            .collect::<Vec<_>>();
        assert_eq!(words, ["128", "128", "0", "0"]);
    }

    #[test]
    fn symbol_descriptors_use_their_own_holder_and_display_order() {
        let d=members("A",&tokens(r#"[Symbol.asyncDispose]; ["@for:Stream.toAsyncStreamable"]; [Symbol.asyncIterator];"#).unwrap()).unwrap();
        assert!(!d.descriptors);
        let keys = d
            .properties
            .iter()
            .map(|p| p.get("key").unwrap().string().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            keys,
            [
                "@for:Stream.toAsyncStreamable",
                "@@asyncDispose",
                "@@asyncIterator"
            ]
        );
        assert_eq!(d.symbols[0].string(), Some("@@asyncDispose"));
    }

    #[test]
    fn members_derive_edges_order_accessors_and_default_attributes() {
        let d=birth(&tokens("super::proto_room::alloc_builtin_prototype()").unwrap(),&tokens(r#"Example.prototype, members={constructor; z; size=accessor(get="Example.prototype.size.<get>"); alias=ref(Example.prototype.z); [Symbol.toStringTag]="Example"; "2"=2; "1"=-0.0;}"#).unwrap()).unwrap();
        assert_eq!(
            d.get("own_key_order").unwrap().encode(),
            r#"["1","2","constructor","z","size","alias"]"#
        );
        assert_eq!(
            d.get("own_symbol_order").unwrap().encode(),
            r#"["@@toStringTag"]"#
        );
        assert_eq!(
            d.get("header").unwrap().get("reserved"),
            Some(&number(2176))
        );
        assert_eq!(
            d.get("births").unwrap().encode(),
            r#"["Example","Example.prototype.size.<get>","Example.prototype.z","Object.prototype"]"#
        );
        assert!(birth(
            &tokens("arbitrary()").unwrap(),
            &tokens("A,members={}").unwrap()
        )
        .is_err());
        assert!(members("A", &tokens("x,attrs=writable|configurable;").unwrap()).is_err());
        assert!(members("A", &tokens("x; x;").unwrap()).is_err());
    }
}
