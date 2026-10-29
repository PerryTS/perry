//! Human realm facts. ABI, default function properties and header encodings are
//! constructed here, rather than copied from a reflective snapshot.
use super::info::{self, Info};
use super::{hash, Token};
use crate::realm_json::Value;
use std::collections::{BTreeMap, BTreeSet};

fn string(value: impl Into<String>) -> Value {
    Value::String(value.into())
}
fn number(value: u64) -> Value {
    Value::Number(value.to_string())
}
fn object<const N: usize>(values: [(&str, Value); N]) -> Value {
    Value::Object(values.into_iter().map(|(k, v)| (k.into(), v)).collect())
}

pub(super) fn path(ts: &[Token<'_>]) -> Result<String, String> {
    if ts.len() == 1 {
        if let Some(value) = &ts[0].string {
            return Ok(value.clone());
        }
    }
    let value = info::text(ts);
    if value.is_empty() {
        return Err("empty realm path".into());
    }
    // The source uses Rust-shaped well-known-symbol keys; the v1 wire format
    // retains its established spelling.
    if let Some((owner, tail)) = value.split_once("[Symbol.") {
        if let Some((symbol, rest)) = tail.split_once(']') {
            return Ok(format!("{owner}[\"@@{symbol}\"]{rest}"));
        }
    }
    Ok(value)
}

pub(super) struct Facts {
    pub name: String,
    pub members: Option<super::birth::Members>,
    pub kind: Option<String>,
    pub constructible: bool,
    pub frozen: bool,
    pub capture_key: bool,
    pub capture_public_name: bool,
    pub capture_enum: Option<String>,
    pub capture_number: Option<String>,
    pub capture_bindings: Option<Vec<String>>,
    pub births: Vec<String>,
    pub provider: bool,
    pub bare_closure: bool,
    pub allocated_closure: bool,
    pub ansi_colors: bool,
    pub namespace_table: Option<String>,
    pub native_method: bool,
    pub native_key: Option<String>,
    pub builtin_constructor: bool,
    pub receiver: Option<String>,
    pub alias: Option<String>,
    pub get: Option<String>,
    pub set: Option<String>,
    pub attrs: Option<u64>,
    pub instance_attrs: Option<u64>,
    pub value: Option<Value>,
    pub spec_name: Option<String>,
    pub spec_length: Option<u64>,
    pub emit_metadata: bool,
    pub metadata_macro: Option<String>,
    pub instance: Option<String>,
    pub extends: Option<String>,
    pub family: Option<String>,
}

impl Facts {
    pub(super) fn canonical_name(&self) -> &str {
        self.alias
            .as_deref()
            .map_or(self.name.as_str(), |alias| self.name.as_str().min(alias))
    }
}

pub(super) fn parse_many(ts: &[Token<'_>]) -> Result<Vec<Facts>, String> {
    if ts.first().is_some_and(|t| t.text == "realm_decl") {
        if ts.get(1).is_none_or(|t| t.text != "!")
            || ts.get(2).is_none_or(|t| t.text != "{")
            || ts.last().is_none_or(|t| t.text != "}")
        {
            return Err("malformed shared-body realm_decl! block".into());
        }
        info::split(&ts[3..ts.len() - 1], ";")
            .into_iter()
            .map(parse)
            .collect()
    } else {
        Ok(vec![parse(ts)?])
    }
}

pub(super) fn parse(ts: &[Token<'_>]) -> Result<Facts, String> {
    let clauses = info::split(ts, ",");
    let name = path(clauses.first().ok_or("empty realm declaration")?)?;
    let mut facts = Facts {
        name,
        members: None,
        kind: None,
        constructible: false,
        frozen: false,
        capture_key: false,
        capture_public_name: false,
        capture_enum: None,
        capture_number: None,
        capture_bindings: None,
        births: Vec::new(),
        provider: false,
        bare_closure: false,
        allocated_closure: false,
        ansi_colors: false,
        namespace_table: None,
        native_method: false,
        native_key: None,
        builtin_constructor: false,
        receiver: None,
        alias: None,
        get: None,
        set: None,
        attrs: None,
        instance_attrs: None,
        value: None,
        spec_name: None,
        spec_length: None,
        emit_metadata: false,
        metadata_macro: None,
        instance: None,
        extends: None,
        family: None,
    };
    let mut fields = BTreeSet::new();
    let mut member_tokens = None;
    for clause in &clauses[1..] {
        if clause.len() < 3 || clause[1].text != "=" {
            return Err("realm facts require keyword arguments".into());
        }
        let field = clause[0].text;
        if !fields.insert(field) {
            return Err(format!("duplicate realm fact {field}"));
        }
        match field {
            "kind" => facts.kind = Some(path(&clause[2..])?),
            "capture" => {
                if clause[2].text == "[" && clause.last().is_some_and(|t| t.text == "]") {
                    facts.capture_bindings = Some(
                        info::split(&clause[3..clause.len() - 1], ",")
                            .iter()
                            .map(|row| info::text(row))
                            .collect(),
                    );
                    continue;
                }
                if clause[2].text == "number" {
                    if clause.len() != 6 || clause[3].text != "(" || clause[5].text != ")" {
                        return Err("numeric capture needs its existing constant".into());
                    }
                    facts.capture_number = Some(clause[4].text.into());
                    continue;
                }
                if clause[2].text == "enum_property" {
                    if clause.len() != 6 || clause[3].text != "(" || clause[5].text != ")" {
                        return Err("enum property capture needs its local enum".into());
                    }
                    facts.capture_enum = Some(clause[4].text.into());
                } else {
                    let binding = info::text(&clause[2..]);
                    if !matches!(binding.as_str(), "property_key" | "public_name") {
                        return Err("unknown symbolic closure binding".into());
                    }
                    facts.capture_key = true;
                    facts.capture_public_name = binding == "public_name";
                }
            }
            "integrity" => {
                if info::text(&clause[2..]) != "frozen" {
                    return Err("the only integrity level is frozen".into());
                }
                facts.frozen = true;
            }
            "constructible" => {
                if info::text(&clause[2..]) != "true" {
                    return Err("omit the default nonconstructible birth fact".into());
                }
                facts.constructible = true;
            }
            "instance" => facts.instance = Some(path(&clause[2..])?),
            "extends" => facts.extends = Some(path(&clause[2..])?),
            "family" => facts.family = Some(path(&clause[2..])?),
            "emit" => {
                if clause[2].text != "metadata" {
                    return Err("unknown realm metadata projection".into());
                }
                if clause.len() != 3 {
                    if clause.len() != 6 || clause[3].text != "(" || clause[5].text != ")" {
                        return Err("metadata projection needs one macro identifier".into());
                    }
                    let name = clause[4].text;
                    if !name.starts_with("realm_")
                        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    {
                        return Err("metadata macro must be an identifier beginning realm_".into());
                    }
                    facts.metadata_macro = Some(name.into());
                }
                facts.emit_metadata = true;
            }
            "spec" => {
                if clause[2].text != "{" || clause.last().is_none_or(|t| t.text != "}") {
                    return Err("spec overrides must be a block".into());
                }
                for spec in info::split(&clause[3..clause.len() - 1], ",") {
                    if spec.len() < 3 || spec[1].text != "=" {
                        return Err("malformed public function metadata".into());
                    }
                    match spec[0].text {
                        "name" if facts.spec_name.is_none() => {
                            if spec.len() != 3 {
                                return Err("public name must be a string literal".into());
                            }
                            facts.spec_name = Some(if spec[2].text == "property_key" {
                                let key = public_name(&facts.name, "method")?;
                                key.strip_prefix("get ")
                                    .or_else(|| key.strip_prefix("set "))
                                    .unwrap_or(&key)
                                    .to_owned()
                            } else {
                                spec[2]
                                    .string
                                    .clone()
                                    .ok_or("public name must be a literal or property_key")?
                            });
                        }
                        "length" if facts.spec_length.is_none() => {
                            facts.spec_length =
                                Some(info::integer(&info::text(&spec[2..]), &BTreeMap::new())?)
                        }
                        _ => return Err("unknown/duplicate public metadata override".into()),
                    }
                }
            }

            "provider" => {
                if clause[2].text == "namespace" {
                    if clause.len() != 6 || clause[3].text != "(" || clause[5].text != ")" {
                        return Err("namespace provider needs its Member declaration".into());
                    }
                    facts.namespace_table = Some(clause[4].text.into());
                } else {
                    match info::text(&clause[2..]).as_str() {
                        "native" => facts.provider = true,
                        "ansi_colors" => facts.ansi_colors = true,
                        "intrinsic_closure" => facts.allocated_closure = true,
                        "closure" => facts.bare_closure = true,
                        "closure_alloc" => {
                            facts.bare_closure = true;
                            facts.allocated_closure = true;
                        }
                        "bound_method" => facts.native_method = true,
                        "builtin_constructor" => facts.builtin_constructor = true,
                        _ => return Err("unknown realm provider".into()),
                    }
                }
            }
            "members" => {
                if clause[2].text != "{" || clause.last().is_none_or(|t| t.text != "}") {
                    return Err("callable members need a declaration block".into());
                }
                member_tokens = Some(&clause[3..clause.len() - 1]);
            }
            "receiver" => facts.receiver = Some(path(&clause[2..])?),
            "alias" => facts.alias = Some(path(&clause[2..])?),
            "get" => facts.get = Some(path(&clause[2..])?),
            "set" => facts.set = Some(path(&clause[2..])?),
            "attrs" => {
                if clause[2].text == "{" && clause.last().is_some_and(|t| t.text == "}") {
                    let fields = info::split(&clause[3..clause.len() - 1], ",");
                    if fields.len() != 1
                        || fields[0].len() < 3
                        || fields[0][0].text != "prototype"
                        || fields[0][1].text != "="
                    {
                        return Err(
                            "callable member attributes require the prototype property".into()
                        );
                    }
                    let flags = BTreeMap::from([
                        ("writable".into(), 1),
                        ("enumerable".into(), 2),
                        ("configurable".into(), 4),
                    ]);
                    let attrs = info::integer(&info::text(&fields[0][2..]), &flags)?;
                    if attrs == 0 || attrs > 7 {
                        return Err("omit default prototype attributes".into());
                    }
                    facts.instance_attrs = Some(attrs);
                } else {
                    let attrs = info::integer(&info::text(&clause[2..]), &BTreeMap::new())?;
                    if attrs > 7 {
                        return Err("invalid realm property attributes".into());
                    }
                    facts.attrs = Some(attrs);
                }
            }
            "value" => facts.value = Some(constant(&clause[2..])?),

            "births" => {
                if clause[2].text != "[" || clause.last().is_none_or(|t| t.text != "]") {
                    return Err("births must be a list of realm paths".into());
                }
                facts.births = info::split(&clause[3..clause.len() - 1], ",")
                    .iter()
                    .map(|item| path(item))
                    .collect::<Result<_, _>>()?;
            }
            "source" | "line" | "header" | "number_bits" | "info_symbol" | "function_info"
            | "capture_flags" | "own_key_order" | "own_symbol_order" | "flags" | "length"
            | "name" | "prototype" => {
                return Err(format!(
                    "{field} is derived; it cannot be a realm source fact"
                ));
            }
            _ => return Err(format!("unknown realm fact {field}")),
        }
    }
    if let Some(tokens) = member_tokens {
        facts.members = Some(super::birth::members(facts.canonical_name(), tokens)?);
    }
    Ok(facts)
}

pub(super) fn constant(ts: &[Token<'_>]) -> Result<Value, String> {
    if ts
        .first()
        .is_some_and(|t| matches!(t.text, "runtime" | "target"))
    {
        if ts.len() < 4 || ts[1].text != "(" || ts.last().unwrap().text != ")" {
            return Err("runtime value requires its provider path".into());
        }
        let binding = path(&ts[2..ts.len() - 1])?;
        if !binding.starts_with("process.") && !binding.starts_with("navigator.") {
            return Err("unknown runtime value provider".into());
        }
        let binding = if ts[0].text == "target" {
            format!("target.{binding}")
        } else {
            binding
        };
        return Ok(object([
            ("kind", string("dynamic")),
            ("value", string(binding)),
        ]));
    }
    if ts.len() == 1 {
        if let Some(value) = &ts[0].string {
            return Ok(object([
                ("kind", string("string")),
                ("value", string(value.clone())),
            ]));
        }
    }
    if ts.first().is_some_and(|t| t.text == "Symbol") {
        let identity = if ts.len() == 3 && ts[1].text == "." {
            format!("@@{}", ts[2].text)
        } else if ts.len() == 6
            && ts[1].text == "."
            && ts[2].text == "for"
            && ts[3].text == "("
            && ts[5].text == ")"
        {
            format!(
                "@for:{}",
                ts[4]
                    .string
                    .as_ref()
                    .ok_or("Symbol.for needs a literal key")?
            )
        } else if ts.len() == 4 && ts[1].text == "(" && ts[3].text == ")" {
            format!(
                "@runtime:{}",
                ts[2]
                    .string
                    .as_ref()
                    .ok_or("Symbol needs a literal description")?
            )
        } else {
            return Err("invalid symbolic constant".into());
        };
        return Ok(object([
            ("kind", string("symbol")),
            ("value", string(identity)),
        ]));
    }
    let text = info::text(ts);
    match text.as_str() {
        "undefined" | "null" => Ok(object([("kind", string(text))])),
        "true" | "false" => Ok(object([
            ("kind", string("boolean")),
            ("value", Value::Bool(text == "true")),
        ])),
        _ => {
            let number = match text.as_str() {
                "Infinity" => f64::INFINITY,
                "NaN" => f64::NAN,
                "-Infinity" => f64::NEG_INFINITY,
                _ => text
                    .parse::<f64>()
                    .map_err(|_| format!("invalid realm constant: {text}"))?,
            };
            Ok(object([
                ("kind", string("number_bits")),
                ("value", string(format!("{:016x}", number.to_bits()))),
            ]))
        }
    }
}

/// The modules js_create_native_module_namespace seals after the birth:
/// `if module_name == "m" { crate::object::js_object_seal(value); }`.
fn sealed_native_namespaces(source: &str) -> Result<Vec<&str>, String> {
    let body = source
        .split_once("pub extern \"C\" fn js_create_native_module_namespace(")
        .ok_or("missing native namespace birth")?
        .1;
    let body = body.split_once("\n}\n").map_or(body, |(b, _)| b);
    let mut sealed = Vec::new();
    for (i, _) in body.match_indices("crate::object::js_object_seal(value)") {
        let head = &body[..i];
        let cond = head
            .rsplit_once("if module_name == \"")
            .ok_or("unconditioned seal")?
            .1;
        let module = cond.split_once('"').ok_or("malformed seal condition")?.0;
        sealed.push(module);
    }
    Ok(sealed)
}

pub(super) fn logical(
    mut facts: Facts,
    source: &str,
    native_abi: &super::native::Abi,
) -> Result<Value, String> {
    if (facts.instance_attrs.is_some() && !facts.native_method)
        || facts.ansi_colors
        || facts.bare_closure
        || facts.allocated_closure
        || facts.capture_enum.is_some()
        || facts.capture_number.is_some()
        || facts.capture_bindings.is_some()
        || facts.capture_key
        || facts.builtin_constructor
        || facts.constructible
        || ((facts.spec_name.is_some() || facts.spec_length.is_some() || facts.emit_metadata)
            && !facts.native_method)
        || ((facts.instance.is_some() || facts.extends.is_some()) && !facts.native_method)
        || facts.family.is_some()
    {
        return Err("callable specialization requires a fn_info declaration".into());
    }
    if facts.native_method {
        let members = facts.members.take();
        return super::birth::extend_callable(super::native::method(facts, native_abi)?, members);
    }
    if facts.members.is_some() {
        return Err("object members require an owned birth expression".into());
    }
    if facts.receiver.is_some() {
        return Err("receiver is only a bound-method birth fact".into());
    }
    if let Some(table) = facts.namespace_table {
        if facts
            .kind
            .as_deref()
            .is_some_and(|kind| kind != "namespace")
            || facts.provider
            || facts.alias.is_some()
            || facts.get.is_some()
            || facts.set.is_some()
            || facts.attrs.is_some()
            || facts.value.is_some()
        {
            return Err("namespace Member rows own their property facts".into());
        }
        return super::namespace::birth(&facts.name, &table, source, facts.births);
    }
    let mut births = facts.births.into_iter().collect::<BTreeSet<_>>();
    let mut declaration = BTreeMap::from([
        ("name".into(), string(facts.name.clone())),
        ("own_symbol_order".into(), Value::Array(Vec::new())),
    ]);
    let kind = if facts.provider {
        let (namespace, kind) = if let Some(module) = facts.name.strip_prefix("node:") {
            declaration.insert("module".into(), string(module));
            declaration.insert(
                "body".into(),
                string("crate::object::native_module::js_create_native_module_namespace"),
            );
            (module, "namespace")
        } else if let Some(module) = facts
            .name
            .strip_prefix("%NativeNamespace:")
            .and_then(|s| s.strip_suffix('%'))
        {
            // The receiver's physical birth: an ordinary object allocated with the
            // native-module class id (js_create_native_module_namespace).
            let gc = info::gc_constants()?;
            let native = info::constants(include_str!(
                "../../perry-runtime/src/object/native_module.rs"
            ))?;
            let source = include_str!("../../perry-runtime/src/object/native_module.rs");
            let reserved = if sealed_native_namespaces(source)?.contains(&module) {
                gc["OBJ_FLAG_SEALED"] | gc["OBJ_FLAG_NO_EXTEND"] | gc["OBJ_FLAG_HAS_DESCRIPTORS"]
            } else {
                0
            };
            declaration.insert(
                "header".into(),
                Value::Object(BTreeMap::from([
                    ("gc_type".into(), number(gc["GC_TYPE_OBJECT"])),
                    ("reserved".into(), number(reserved)),
                    ("class_id".into(), number(native["NATIVE_MODULE_CLASS_ID"])),
                ])),
            );
            declaration.insert("prototype".into(), string("Object.prototype"));
            births.insert("Object.prototype".into());
            (module, "exotic")
        } else {
            return Err("native provider needs node: or %NativeNamespace: path".into());
        };
        declaration.insert("native_namespace".into(), string(namespace));
        declaration.insert(
            "exotic_vtable".into(),
            string("crate::object::native_module::NATIVE_MODULE_VTABLE_IMPL"),
        );
        kind
    } else if let Some(value) = facts.value {
        declaration.insert("value".into(), value);
        declaration.insert("attributes".into(), number(facts.attrs.unwrap_or(0)));
        "constant"
    } else if facts.alias.is_some() || facts.get.is_some() || facts.set.is_some() {
        births.extend(facts.alias);
        for (role, target) in [("get", facts.get), ("set", facts.set)] {
            if let Some(target) = target {
                births.insert(target.clone());
                declaration.insert(role.into(), string(target));
            }
        }
        declaration.insert("attributes".into(), number(facts.attrs.unwrap_or(5)));
        "alias"
    } else {
        return Err(format!("{} needs an object birth or provider", facts.name));
    };
    if facts.kind.as_deref().is_some_and(|stated| stated != kind) {
        return Err(format!("{}: kind disagrees with its birth", facts.name));
    }
    births.remove(&facts.name);
    declaration.insert("kind".into(), string(kind));
    declaration.insert(
        "births".into(),
        Value::Array(births.into_iter().map(string).collect()),
    );
    Ok(Value::Object(declaration))
}

pub(super) fn public_name(name: &str, kind: &str) -> Result<String, String> {
    let (name, prefix) = if let Some(name) = name.strip_suffix(".<get>") {
        (name, "get ")
    } else if let Some(name) = name.strip_suffix(".<set>") {
        (name, "set ")
    } else {
        (name, "")
    };
    let key = if let Some((_, tail)) = name.rsplit_once("[\"") {
        let key = tail
            .strip_suffix("\"]")
            .ok_or("malformed symbol/key path")?;
        if let Some(symbol) = key.strip_prefix("@@") {
            format!("[Symbol.{symbol}]")
        } else {
            key.into()
        }
    } else {
        name.rsplit('.').next().unwrap().into()
    };
    if matches!(kind, "getter" | "setter") && prefix.is_empty() {
        return Err("accessor path needs .<get> or .<set>".into());
    }
    Ok(format!("{prefix}{key}"))
}

pub(super) fn callable(
    mut facts: Facts,
    info: &Info,
    constants: &BTreeMap<String, u64>,
    body: String,
    source_symbol: String,
    source: &str,
) -> Result<Value, String> {
    if facts.ansi_colors
        || facts.namespace_table.is_some()
        || facts.provider
        || facts.native_method
        || (facts.receiver.is_some() && !facts.bare_closure)
        || facts.get.is_some()
        || facts.set.is_some()
        || facts.attrs.is_some()
        || facts.value.is_some()
    {
        return Err("non-callable birth facts require realm_decl!".into());
    }
    if facts.alias.as_deref() == Some(&facts.name) {
        return Err("a callable alias must name a distinct binding".into());
    }
    let canonical = facts
        .alias
        .as_ref()
        .map_or(&facts.name, |alias| std::cmp::min(&facts.name, alias))
        .clone();
    let members = facts.members.take();
    let kind = facts.kind.as_deref().unwrap_or_else(|| {
        if facts.name.ends_with(".<get>") {
            "getter"
        } else if facts.name.ends_with(".<set>") {
            "setter"
        } else if facts.name.contains('.') || facts.name.contains('[') {
            "method"
        } else {
            "function"
        }
    });
    if !matches!(
        kind,
        "method" | "function" | "getter" | "setter" | "constructor"
    ) {
        return Err(format!("{kind} cannot annotate fn_info!"));
    }
    if (facts.instance_attrs.is_some() || facts.extends.is_some()) && kind != "constructor" {
        return Err("prototype attributes require a constructor birth".into());
    }
    if (facts.spec_name.is_some() || facts.spec_length.is_some()) && !facts.emit_metadata {
        return Err(
            "public metadata overrides must be consumed by the runtime birth (emit = metadata)"
                .into(),
        );
    }
    if facts.builtin_constructor && (kind != "constructor" || facts.capture_key) {
        return Err("builtin constructor provider requires constructor kind".into());
    }
    if facts.bare_closure
        && (facts.constructible
            || facts.capture_key
            || facts.builtin_constructor
            || !matches!(
                (facts.receiver.as_deref(), facts.capture_enum.as_deref()),
                (Some("undefined"), None) | (None, Some(_)) | (None, None)
            ))
    {
        return Err("bare closure birth requires one symbolic capture binding".into());
    }
    if facts.capture_number.is_some() && (facts.bare_closure || facts.builtin_constructor) {
        return Err("numeric constant capture needs an ordinary metadata birth".into());
    }
    if facts.capture_enum.is_some() && !facts.bare_closure {
        return Err("enum property captures require their bare closure birth".into());
    }
    let default_length = if facts.bare_closure {
        info.bare_length(constants)?
    } else {
        info.public_length(constants)?
    };
    let default_name = if facts.bare_closure {
        String::new()
    } else {
        public_name(&facts.name, kind)?
    };
    if facts.spec_length == Some(default_length)
        || facts.spec_name.as_deref() == Some(default_name.as_str())
    {
        return Err("default public metadata is derived; omit the redundant override".into());
    }
    let length = facts.spec_length.unwrap_or(default_length);
    let function_info = info.facts(source_symbol, false);
    let info_symbol = hash::info_symbol(&body, &function_info);
    let captured_key = if facts.capture_public_name {
        Some(
            facts
                .spec_name
                .clone()
                .ok_or("public-name capture needs its projected name")?,
        )
    } else if facts.capture_key {
        Some(
            default_name
                .strip_prefix("get ")
                .filter(|_| kind == "getter")
                .or_else(|| {
                    default_name
                        .strip_prefix("set ")
                        .filter(|_| kind == "setter")
                })
                .unwrap_or(&default_name)
                .to_owned(),
        )
    } else {
        None
    };
    let label = facts.spec_name.clone().unwrap_or(default_name);
    let property = |key: &str, value: Value| {
        object([
            ("key", string(key)),
            ("symbol", Value::Bool(false)),
            ("attributes", number(if facts.frozen { 0 } else { 4 })),
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
            object([("kind", string("string")), ("value", string(label))]),
        ),
    ];
    let non_constructor = *constants
        .get("FN_NON_CONSTRUCTOR")
        .ok_or("missing FN_NON_CONSTRUCTOR")?;
    if facts.constructible && (kind == "constructor" || info.flags & non_constructor != 0) {
        return Err("constructible birth is redundant or contradicts its ABI".into());
    }
    let bound_captures = facts
        .capture_bindings
        .as_ref()
        .map(|bindings| {
            let key = public_name(&facts.name, kind)?;
            let key = key
                .strip_prefix("get ")
                .or_else(|| key.strip_prefix("set "))
                .unwrap_or(&key);
            super::capture::bindings(source, key, bindings, constants["TAG_UNDEFINED"])
        })
        .transpose()?;
    if bound_captures.is_some()
        && (facts.receiver.is_some() || facts.capture_enum.is_some() || facts.builtin_constructor)
    {
        return Err("symbolic capture list conflicts with another capture provider".into());
    }
    let pointer_capture = facts
        .capture_bindings
        .as_ref()
        .is_some_and(|bindings| bindings.iter().any(|b| b.starts_with("pointer_constant(")));
    let scalar_captures = !pointer_capture
        && bound_captures.as_ref().is_some_and(|captures| {
            captures
                .iter()
                .all(|c| c.get("kind").and_then(Value::string) == Some("raw_word"))
        });
    let reserved = if facts.allocated_closure
        || (facts.bare_closure && scalar_captures)
        || (facts.bare_closure && (facts.receiver.is_some() || facts.capture_enum.is_some()))
    {
        *constants
            .get("GC_LAYOUT_POINTER_FREE")
            .ok_or("missing bare closure layout")?
    } else if facts.bare_closure
        || facts.constructible
        || kind == "constructor"
        || info.flags & non_constructor != 0
    {
        0
    } else {
        128
    };
    let reserved = if facts.frozen {
        let gc = info::gc_constants()?;
        reserved | gc["OBJ_FLAG_FROZEN"] | gc["OBJ_FLAG_SEALED"] | gc["OBJ_FLAG_NO_EXTEND"]
    } else {
        reserved
    };
    let prototype = facts
        .extends
        .clone()
        .unwrap_or_else(|| "Function.prototype".into());
    let instance = facts
        .instance
        .clone()
        .unwrap_or_else(|| format!("{}.prototype", facts.name));
    if facts.births.contains(&prototype)
        || (kind == "constructor" && facts.births.contains(&instance))
    {
        return Err(format!("{}: omit derived callable birth edges", facts.name));
    }
    let mut births = facts.births.into_iter().collect::<BTreeSet<_>>();
    births.insert(prototype.clone());
    births.remove(&facts.name);
    let mut keys = vec![string("length"), string("name")];
    if kind == "constructor" {
        let instance = facts
            .instance
            .clone()
            .unwrap_or_else(|| format!("{}.prototype", facts.name));
        births.insert(instance.clone());
        properties.push(object([
            ("key", string("prototype")),
            ("symbol", Value::Bool(false)),
            ("attributes", number(facts.instance_attrs.unwrap_or(0))),
            (
                "value",
                object([("kind", string("reference")), ("value", string(instance))]),
            ),
        ]));
        keys.push(string("prototype"));
    }
    if facts.constructible
        || (facts.bare_closure && kind != "constructor" && info.flags & non_constructor == 0)
    {
        keys.push(string("prototype"));
    }
    let constructor = if kind == "constructor" {
        let families = super::families::read(include_str!(
            "../../perry-runtime/src/object/global_this_tables.rs"
        ))?;
        families
            .get(facts.family.as_deref().unwrap_or(&facts.name))
            .cloned()
    } else {
        None
    };
    let (captures, capture_flags) = if let Some(captures) = bound_captures {
        (captures, 0)
    } else if facts.builtin_constructor {
        let family = constructor
            .as_ref()
            .ok_or("builtin constructor has no family declaration")?;
        let Some(Value::Number(identity)) = family.get("identity") else {
            return Err("constructor family has no identity".into());
        };
        let identity = identity.parse::<u64>().map_err(|e| e.to_string())?;
        let shift = constants
            .get("CLOSURE_CONSTRUCT_SHIFT")
            .ok_or("missing constructor identity shift")?;
        let no_rebind = constants
            .get("CLOSURE_NO_THIS_REBIND_FLAG")
            .ok_or("missing constructor birth flag")?;
        (
            vec![object([
                ("kind", string("int32")),
                ("value", number(identity)),
            ])],
            (identity << shift) | no_rebind,
        )
    } else if facts.bare_closure && facts.receiver.is_none() && facts.capture_enum.is_none() {
        (Vec::new(), 0)
    } else if facts.bare_closure {
        let receiver = if let Some(name) = &facts.capture_enum {
            let key = public_name(&facts.name, kind)?;
            let key = key
                .strip_prefix("get ")
                .or_else(|| key.strip_prefix("set "))
                .unwrap_or(&key);
            super::capture::enum_property(source, name, key)?
        } else {
            *constants
                .get("TAG_UNDEFINED")
                .ok_or("missing undefined value ABI")?
        };
        (
            vec![object([
                ("kind", string("raw_word")),
                ("value", string(format!("{receiver:#x}"))),
            ])],
            0,
        )
    } else if let Some(binding) = facts.capture_number {
        let bits = super::capture::number_constant(source, &binding)?;
        (
            vec![object([
                ("kind", string("raw_word")),
                ("value", string(format!("{bits:#x}"))),
            ])],
            0,
        )
    } else if let Some(key) = captured_key {
        (
            vec![
                object([("kind", string("bytes")), ("value", string(key.clone()))]),
                object([
                    ("kind", string("raw_word")),
                    ("value", string(format!("0x{:x}", key.len()))),
                ]),
            ],
            0,
        )
    } else {
        (Vec::new(), 0)
    };
    for capture in &captures {
        if matches!(
            capture.get("kind").and_then(Value::string),
            Some("reference" | "raw_reference")
        ) {
            births.insert(
                capture
                    .get("value")
                    .and_then(Value::string)
                    .ok_or("missing capture target")?
                    .to_owned(),
            );
        }
    }
    let mut declaration = BTreeMap::from([
        ("name".into(), string(canonical)),
        ("kind".into(), string(kind)),
        ("properties".into(), Value::Array(properties)),
        ("own_key_order".into(), Value::Array(keys)),
        ("own_symbol_order".into(), Value::Array(Vec::new())),
        (
            "header".into(),
            object([
                ("gc_type", number(4)),
                ("reserved", number(reserved)),
                ("capture_flags", number(capture_flags)),
            ]),
        ),
        ("prototype".into(), string(prototype)),
        ("length".into(), number(length)),
        ("body".into(), string(body)),
        ("function_info".into(), function_info),
        ("captures".into(), Value::Array(captures)),
        ("info_symbol".into(), string(info_symbol)),
        (
            "births".into(),
            Value::Array(births.into_iter().map(string).collect()),
        ),
    ]);
    if let Some(constructor) = constructor {
        declaration.insert("constructor".into(), constructor);
    }
    if kind == "constructor" {
        declaration.insert(
            "construct".into(),
            string("crate::object::js_new_function_construct"),
        );
    }
    super::birth::extend_callable(Value::Object(declaration), members)
}

#[cfg(test)]
mod tests {
    use super::super::tokens;
    use super::*;
    #[test]
    fn integrity_is_only_frozen() {
        assert!(parse(&tokens("Example, integrity = sealed").unwrap()).is_err());
        let facts = parse(&tokens("Example, integrity = frozen").unwrap()).unwrap();
        assert!(facts.frozen);
    }
    #[test]
    fn human_paths_symbols_and_births_are_structured() {
        let facts =
            parse(&tokens("Array.prototype[Symbol.iterator], births = [Array, Object]").unwrap())
                .unwrap();
        assert_eq!(facts.name, "Array.prototype[\"@@iterator\"]");
        assert_eq!(facts.births, ["Array", "Object"]);
    }
    #[test]
    fn reflected_encodings_and_provenance_are_not_source_facts() {
        for field in [
            "source",
            "line",
            "header",
            "number_bits",
            "info_symbol",
            "function_info",
            "capture_flags",
            "own_key_order",
            "own_symbol_order",
            "flags",
            "length",
            "name",
            "prototype",
        ] {
            assert!(
                parse(&tokens(&format!("Atomics.add, {field}=1")).unwrap()).is_err(),
                "{field}"
            );
        }
        assert!(parse(&tokens("Atomics.add, typo=1").unwrap()).is_err());
        assert!(parse(&tokens("Atomics.add, births=[], births=[]").unwrap()).is_err());
    }
}
