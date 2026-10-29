//! Realm values name the same image strings as literal and descriptor keys.
//! This map is compiler state only; the binary contains the cells themselves.
use std::collections::{BTreeMap, BTreeSet};

use perry_hir::realm_manifest::{RealmDeclaration, RealmStorageFacts, RealmValue};

use super::*;
use crate::realm_plan::RealmPlan;

pub(crate) struct RealmAtoms {
    strings: BTreeMap<String, String>,
    key_records: BTreeMap<String, String>,
    symbols: BTreeMap<String, (String, u64)>,
}

impl RealmAtoms {
    /// Emit exactly this realm's text and symbol cells. Symbol ids are assigned
    /// in the full manifest's order, so an R restriction does not renumber an
    /// identity. No address, sampled counter, or runtime lookup is serialized.
    pub(crate) fn emit(module: &mut LlModule, plan: &RealmPlan) -> Result<Self, String> {
        let identities = symbol_identities()?;
        let mut pool = crate::strings::StringPool::new();
        let mut result = Self {
            strings: BTreeMap::new(),
            key_records: BTreeMap::new(),
            symbols: BTreeMap::new(),
        };
        let mut texts = plan.atoms.clone();
        let mut key_atoms = plan.key_atoms.clone();
        let mut symbols = plan.symbols.clone();
        for layout in plan.runtime_layouts.values() {
            for key in &layout.for_target(&module.target_triple)?.keys {
                match key.atom.realm_value()? {
                    RealmValue::String(text) => {
                        texts.insert(text.clone());
                        key_atoms.insert(text);
                    }
                    RealmValue::Symbol(key) => {
                        symbols.insert(key);
                    }
                    _ => unreachable!("a layout key is a string or a symbol"),
                }
            }
        }
        for symbol in &symbols {
            texts.insert(symbol_description(symbol)?.0);
        }
        for text in texts {
            let index = pool.intern(&text);
            // Key atom records and their strings share the ordinary emission
            // mechanism. Value-only strings need no extra lookup structure.
            if key_atoms.contains(&text) {
                crate::layout_desc::atom(module, &mut pool, &mut result.key_records, &text)
                    .ok_or_else(|| format!("realm key cannot be an image atom: {text:?}"))?;
            }
            let entry = pool.entry(index);
            super::super::string_pool::emit_image_record(module, entry);
            result
                .strings
                .insert(text, super::super::string_pool::image_record_symbol(entry));
        }
        for symbol in &symbols {
            let (description, registered) = symbol_description(symbol)?;
            let id = *identities
                .get(symbol)
                .ok_or_else(|| format!("unknown realm symbol {symbol}"))?;
            let name = symbol_cell_name(symbol);
            emit_symbol_cell(module, &name, &result.strings[&description], id, registered);
            result.symbols.insert(symbol.clone(), (name, id));
        }
        Ok(result)
    }

    /// A direct image-symbol atom uses the ordinary descriptor record format.
    /// Its pointer is the symbol itself, so linking a list cannot allocate it.
    pub(crate) fn symbol_atom(
        &self,
        module: &mut LlModule,
        key: &str,
    ) -> Result<(String, u64), String> {
        let Slot::Cell(symbol, _) = self.symbol(key)? else {
            unreachable!()
        };
        let id = self.symbols[key].1;
        let name = format!(
            "perry_atom_realm_symbol_{}",
            key.bytes().map(|b| format!("{b:02x}")).collect::<String>()
        );
        // Several layouts can use the same symbol. Its definition belongs
        // to this module once, including the COMDAT declaration on ELF/COFF.
        let definition = format!("@{name} = ");
        if module
            .global_definition_lines()
            .any(|line| line.starts_with(&definition))
        {
            return Ok((name, id));
        }
        let (section, storage) = if module.target_triple.contains("apple") {
            ("__DATA,__perry_atoms", "global")
        } else if module.target_triple.contains("windows") {
            (".perryatm$m", "constant")
        } else {
            ("perry_atoms", "global")
        };
        module.add_coalesced_global(&name, format!(
            "@{name} = weak_odr {storage} {} {{ i64 {}, ptr {}, i8 {}, [3 x i8] zeroinitializer, i32 0 }}, section \"{section}\", align 8",
            crate::layout_desc::ATOM_TY, crate::nanbox::i64_literal(id), user_address(&symbol), perry_abi::ATOM_KIND_IMAGE_SYMBOL,
        ));
        Ok((name, id))
    }

    /// One key representation for receiver layouts and runtime-born layouts.
    /// The atom record and the list slot name the same emitted cell.
    pub(crate) fn layout_key(
        &self,
        module: &mut LlModule,
        value: &RealmValue,
        slot: u32,
        attrs: u8,
    ) -> Result<super::layouts::LayoutKey, String> {
        let (atom, hash, string_hash) = match value {
            RealmValue::String(text) => {
                let atom = self
                    .key_records
                    .get(text)
                    .ok_or_else(|| format!("realm key atom was not emitted: {text:?}"))?;
                let hash = crate::layout_desc::atom_hash(text);
                (atom.clone(), hash, Some(hash))
            }
            RealmValue::Symbol(symbol) => {
                let (atom, hash) = self.symbol_atom(module, symbol)?;
                (atom, hash, None)
            }
            _ => return Err("layout key must be a string or symbol".into()),
        };
        Ok(super::layouts::LayoutKey {
            atom,
            hash,
            image: ImageKeySlot {
                value: self.value(value)?,
                string_hash,
                attrs,
                slot,
            },
        })
    }

    pub(crate) fn string(&self, text: &str) -> Result<Slot, String> {
        self.strings
            .get(text)
            .cloned()
            .map(Slot::ImageString)
            .ok_or_else(|| format!("realm string was not declared: {text:?}"))
    }

    pub(crate) fn symbol(&self, key: &str) -> Result<Slot, String> {
        self.symbols
            .get(key)
            .cloned()
            .map(|(name, _)| Slot::Cell(name, POINTER_TAG))
            .ok_or_else(|| format!("realm symbol was not declared: {key}"))
    }

    pub(crate) fn value(&self, value: &RealmValue) -> Result<Slot, String> {
        use crate::nanbox;
        Ok(match value {
            RealmValue::Undefined => Slot::Bits(nanbox::TAG_UNDEFINED),
            RealmValue::Null => Slot::Bits(nanbox::TAG_NULL),
            RealmValue::Boolean(v) => Slot::Bits(if *v {
                nanbox::TAG_TRUE
            } else {
                nanbox::TAG_FALSE
            }),
            RealmValue::NumberBits(bits) => Slot::Bits(parse_word(bits)?),
            RealmValue::Int32(v) => Slot::Bits(nanbox::INT32_TAG | u64::from(*v as u32)),
            RealmValue::String(text) => self.string(text)?,
            RealmValue::Symbol(key) => self.symbol(key)?,
            RealmValue::Reference(name) => {
                Slot::Cell(crate::realm_plan::cell_symbol(name), POINTER_TAG)
            }
            RealmValue::RawReference(name) => Slot::Cell(crate::realm_plan::cell_symbol(name), 0),
            RealmValue::Bytes(text) => {
                let symbol = self
                    .strings
                    .get(text)
                    .ok_or_else(|| format!("realm byte capture was not declared: {text:?}"))?;
                Slot::Address {
                    symbol: symbol.clone(),
                    offset: crate::target_layout::GC_HEADER_SIZE_BYTES + 20,
                    tag: 0,
                }
            }
            RealmValue::RawWord(bits) => Slot::Bits(parse_word(bits)?),
            RealmValue::Dynamic(provider) => {
                return Err(format!(
                    "live realm value requires its birth provider: {provider}"
                ))
            }
        })
    }
}

fn parse_word(bits: &str) -> Result<u64, String> {
    u64::from_str_radix(bits.strip_prefix("0x").unwrap_or(bits), 16)
        .map_err(|_| format!("invalid realm word {bits:?}"))
}

fn symbol_cell_name(key: &str) -> String {
    format!(
        "perry_realm_symbol_{}",
        key.bytes().map(|b| format!("{b:02x}")).collect::<String>()
    )
}

fn symbol_description(key: &str) -> Result<(String, bool), String> {
    if let Some(name) = key.strip_prefix("@@") {
        if !perry_abi::WELL_KNOWN_SYMBOLS.contains(&name) {
            return Err(format!("unknown well-known realm symbol {name}"));
        }
        return Ok((format!("Symbol.{name}"), false));
    }
    if let Some(name) = key.strip_prefix("@for:") {
        return Ok((name.into(), true));
    }
    if let Some(name) = key.strip_prefix("@runtime:") {
        return Ok((name.into(), false));
    }
    Err(format!("realm symbol lacks an identity declaration: {key}"))
}

fn symbol_identities() -> Result<BTreeMap<String, u64>, String> {
    let mut keys = BTreeSet::new();
    for entry in crate::realm_manifest::REALM {
        let declaration: RealmDeclaration =
            serde_json::from_str(entry.facts).map_err(|e| format!("realm {}: {e}", entry.name))?;
        if let Some(RealmStorageFacts::RuntimeLayout(layout)) = &declaration.storage {
            for variant in &layout.variants {
                for key in &variant.keys {
                    if let RealmValue::Symbol(key) = key.atom.realm_value()? {
                        keys.insert(key);
                    }
                }
            }
        }
        for property in declaration.properties {
            if property.symbol {
                keys.insert(property.key);
            }
            if let Some(RealmValue::Symbol(key)) = property.value {
                keys.insert(key);
            }
        }
        for value in declaration.captures.into_iter().chain(declaration.value) {
            if let RealmValue::Symbol(key) = value {
                keys.insert(key);
            }
        }
    }
    let mut result = BTreeMap::new();
    for name in perry_abi::WELL_KNOWN_SYMBOLS {
        let key = format!("@@{name}");
        keys.remove(&key);
        result.insert(
            key,
            perry_abi::symbol_identity(perry_abi::IMAGE_SYMBOL_ORDINAL_BASE + result.len() as u64),
        );
    }
    for key in keys {
        symbol_description(&key)?;
        result.insert(
            key,
            perry_abi::symbol_identity(perry_abi::IMAGE_SYMBOL_ORDINAL_BASE + result.len() as u64),
        );
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
