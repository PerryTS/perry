//! Build-time realm facts. This module contains no runtime symbols or addresses.
use serde::{Deserialize, Serialize};

#[path = "realm_layout_facts.rs"]
mod realm_layout_facts;
pub use realm_layout_facts::*;

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
    Set { entries: Vec<RealmValue> },
    RuntimeLayout(RuntimeLayoutFacts),
}

impl RealmStorageFacts {
    pub fn values(&self) -> &[RealmValue] {
        match self {
            Self::Set { entries } => entries,
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
    #[serde(default)]
    pub native_namespace: Option<String>,
    #[serde(default)]
    pub constructor: Option<ConstructorFacts>,
    #[serde(default)]
    pub storage: Option<RealmStorageFacts>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RealmManifest {
    pub version: u32,
    pub declarations: Vec<RealmDeclaration>,
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
