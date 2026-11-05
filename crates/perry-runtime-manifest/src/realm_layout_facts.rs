//! Target-dependent runtime descriptor facts carried by the realm manifest.
use perry_runtime_manifest::abi as perry_abi;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeLayoutTarget {
    Any,
    Windows,
    NonWindows,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct RuntimeLayoutAtom {
    pub kind: u8,
    pub text: Option<String>,
    pub index: u32,
    #[serde(default)]
    pub registered: Option<bool>,
}

impl RuntimeLayoutAtom {
    pub fn realm_value(&self) -> Result<super::RealmValue, String> {
        match self.kind {
            perry_abi::ATOM_KIND_STRING => self
                .text
                .clone()
                .map(super::RealmValue::String)
                .ok_or_else(|| "layout string atom lacks text".into()),
            perry_abi::ATOM_KIND_WELL_KNOWN_SYMBOL => perry_abi::WELL_KNOWN_SYMBOLS
                .get(self.index as usize)
                .map(|name| super::RealmValue::Symbol(format!("@@{name}")))
                .ok_or_else(|| "unknown well-known layout symbol".into()),
            perry_abi::ATOM_KIND_PROGRAM_SYMBOL => {
                let text = self
                    .text
                    .as_deref()
                    .ok_or("runtime symbol lacks a description")?;
                let prefix = if self
                    .registered
                    .ok_or("runtime symbol lacks registration facts")?
                {
                    "@for:"
                } else {
                    "@runtime:"
                };
                Ok(super::RealmValue::Symbol(format!("{prefix}{text}")))
            }
            _ => Err("unsupported runtime layout atom kind".into()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RuntimeLayoutKey {
    pub atom: RuntimeLayoutAtom,
    pub slot: u32,
    /// Existing layout-entry bits, including accessor/private/optional facts.
    pub attributes: u8,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuntimeLayoutPrototype {
    Object,
    Null,
    Serial { serial: u64 },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RuntimeLayoutVariant {
    pub target: RuntimeLayoutTarget,
    pub family: u32,
    pub variant: u16,
    pub object_kind: u8,
    pub integrity: u8,
    pub rep: u64,
    pub class_id: u32,
    pub parent_class_id: u32,
    pub prototype: RuntimeLayoutPrototype,
    pub keys: Vec<RuntimeLayoutKey>,
    pub source: String,
    pub line: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RuntimeLayoutFacts {
    pub symbol: String,
    /// Reached realm births that require this layout; compiler-only edges.
    #[serde(default)]
    pub owners: Vec<String>,
    pub variants: Vec<RuntimeLayoutVariant>,
}

impl RuntimeLayoutFacts {
    pub fn for_target(&self, triple: &str) -> Result<&RuntimeLayoutVariant, String> {
        let windows = triple.contains("windows");
        let mut variants = self.variants.iter().filter(|v| match v.target {
            RuntimeLayoutTarget::Any => true,
            RuntimeLayoutTarget::Windows => windows,
            RuntimeLayoutTarget::NonWindows => !windows,
        });
        let value = variants
            .next()
            .ok_or_else(|| format!("{} has no facts for {triple}", self.symbol))?;
        if variants.next().is_some() {
            return Err(format!("{} has overlapping target facts", self.symbol));
        }
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        use std::collections::BTreeSet;
        let mut targets = self.variants.iter().map(|v| v.target).collect::<Vec<_>>();
        targets.sort();
        if targets != [RuntimeLayoutTarget::Any]
            && targets
                != [
                    RuntimeLayoutTarget::Windows,
                    RuntimeLayoutTarget::NonWindows,
                ]
        {
            return Err(format!(
                "{} has incomplete or overlapping target facts",
                self.symbol
            ));
        }
        if !self.symbol.starts_with("perry_runtime_layout_")
            || !self
                .symbol
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err("invalid runtime layout export".into());
        }
        for variant in &self.variants {
            if variant.source.is_empty() || variant.line == 0 {
                return Err("runtime layout lacks declaration provenance".into());
            }
            if matches!(
                variant.prototype,
                RuntimeLayoutPrototype::Serial { serial: 0 }
            ) {
                return Err("runtime prototype lacks its serial".into());
            }
            let mut slots = BTreeSet::new();
            let mut keys = BTreeSet::new();
            for key in &variant.keys {
                if key.slot as usize >= variant.keys.len()
                    || !slots.insert(key.slot)
                    || !keys.insert(&key.atom)
                {
                    return Err(format!(
                        "{} has invalid or duplicate layout keys/slots",
                        self.symbol
                    ));
                }
                match key.atom.kind {
                    perry_abi::ATOM_KIND_STRING
                        if key.atom.text.is_some()
                            && key.atom.index == 0
                            && key.atom.registered.is_none() => {}
                    perry_abi::ATOM_KIND_WELL_KNOWN_SYMBOL
                        if key.atom.text.is_none()
                            && key.atom.registered.is_none()
                            && (key.atom.index as usize) < perry_abi::WELL_KNOWN_SYMBOLS.len() => {}
                    perry_abi::ATOM_KIND_PROGRAM_SYMBOL
                        if key.atom.text.is_some() && key.atom.registered.is_some() => {}
                    _ => return Err(format!("{} has invalid source atom facts", self.symbol)),
                }
            }
        }
        Ok(())
    }
}
