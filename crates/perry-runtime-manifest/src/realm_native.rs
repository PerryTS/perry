//! The bound native-method birth recipe, shared with the runtime sentinel ABI.
use super::{facts::Facts, hash, info, tokens};
use crate::realm_json::Value;
use perry_abi::realm_header::{ClosureBirth, Integrity};
use std::collections::{BTreeMap, BTreeSet};

fn string(value: impl Into<String>) -> Value {
    Value::String(value.into())
}
fn number(value: u64) -> Value {
    Value::Number(value.to_string())
}
fn object<const N: usize>(fields: [(&str, Value); N]) -> Value {
    Value::Object(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
}

pub(super) type Abi = (info::Info, String, u64);
pub(super) fn runtime_abi() -> Result<Abi, String> {
    abi(include_str!("../../perry-runtime/src/closure/registry.rs"))
}
/// The ABI of a bound native constructor that captures its class identity.
fn constructor_abi() -> Result<Abi, String> {
    abi_of(
        include_str!("../../perry-runtime/src/closure/registry.rs"),
        "BOUND_NATIVE_CONSTRUCTOR_INFO",
    )
}
fn abi(source: &str) -> Result<Abi, String> {
    abi_of(source, "BOUND_METHOD_INFO")
}
fn abi_of(source: &str, info_static: &str) -> Result<Abi, String> {
    let ts = tokens(source)?;
    // The exported record: `fn_info!(export static pub BOUND_METHOD_INFO = sentinel path, n)`.
    if let Some(at) = ts.windows(4).position(|w| {
        w[0].text == "export"
            && w[1].text == "static"
            && w[2].text == "pub"
            && w[3].text == info_static
    }) {
        let decl = &ts[at + 4..];
        if decl.first().is_none_or(|t| t.text != "=")
            || decl.get(1).is_none_or(|t| t.text != "sentinel")
        {
            return Err("unsupported exported bound-method ABI".into());
        }
        let close = decl
            .iter()
            .position(|t| t.text == ")")
            .ok_or("unterminated exported bound-method ABI")?;
        let args = info::split(&decl[2..close], ",");
        if args.len() != 2 {
            return Err("unsupported exported bound-method ABI".into());
        }
        let path = info::text(args[0]);
        let symbol = path.rsplit("::").next().unwrap().to_owned();
        let params = info::integer(&info::text(args[1]), &BTreeMap::new())?;
        return sentinel_abi(&ts, symbol, params);
    }
    let start = ts
        .windows(2)
        .position(|w| w[0].text == "static" && w[1].text == info_static)
        .ok_or("missing bound-method ABI declaration")?;
    let end = start
        + ts[start..]
            .iter()
            .position(|t| t.text == ";")
            .ok_or("unterminated bound-method ABI")?;
    let decl = &ts[start..end];
    let from = decl
        .iter()
        .position(|t| t.text == "from_code")
        .ok_or("bound-method ABI must declare from_code")?;
    let open = from + 1;
    let close = super::closing_paren(decl, open)?;
    let args = info::split(&decl[open + 1..close], ",");
    if args.len() != 2 || args[0].len() != 1 || info::text(&decl[close + 1..]) != "}" {
        return Err("unsupported bound-method ABI expression".into());
    }
    let symbol = args[0][0].text.to_owned();
    let params = info::integer(&info::text(args[1]), &BTreeMap::new())?;
    sentinel_abi(&ts, symbol, params)
}

fn sentinel_abi(ts: &[super::Token<'_>], symbol: String, params: u64) -> Result<Abi, String> {
    let constant = ts
        .windows(2)
        .position(|w| w[0].text == "const" && w[1].text == symbol)
        .ok_or("missing native sentinel constant")?;
    let equals = constant
        + ts[constant..]
            .iter()
            .position(|t| t.text == "=")
            .ok_or("missing native sentinel value")?;
    let cast = equals
        + 1
        + ts[equals + 1..]
            .iter()
            .position(|t| t.text == "as")
            .ok_or("missing native sentinel pointer cast")?;
    let literal = info::text(&ts[equals + 1..cast]);
    let address = info::integer(literal.trim_end_matches("_u64"), &BTreeMap::new())?;
    let body = format!("crate::closure::{symbol}");
    Ok((
        info::Info {
            body: body.clone(),
            params,
            rest_fixed: 0,
            flags: 0,
            length: 0,
            declared: 0,
            plain_params: 0,
        },
        body,
        address,
    ))
}

/// Literal provider metadata consumed by realm_decl!'s runtime predicate arm.
pub(super) fn nonconstructors(
    ts: &[super::Token<'_>],
) -> Result<BTreeSet<(String, String)>, String> {
    let separator = ts
        .iter()
        .position(|t| t.text == ";")
        .ok_or("missing native metadata rows")?;
    if ts
        .first()
        .is_none_or(|t| t.text != "native_nonconstructors")
    {
        return Err("unknown native metadata declaration".into());
    }
    let mut entries = BTreeSet::new();
    for row in info::split(&ts[separator + 1..], ",") {
        let arrow = row
            .windows(2)
            .position(|w| w[0].text == "=" && w[1].text == ">")
            .ok_or("native metadata needs a module => keys row")?;
        let keys = &row[arrow + 2..];
        if keys.first().is_none_or(|t| t.text != "[") || keys.last().is_none_or(|t| t.text != "]") {
            return Err("native metadata keys must be a pattern list".into());
        }
        let literals = |tokens: &[super::Token<'_>]| -> Result<Vec<String>, String> {
            info::split(tokens, "|")
                .into_iter()
                .map(|item| {
                    if item.len() != 1 {
                        return Err("native metadata needs literal names".into());
                    }
                    item[0]
                        .string
                        .clone()
                        .ok_or_else(|| "native metadata needs literal names".into())
                })
                .collect()
        };
        for module in literals(&row[..arrow])? {
            for key in literals(&keys[1..keys.len() - 1])? {
                if !entries.insert((module.clone(), key)) {
                    return Err("duplicate native constructor metadata".into());
                }
            }
        }
    }
    Ok(entries)
}

/// Resolve a receiver through its actual provider declaration. Public aliases
/// (assert.strict, sys.types, etc.) do not define the dispatch module identity.
pub(super) fn resolve_receivers(
    declarations: &mut [Value],
    receivers: &[(String, String)],
) -> Result<(), String> {
    if receivers.is_empty() {
        return Ok(());
    }
    let namespaces = declarations
        .iter()
        .filter_map(|d| {
            Some((
                d.get("name")?.string()?.to_owned(),
                d.get("native_namespace")?.string()?.to_owned(),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let ts = tokens(include_str!(
        "../../perry-runtime/src/object/native_module/constructor_exports.rs"
    ))?;
    let start = ts
        .iter()
        .position(|t| t.text == "native_nonconstructors")
        .ok_or("missing native constructor metadata declaration")?;
    let mut end = start;
    while end < ts.len() && ts[end].text != "}" {
        end += 1;
    }
    let nonconstructors = nonconstructors(&ts[start..end])?;
    let receivers = receivers
        .iter()
        .map(|(name, receiver)| {
            let module = namespaces.get(receiver).ok_or_else(|| {
                format!("{name}: receiver {receiver} has no native provider declaration")
            })?;
            Ok((name.as_str(), module.as_str()))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    for declaration in declarations {
        let name = declaration
            .get("name")
            .and_then(Value::string)
            .ok_or("missing native method name")?;
        let Some(module) = receivers.get(name) else {
            continue;
        };
        let key = declaration
            .get("captures")
            .and_then(Value::array)
            .and_then(|values| values.get(1))
            .and_then(|capture| capture.get("value"))
            .and_then(Value::string)
            .ok_or("native callable is missing its dispatch key")?
            .to_owned();
        let mut keys = declaration
            .get("own_key_order")
            .and_then(Value::array)
            .ok_or("native callable has no key order")?
            .to_vec();
        let needs_prototype = declaration.get("kind").and_then(Value::string)
            == Some("constructor")
            || !nonconstructors.contains(&(module.to_string(), key));
        if needs_prototype {
            if !keys.contains(&string("prototype")) {
                keys.insert(2, string("prototype"));
            }
        } else {
            keys.retain(|key| key.string() != Some("prototype"));
        }
        let Value::Object(map) = declaration else {
            unreachable!()
        };
        map.insert("own_key_order".into(), Value::Array(keys));
    }
    Ok(())
}

pub(super) fn method(facts: Facts, abi: &Abi) -> Result<Value, String> {
    let facts_class_identity = facts.class_identity.clone();
    let facts_module = facts.module.clone();
    let identity_abi;
    let abi = match &facts.class_identity {
        Some(_) => {
            identity_abi = constructor_abi()?;
            &identity_abi
        }
        None => abi,
    };
    if facts.get.is_some()
        || facts.set.is_some()
        || facts.attrs.is_some()
        || facts.value.is_some()
        || facts.provider
    {
        return Err("bound-method birth cannot carry object facts".into());
    }
    if facts.alias.as_deref() == Some(&facts.name) {
        return Err("a callable alias must name a distinct binding".into());
    }
    let canonical = facts.canonical_name().to_owned();
    let receiver = facts
        .receiver
        .ok_or("bound-method birth requires a receiver")?;
    let kind = facts.kind.as_deref().unwrap_or("method");
    if !matches!(kind, "method" | "function" | "constructor" | "prototype") {
        return Err("unknown native callable birth kind".into());
    }
    if (facts.instance_attrs.is_some() || facts.extends.is_some() || facts.instance.is_some())
        && !matches!(kind, "constructor" | "prototype")
    {
        return Err("prototype attributes require a native constructor".into());
    }
    if kind == "prototype" && (facts.native_key.is_none() || facts.instance.is_none()) {
        return Err("native callable prototype needs its binding birth and instance".into());
    }
    let key_birth = facts.native_key.is_some();
    if key_birth && facts.instance_attrs == Some(1) {
        return Err("native parent-key births derive writable instance prototypes".into());
    }
    let dispatch_key = facts
        .native_key
        .clone()
        .unwrap_or(super::facts::public_name(&facts.name, kind)?);
    let label = dispatch_key
        .strip_prefix('#')
        .unwrap_or(&dispatch_key)
        .to_owned();
    let (info, body, address) = abi;
    let default_length = info.public_length(super::abi_constants())?;
    if (facts.spec_name.is_some() || facts.spec_length.is_some()) != facts.metadata_macro.is_some()
        || facts.emit_metadata != facts.metadata_macro.is_some()
    {
        return Err("native public metadata requires one named runtime projection".into());
    }
    if facts.spec_length == Some(default_length)
        || facts.spec_name.as_deref() == Some(label.as_str())
    {
        return Err("native default metadata is derived; omit the override".into());
    }
    let length = facts.spec_length.unwrap_or(default_length);
    let function_info = info.facts(format!("source:No symbol matches {address:#x}."), true);
    let info_symbol = hash::info_symbol(&body, &function_info);
    let property = |key: &str, value| {
        object([
            ("key", string(key)),
            ("symbol", Value::Bool(false)),
            ("attributes", number(4)),
            ("value", value),
        ])
    };
    let mut properties = vec![
        property(
            "length",
            object([
                ("kind", string("number_bits")),
                (
                    "value",
                    string(format!("{:016x}", (length as f64).to_bits())),
                ),
            ]),
        ),
        property(
            "name",
            object([
                ("kind", string("string")),
                (
                    "value",
                    string(facts.spec_name.clone().unwrap_or_else(|| label.clone())),
                ),
            ]),
        ),
    ];
    let mut keys = vec![string("length"), string("name")];
    keys.push(string("prototype"));
    let prototype = facts
        .extends
        .clone()
        .unwrap_or_else(|| "Function.prototype".into());
    let mut births = facts.births.into_iter().collect::<BTreeSet<_>>();
    births.extend([prototype.clone(), receiver.clone()]);
    births.remove(&canonical);
    if matches!(kind, "constructor" | "prototype") {
        let instance = facts
            .instance
            .clone()
            .unwrap_or_else(|| format!("{canonical}.prototype"));
        births.insert(instance.clone());
        properties.push(object([
            ("key", string("prototype")),
            ("symbol", Value::Bool(false)),
            (
                "attributes",
                number(facts.instance_attrs.unwrap_or(u64::from(key_birth))),
            ),
            (
                "value",
                object([("kind", string("reference")), ("value", string(instance))]),
            ),
        ]));
    }
    let mut captures = vec![
        object([
            ("kind", string("reference")),
            ("value", string(receiver.clone())),
        ]),
        object([
            ("kind", string("bytes")),
            ("value", string(dispatch_key.clone())),
        ]),
        object([
            ("kind", string("raw_word")),
            ("value", string(format!("0x{:x}", dispatch_key.len()))),
        ]),
    ];
    if let Some(identity) = &facts_class_identity {
        if kind != "constructor" {
            return Err("only a native constructor captures a class identity".into());
        }
        let ids = info::constants(include_str!("../../perry-abi/src/native_class_ids.rs"))?;
        let id = *ids
            .get(identity.as_str())
            .ok_or_else(|| format!("{identity} is not a perry_abi::native_class_ids constant"))?;
        captures.push(object([
            ("kind", string("raw_word")),
            ("value", string(format!("0x{:x}", (id as f64).to_bits()))),
        ]));
    }
    let mut declaration = object([
        ("name", string(canonical)),
        ("kind", string(kind)),
        ("properties", Value::Array(properties)),
        ("own_key_order", Value::Array(keys)),
        ("own_symbol_order", Value::Array(Vec::new())),
        (
            "header",
            object([
                ("gc_type", number(4)),
                (
                    "reserved",
                    // The receiver capture is a pointer. A bound native
                    // method carries no per-instance non-constructable bit
                    // at birth (WRT3a3 open question 2).
                    number(super::derive::closure_word(ClosureBirth {
                        pointer_free_captures: false,
                        constructable: true,
                        body_non_constructor: false,
                        integrity: Integrity::Extensible,
                    })),
                ),
                ("capture_flags", number(0)),
            ]),
        ),
        ("prototype", string(prototype)),
        ("length", number(length)),
        ("body", string(body.clone())),
        ("function_info", function_info),
        ("info_symbol", string(info_symbol)),
        ("captures", Value::Array(captures)),
        (
            "births",
            Value::Array(births.into_iter().map(string).collect()),
        ),
    ]);
    if kind == "constructor" {
        let Value::Object(map) = &mut declaration else {
            unreachable!()
        };
        map.insert(
            "construct".into(),
            string("crate::object::js_new_function_construct"),
        );
    }
    if let Some(module) = facts_module {
        let Value::Object(map) = &mut declaration else {
            unreachable!()
        };
        map.insert("module".into(), string(module));
    }
    Ok(declaration)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_keys_follow_the_receiver_provider_including_aliases() {
        let provider = object([
            ("name", string("node:sys.types")),
            ("native_namespace", string("util.types")),
        ]);
        let method = object([
            ("name", string("node:util/types.isMap")),
            (
                "captures",
                Value::Array(vec![
                    Value::Null,
                    object([("kind", string("bytes")), ("value", string("isMap"))]),
                ]),
            ),
            (
                "own_key_order",
                Value::Array(vec![string("length"), string("name")]),
            ),
        ]);
        let bindings = vec![("node:util/types.isMap".into(), "node:sys.types".into())];
        let mut declarations = vec![provider, method];
        resolve_receivers(&mut declarations, &bindings).unwrap();
        assert_eq!(
            declarations[1].get("own_key_order"),
            Some(&Value::Array(vec![string("length"), string("name")]))
        );
        let Value::Object(provider) = &mut declarations[0] else {
            unreachable!()
        };
        provider.insert("native_namespace".into(), string("fs"));
        resolve_receivers(&mut declarations, &bindings).unwrap();
        assert_eq!(
            declarations[1]
                .get("own_key_order")
                .unwrap()
                .array()
                .unwrap()
                .len(),
            3
        );
        declarations.remove(0);
        assert!(resolve_receivers(&mut declarations, &bindings).is_err());
    }

    #[test]
    fn alias_members_use_the_physical_owner_in_either_field_order() {
        let abi = runtime_abi().unwrap();
        let make = |tail: &str| {
            let source = format!(
                r#""node:assert.ok", provider=bound_method, receiver="node:assert", {tail}"#
            );
            let ts = tokens(&source).unwrap();
            super::super::facts::logical(super::super::facts::parse(&ts).unwrap(), "", &abi)
                .unwrap()
        };
        let a = make(r#"members={extra;}, alias="node:assert.default""#);
        let b = make(r#"alias="node:assert.default", members={extra;}"#);
        assert_eq!(a, b);
        assert_eq!(a.get("name").unwrap().string(), Some("node:assert.default"));
        assert!(a
            .get("births")
            .unwrap()
            .array()
            .unwrap()
            .contains(&string("node:assert.default.extra")));
        assert_eq!(
            a.get("captures").unwrap().array().unwrap()[1]
                .get("value")
                .unwrap()
                .string(),
            Some("ok")
        );
    }

    #[test]
    fn parent_key_birth_derives_internal_name_and_instance_attributes() {
        let ts = tokens(
            r#"Internal.parent, kind=prototype, receiver=Module, instance=Internal.prototype"#,
        )
        .unwrap();
        let mut facts = super::super::facts::parse(&ts).unwrap();
        facts.native_key = Some("#Hidden".into());
        let d = method(facts, &runtime_abi().unwrap()).unwrap();
        let properties = d.get("properties").unwrap().array().unwrap();
        assert_eq!(
            properties[1]
                .get("value")
                .unwrap()
                .get("value")
                .unwrap()
                .string(),
            Some("Hidden")
        );
        assert_eq!(properties[2].get("attributes").unwrap().encode(), "1");
        assert_eq!(
            d.get("captures").unwrap().array().unwrap()[1]
                .get("value")
                .unwrap()
                .string(),
            Some("#Hidden")
        );
        assert!(d.get("construct").is_none());
        let facts = super::super::facts::parse(&ts).unwrap();
        assert!(method(facts, &runtime_abi().unwrap()).is_err());
    }

    #[test]
    fn public_name_projection_does_not_rename_native_dispatch() {
        let ts=tokens(r#""node:fs.Stats", receiver=Module, spec={name="deprecated"}, emit=metadata(realm_stats)"#).unwrap();
        let d = method(
            super::super::facts::parse(&ts).unwrap(),
            &runtime_abi().unwrap(),
        )
        .unwrap();
        assert_eq!(
            d.get("captures").unwrap().array().unwrap()[1]
                .get("value")
                .unwrap()
                .string(),
            Some("Stats")
        );
        assert_eq!(
            d.get("properties").unwrap().array().unwrap()[1]
                .get("value")
                .unwrap()
                .get("value")
                .unwrap()
                .string(),
            Some("deprecated")
        );
    }

    #[test]
    fn native_constructor_declaration_rejects_ambiguous_rows() {
        assert!(nonconstructors(
            &tokens(
                "native_nonconstructors(m,p); \"fs\" => [\"readFile\"], \"fs\" => [\"readFile\"]"
            )
            .unwrap()
        )
        .is_err());
        assert!(nonconstructors(
            &tokens("native_nonconstructors(m,p); \"fs\" => dynamic()").unwrap()
        )
        .is_err());
    }

    #[test]
    fn native_info_and_sentinel_are_derived_from_the_runtime_declaration() {
        let source = "pub const BOUND_METHOD_FUNC_PTR: *const u8 = 0xBADD_DEAD_u64 as *const u8;
            pub static BOUND_METHOD_INFO: JsFunctionInfo = unsafe { JsFunctionInfo::from_code(BOUND_METHOD_FUNC_PTR, 0) };";
        let (info, body, address) = abi(source).unwrap();
        assert_eq!(info.params, 0);
        assert_eq!(address, 0xbadddead);
        assert_eq!(body, "crate::closure::BOUND_METHOD_FUNC_PTR");
        let (changed, _, address) = abi(&source
            .replace(", 0)", ", 3)")
            .replace("BADD_DEAD", "BADD_BEEF"))
        .unwrap();
        assert_eq!(changed.params, 3);
        assert_eq!(address, 0xbaddbeef);
        assert!(abi(&source.replace("from_code", "something_else")).is_err());
    }
}
