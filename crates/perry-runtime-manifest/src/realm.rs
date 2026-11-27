//! Build-time realm facts. This module contains no runtime symbols or addresses.
use serde::{Deserialize, Serialize};

#[path = "realm_layout_facts.rs"]
mod realm_layout_facts;
pub use realm_layout_facts::*;
#[path = "realm_effect.rs"]
mod realm_effect;
pub use realm_effect::*;

pub use perry_runtime_manifest::realm_source::REALM_MANIFEST_VERSION;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RealmKind {
    Constructor,
    Prototype,
    Namespace,
    Exotic,
    Function,
    Method,
    Getter,
    Setter,
    Constant,
    Alias,
    /// A birth the runtime makes once per call (an arguments object), never
    /// a realm singleton: reach edges to the layouts it births only, no cell.
    BirthSite,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum RealmValue {
    Undefined,
    Null,
    Boolean(bool),
    NumberBits(String),
    Int32(i32),
    String(String),
    Symbol(String),
    Reference(String),
    /// Untagged pointer capture (for example a native global agent receiver).
    RawReference(String),
    Bytes(String),
    RawWord(String),
    Dynamic(String),
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RealmProperty {
    /// Ordinary spelling, or `@@iterator` / `@@toStringTag`, etc.
    pub key: String,
    pub symbol: bool,
    /// writable | enumerable << 1 | configurable << 2.
    pub attributes: u8,
    #[serde(default)]
    pub value: Option<RealmValue>,
    #[serde(default)]
    pub get: Option<String>,
    #[serde(default)]
    pub set: Option<String>,
    /// The one primitive type a runtime-computed value always has
    /// (`runtime(process.pid, number)`): `string`, `number` or `boolean`.
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub value_type: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FunctionInfoFacts {
    pub params: u16,
    pub rest_fixed: u16,
    pub flags: u32,
    pub length: u32,
    pub declared: u16,
    pub plain_params: u16,
    /// Original source locator, not a link dependency of the compiler.
    pub source_symbol: String,
    /// Bound native methods carry the existing dispatch sentinel rather than
    /// a callable code symbol. Their captures name the namespace and member.
    pub native_method: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RealmHeaderFacts {
    pub gc_type: u8,
    pub reserved: u16,
    #[serde(default)]
    pub class_id: Option<u32>,
    #[serde(default)]
    pub capture_flags: u32,
}

/// Constructor-family facts currently consumed by global_this_tables. Integer
/// references remain source expressions: they name runtime ABI constants, not
/// compiler copies of a process address or a sampled object-layout identity.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ConstructorFacts {
    pub identity: u8,
    #[serde(default)]
    pub prototype_class: Option<String>,
    #[serde(default)]
    pub prototype_serial: Option<String>,
    #[serde(default)]
    pub sealed_serial: Option<String>,
    #[serde(default)]
    pub named_serial: Option<String>,
    pub prototype_callable: bool,
    #[serde(default)]
    pub instance_identity: Option<String>,
}

/// Internal storage that reflection cannot describe. The source producer
/// resolves these values from the owning birth's declarations, never a dump.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RealmStorageFacts {
    Set {
        entries: Vec<RealmValue>,
    },
    /// The dense elements of a static array birth, in index order. Reflection
    /// reads them as index properties; the array holder carries only
    /// and its named properties, so the elements have one home.
    Array {
        elements: Vec<RealmValue>,
    },
    RuntimeLayout(RuntimeLayoutFacts),
}

impl RealmStorageFacts {
    pub fn values(&self) -> &[RealmValue] {
        match self {
            Self::Set { entries } => entries,
            Self::Array { elements } => elements,
            Self::RuntimeLayout(_) => &[],
        }
    }
}

/// A declaration belongs to the source that owns the body or object birth.
/// Names are symbolic paths, never addresses. Symbol keys use `@@iterator`
/// etc.; accessors declare their two independently callable entries.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RealmDeclaration {
    pub name: String,
    pub kind: RealmKind,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub info_symbol: Option<String>,
    #[serde(default)]
    pub construct: Option<String>,
    #[serde(default)]
    pub prototype: Option<String>,
    #[serde(default)]
    pub length: Option<u32>,
    #[serde(default)]
    pub flags: Vec<String>,
    #[serde(default)]
    pub attributes: Option<u8>,
    #[serde(default)]
    pub get: Option<String>,
    #[serde(default)]
    pub set: Option<String>,
    #[serde(default)]
    pub value: Option<RealmValue>,
    #[serde(default)]
    pub exotic_vtable: Option<String>,
    #[serde(default)]
    pub births: Vec<String>,
    #[serde(default)]
    pub module: Option<String>,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub line: usize,
    #[serde(default)]
    pub properties: Vec<RealmProperty>,
    #[serde(default)]
    pub own_key_order: Vec<String>,
    #[serde(default)]
    pub own_symbol_order: Vec<String>,
    #[serde(default)]
    pub function_info: Option<FunctionInfoFacts>,
    #[serde(default)]
    pub captures: Vec<RealmValue>,
    #[serde(default)]
    pub header: Option<RealmHeaderFacts>,
    /// What a dynamic host array declares its elements to be
    /// (`value = dynamic, elements = string`): the runtime puts only such
    /// values in the array it makes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elements: Option<String>,
    /// What an exotic host object declares every own value to be
    /// (`process.env, kind = exotic, …, values = string`): its hooks read and
    /// store only such values (a [[Set]] stores ToString(v)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub values: Option<String>,
    #[serde(default)]
    pub native_namespace: Option<String>,
    #[serde(default)]
    pub constructor: Option<ConstructorFacts>,
    #[serde(default)]
    pub storage: Option<RealmStorageFacts>,
    /// What the body does to its inputs; empty = undeclared (unknown code).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<RealmEffect>,
    /// The body `effects` describe when it is not `body`: the function a
    /// `provider = bound_method` member's native_module_dispatch arm calls
    /// (source symbol, from the effects-only `realm_decl!` above it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effects_body: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub throws: Vec<RealmThrowOperand>,
    /// `this = ignored` at the definition: the body never reads its `this`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub this_ignored: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RealmThrowOperand {
    pub source: Option<String>,
    pub argument: u16,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RealmThrowDeclaration {
    pub symbol: String,
    pub source: String,
    pub line: usize,
    pub operands: Vec<RealmThrowOperand>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RealmManifest {
    pub version: u32,
    pub declarations: Vec<RealmDeclaration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub throw_declarations: Vec<RealmThrowDeclaration>,
}

/// Borrowed projection in the generated compiler table. `facts` preserves
/// every declaration field for emission; reachability needs only the edges.
#[derive(Clone, Copy, Debug)]
pub struct RealmEntry {
    pub name: &'static str,
    pub kind: RealmKind,
    pub births: &'static [&'static str],
    pub module: Option<&'static str>,
    pub facts: &'static str,
    /// The declared effects of the body (empty: undeclared, unknown code).
    pub effects: &'static [RealmEffect],
    /// Its own properties (`RealmDeclaration::properties`), key and value
    /// kind, so a reader of the table needs no JSON.
    pub own: &'static [RealmOwn],
    /// Its [[Prototype]] (`RealmDeclaration::prototype`): the declaration a
    /// lookup past `own` continues on; `None` ends the chain (`null`).
    pub proto: Option<&'static str>,
    /// The value a constant declaration holds (`RealmDeclaration::value`,
    /// `%WorkerGlobal%.onmessage`: `null`), its kind as an own property's;
    /// `None` for a declaration without one.
    pub value: Option<RealmOwnValue>,
}

/// An own property of a declaration in the generated compiler table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RealmOwn {
    /// Ordinary spelling, or `@@species` for a symbol key.
    pub key: &'static str,
    pub symbol: bool,
    /// Its [[Enumerable]] attribute (`RealmProperty::attributes` bit 1):
    /// a copy of own enumerable properties (`Object.assign`, `{...o}`,
    /// 7.3.25 CopyDataProperties) reads it.
    pub enumerable: bool,
    pub value: RealmOwnValue,
}

/// What an own property holds (`RealmProperty`): a primitive of a kind,
/// another declaration, or an accessor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RealmOwnValue {
    Undefined,
    Null,
    Boolean,
    Number,
    String,
    Symbol,
    /// The declaration it holds (`Date.now`, `Date.prototype`).
    Reference(&'static str),
    /// Its getter and setter declarations.
    Accessor {
        get: Option<&'static str>,
        set: Option<&'static str>,
    },
    /// A value the runtime computes or a raw word.
    Other,
}

impl RealmValue {
    /// The `RealmOwnValue` variant (const Rust, without its path) of a
    /// property or a constant declaration holding this value.
    pub fn emit_own_value(&self) -> String {
        match self {
            RealmValue::Undefined => "Undefined".into(),
            RealmValue::Null => "Null".into(),
            RealmValue::Boolean(_) => "Boolean".into(),
            RealmValue::NumberBits(_) => "Number".into(),
            RealmValue::String(_) => "String".into(),
            RealmValue::Symbol(_) => "Symbol".into(),
            RealmValue::Reference(r) => format!("Reference({r:?})"),
            // A constructor identity capture (`realm_facts.rs`): a
            // runtime-internal 0x7FFE word, never a JS Number (P3 B4a).
            RealmValue::Int32(_)
            | RealmValue::RawReference(_)
            | RealmValue::Bytes(_)
            | RealmValue::RawWord(_)
            | RealmValue::Dynamic(_) => "Other".into(),
        }
    }
}

impl RealmProperty {
    /// Const Rust for this property's `RealmOwn` in the generated table;
    /// `p` is the path prefix of the schema module.
    pub fn emit_own(&self, p: &str) -> String {
        let value = match (&self.get, &self.set, &self.value) {
            (None, None, Some(v)) => v.emit_own_value(),
            (None, None, None) => "Undefined".into(),
            (get, set, _) => format!("Accessor {{ get: {get:?}, set: {set:?} }}"),
        };
        format!(
            "{p}RealmOwn {{ key: {:?}, symbol: {}, enumerable: {}, value: {p}RealmOwnValue::{value} }}",
            self.key,
            self.symbol,
            self.attributes & 2 != 0
        )
    }
}

impl RealmManifest {
    pub fn validate(&self) -> Result<(), String> {
        use std::collections::BTreeSet;
        if self.version != REALM_MANIFEST_VERSION {
            return Err(format!(
                "unsupported realm manifest version {}",
                self.version
            ));
        }
        let mut names = BTreeSet::new();
        for d in &self.declarations {
            if d.name.is_empty() || !names.insert(d.name.as_str()) {
                return Err(format!("duplicate/empty realm declaration: {}", d.name));
            }
            if let Some(RealmStorageFacts::RuntimeLayout(layout)) = &d.storage {
                layout.validate()?;
                if d.header.is_some() || !d.properties.is_empty() || d.value.is_some() {
                    return Err(format!("runtime layout {} is not a realm receiver", d.name));
                }
            }
            if d.attributes.is_some_and(|a| a & !7 != 0) {
                return Err(format!("invalid property attributes: {}", d.name));
            }
            if d.constructor
                .as_ref()
                .is_some_and(|c| c.identity == 0 || c.identity >= 128)
            {
                return Err(format!("invalid constructor identity: {}", d.name));
            }
            for value in &d.captures {
                if let RealmValue::RawWord(bits) = value {
                    let word = u64::from_str_radix(bits.trim_start_matches("0x"), 16)
                        .map_err(|_| format!("invalid raw capture on {}", d.name))?;
                    if (0x1_0000_0000..=0xffff_ffff_ffff).contains(&word) {
                        return Err(format!("unrelocated capture address on {}", d.name));
                    }
                }
            }
            if matches!(d.value, Some(RealmValue::Dynamic(_)))
                && (!d.properties.is_empty()
                    || !d.own_key_order.is_empty()
                    || !d.own_symbol_order.is_empty())
            {
                return Err(format!(
                    "dynamic provider has sampled storage on {}",
                    d.name
                ));
            }
            let mut keys = BTreeSet::new();
            for property in &d.properties {
                if !keys.insert((&property.key, property.symbol)) || property.attributes & !7 != 0 {
                    return Err(format!("invalid/duplicate property on {}", d.name));
                }
                if property.value.is_some() && (property.get.is_some() || property.set.is_some()) {
                    return Err(format!("mixed data/accessor property on {}", d.name));
                }
            }
        }
        for d in &self.declarations {
            if !d.effects.is_empty() {
                if d.body.is_none() {
                    return Err(format!(
                        "{}: effects on a declaration without a body",
                        d.name
                    ));
                }
                validate_effects(&d.name, &d.effects, &|name| names.contains(name))?;
            }
            if let Some(RealmStorageFacts::RuntimeLayout(layout)) = &d.storage {
                for owner in &layout.owners {
                    let birth = self
                        .declarations
                        .iter()
                        .find(|entry| &entry.name == owner)
                        .ok_or_else(|| {
                            format!("{} names undeclared layout owner {owner}", d.name)
                        })?;
                    if !birth.births.contains(&d.name) {
                        return Err(format!(
                            "layout owner {owner} lacks birth edge to {}",
                            d.name
                        ));
                    }
                }
            }
            let references = d.properties.iter().flat_map(|p| {
                p.get.iter().chain(p.set.iter()).chain(match &p.value {
                    Some(RealmValue::Reference(name)) => Some(name),
                    _ => None,
                })
            });
            let captures = d.captures.iter().filter_map(|value| match value {
                RealmValue::Reference(name) | RealmValue::RawReference(name) => Some(name),
                _ => None,
            });
            for edge in d
                .births
                .iter()
                .chain(d.owner.iter())
                .chain(d.prototype.iter())
                .chain(references)
                .chain(captures)
                .chain(
                    d.storage
                        .iter()
                        .flat_map(|storage| storage.values())
                        .filter_map(|value| match value {
                            RealmValue::Reference(name) | RealmValue::RawReference(name) => {
                                Some(name)
                            }
                            _ => None,
                        }),
                )
            {
                if !names.contains(edge.as_str()) {
                    return Err(format!(
                        "{} references undeclared realm node {edge}",
                        d.name
                    ));
                }
            }
        }
        self.validate_namespace_values()
    }

    /// Every own member of a host namespace object (`%Global%`, `process`,
    /// `node:<m>` and their sub-namespaces) has a value side: a data value
    /// (a runtime-computed one names its primitive type, or the exotic
    /// declaration of the one host object it is declares what that object
    /// holds), or an accessor whose getter declares what it returns, with
    /// no `open` effect and no `unknown` operand. A read of such a member is
    /// then the declared value, never an unknown one.
    fn validate_namespace_values(&self) -> Result<(), String> {
        use self::{RealmEffect as E, RealmOperand as O};
        fn unknown(o: &O) -> bool {
            match o {
                O::Unknown => true,
                O::Fresh {
                    elems,
                    fields,
                    slots,
                    ..
                } => {
                    elems.iter().any(unknown)
                        || fields.iter().chain(slots.iter()).any(|f| unknown(&f.value))
                }
                O::Promise { value, reject } => value.iter().chain(reject.iter()).any(unknown),
                O::Species { args, .. } => args.iter().any(unknown),
                O::Union(members) => members.iter().any(unknown),
                O::Prim(_)
                | O::Slot(_)
                | O::Native(_)
                | O::Namespace(_)
                | O::Bound
                | O::Json
                | O::Never => {
                    false
                }
            }
        }
        let decl = |name: &str| self.declarations.iter().find(|d| d.name == name);
        for d in self
            .declarations
            .iter()
            .filter(|d| d.kind == RealmKind::Namespace)
        {
            for p in &d.properties {
                let member = format!("{}.{}", d.name, p.key);
                match (&p.value, &p.get) {
                    (Some(RealmValue::Dynamic(provider)), _) if p.value_type.is_none() => {
                        let typed = self.declarations.iter().any(|h| {
                            h.kind == RealmKind::Exotic
                                && h.value.as_ref() == p.value.as_ref()
                                && (h.elements.is_some() || h.values.is_some())
                        });
                        if !typed {
                            return Err(format!(
                                "namespace member {member}: runtime value {provider} declares no type \
                                 (`runtime(path, kind)`, or an exotic declaration with `elements`/`values`)"
                            ));
                        }
                    }
                    (Some(_), _) => {}
                    (None, Some(get)) => {
                        let Some(g) = decl(get) else {
                            return Err(format!("namespace member {member}: undeclared getter {get}"));
                        };
                        let returns = g.effects.iter().any(|e| matches!(e, E::Returns(_)));
                        let open = g.effects.iter().any(|e| matches!(e, E::Open));
                        let unknown_operand = g.effects.iter().any(|e| match e {
                            E::Returns(o) => unknown(o),
                            E::Calls { this, args, .. } => unknown(this) || args.iter().any(unknown),
                            _ => false,
                        });
                        if !returns || open || unknown_operand {
                            return Err(format!(
                                "namespace member {member}: getter {get} ({}:{}) must declare \
                                 `returns(..)` with no `open` effect and no `unknown` operand",
                                g.source, g.line
                            ));
                        }
                    }
                    (None, None) => {
                        return Err(format!("namespace member {member} has no value and no getter"));
                    }
                }
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod schema_tests {
    use super::*;

    fn manifest(declarations: &str) -> RealmManifest {
        serde_json::from_str(&format!(r#"{{"version":1,"declarations":{declarations}}}"#)).unwrap()
    }

    #[test]
    fn dynamic_providers_cannot_freeze_sampled_storage() {
        let empty = r#"[{"name":"argv","kind":"exotic","value":{"kind":"dynamic","value":"process.argv"}}]"#;
        assert!(manifest(empty).validate().is_ok());
        let site = r#"[{"name":"%ExampleBirth%","kind":"birth_site","births":[]}]"#;
        assert!(manifest(site).validate().is_ok());
        let sampled = r#"[{"name":"argv","kind":"exotic","value":{"kind":"dynamic","value":"process.argv"},"properties":[{"key":"0","symbol":false,"attributes":7,"value":{"kind":"string","value":"/qualification-machine/app"}}]}]"#;
        assert!(manifest(sampled).validate().is_err());
    }

    #[test]
    fn unknown_fields_versions_and_graph_drift_fail_closed() {
        assert!(serde_json::from_str::<RealmDeclaration>(
            r#"{"name":"X","kind":"namespace","typo":1}"#
        )
        .is_err());
        assert!(
            manifest(r#"[{"name":"X","kind":"namespace"},{"name":"X","kind":"namespace"}]"#)
                .validate()
                .is_err()
        );
        assert!(
            manifest(r#"[{"name":"X","kind":"namespace","births":["missing"]}]"#)
                .validate()
                .is_err()
        );
        assert!(
            manifest(r#"[{"name":"X","kind":"namespace","attributes":8}]"#)
                .validate()
                .is_err()
        );
        let mut m = manifest(r#"[{"name":"X","kind":"namespace"}]"#);
        assert!(m.validate().is_ok());
        m.version += 1;
        assert!(m.validate().is_err());
    }

    #[test]
    fn captures_need_symbolic_relocations_and_properties_have_one_kind() {
        assert!(manifest(r#"[{"name":"X","kind":"function","captures":[{"kind":"raw_word","value":"0x123456789ab"}]}]"#).validate().is_err());
        assert!(manifest(r#"[{"name":"X","kind":"function","captures":[{"kind":"raw_reference","value":"missing"}]}]"#).validate().is_err());
        assert!(manifest(r#"[{"name":"X","kind":"namespace","properties":[{"key":"a","symbol":false,"attributes":7,"value":{"kind":"undefined"},"get":"X"}]}]"#).validate().is_err());
    }
}
