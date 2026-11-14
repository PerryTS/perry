//! Metadata bytes derived from the same birth declarations and ABI seal facts.
use super::families::Families;
use perry_hir::realm_manifest::{RealmDeclaration, RealmKind, RealmValue};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CellMeta {
    pub(crate) flags: u64,
    pub(crate) serial: u64,
}

pub(crate) struct Metadata {
    cells: BTreeMap<String, CellMeta>,
    bags: BTreeMap<String, CellMeta>,
    pub(crate) serial_end: u64,
}

fn seal(flags: &mut u64, family: usize) -> Result<(), String> {
    use perry_abi::object_meta_flags::*;
    if family as u64 > OBJECT_META_SEALED_FAMILY_MASK {
        return Err("sealed realm family exceeds its metadata field".into());
    }
    *flags |= OBJECT_META_FLAG_IS_PROTOTYPE
        | OBJECT_META_FLAG_SEALED_INTRINSIC
        | ((family as u64) << OBJECT_META_SEALED_FAMILY_SHIFT);
    Ok(())
}

fn prototype(declaration: &RealmDeclaration) -> Option<&str> {
    declaration.properties.iter().find_map(|property| {
        match (&property.value, property.symbol, property.key.as_str()) {
            (Some(RealmValue::Reference(name)), false, "prototype") => Some(name.as_str()),
            _ => None,
        }
    })
}

impl Metadata {
    pub(crate) fn new(
        plan: &crate::realm_plan::RealmPlan,
        declarations: &BTreeMap<String, RealmDeclaration>,
        families: &Families,
    ) -> Result<Self, String> {
        use perry_abi::object_meta_flags::*;
        let mut result = Self {
            cells: plan
                .cells
                .keys()
                .map(|name| (name.clone(), CellMeta::default()))
                .collect(),
            bags: plan
                .cells
                .iter()
                .filter(|(_, cell)| {
                    cell.header
                        .as_ref()
                        .is_some_and(|header| header.gc_type == 4)
                })
                .map(|(name, _)| (name.clone(), CellMeta::default()))
                .collect(),
            serial_end: perry_abi::intrinsic_serials::FIRST_DYNAMIC_PROTOTYPE_SERIAL,
        };
        let targets = plan
            .cells
            .values()
            .filter_map(|cell| cell.prototype.as_deref())
            .collect::<BTreeSet<_>>();
        for (name, meta) in &mut result.cells {
            let cell = &plan.cells[name];
            let object = cell
                .header
                .as_ref()
                .is_some_and(|header| header.gc_type != 4);
            if object && (cell.kind == RealmKind::Prototype || targets.contains(name.as_str())) {
                meta.flags |= OBJECT_META_FLAG_IS_PROTOTYPE;
            }
            if let Some(serial) = families.serial(name) {
                if serial >= (1 << 32) {
                    return Err("realm prototype serial exhaustion".into());
                }
                if serial >= result.serial_end {
                    result.serial_end = serial + 1;
                }
                meta.serial = serial;
                meta.flags |= OBJECT_META_FLAG_IS_PROTOTYPE;
                if let Some(family) = perry_abi::iterator_sealed_family(serial) {
                    seal(&mut meta.flags, family)?;
                }
            }
            if name == "%Global%" {
                seal(&mut meta.flags, perry_abi::INTRINSIC_FAMILIES)?;
            }
            if let Some(family) = perry_abi::builtin_namespace_family(name) {
                if cell
                    .header
                    .as_ref()
                    .is_some_and(|header| header.gc_type == 4)
                {
                    let bag = result.bags.entry(name.clone()).or_default();
                    seal(&mut bag.flags, family)?;
                } else {
                    seal(&mut meta.flags, family)?;
                }
            }
        }
        for (_, constructor_name) in perry_abi::BuiltinProto::ALL {
            let Some(constructor) = declarations.get(constructor_name) else {
                continue;
            };
            if constructor.kind != RealmKind::Constructor {
                continue;
            }
            let Some(name) = prototype(constructor) else {
                return Err(format!(
                    "builtin {constructor_name} lacks its prototype fact"
                ));
            };
            if let Some(meta) = result.cells.get_mut(name) {
                let family = perry_abi::BuiltinProto::ALL
                    .iter()
                    .find(|(_, name)| *name == constructor_name)
                    .unwrap()
                    .0;
                seal(&mut meta.flags, family as usize)?;
            }
        }
        for constructor in declarations.values() {
            let Some(facts) = &constructor.constructor else {
                continue;
            };
            if facts.sealed_serial_value.is_some() {
                let name = prototype(constructor).ok_or_else(|| {
                    format!("sealed constructor {} lacks prototype", constructor.name)
                })?;
                if let Some(meta) = result.cells.get_mut(name) {
                    seal(&mut meta.flags, perry_abi::RUNTIME_PROTOTYPE_FAMILY)?;
                }
            }
        }
        // A function tower declares its instance prototype in its own slot.
        // The serial distinguishes the two existing generator bodies.
        for root in declarations.values() {
            let Some(serial) = families.serial(&root.name) else {
                continue;
            };
            let family = match serial {
                perry_abi::intrinsic_serials::INTRINSIC_SERIAL_GENERATOR_FUNCTION => {
                    perry_abi::GENERATOR_PROTOTYPE_FAMILY
                }
                perry_abi::intrinsic_serials::INTRINSIC_SERIAL_ASYNC_GENERATOR_FUNCTION => {
                    perry_abi::ASYNC_GENERATOR_PROTOTYPE_FAMILY
                }
                _ => continue,
            };
            if let Some(meta) = prototype(root).and_then(|name| result.cells.get_mut(name)) {
                seal(&mut meta.flags, family)?;
            }
        }
        for meta in result.cells.values_mut().chain(result.bags.values_mut()) {
            if meta.flags & OBJECT_META_FLAG_IS_PROTOTYPE != 0 && meta.serial == 0 {
                meta.serial = result.serial_end;
                result.serial_end = result
                    .serial_end
                    .checked_add(1)
                    .filter(|end| *end < (1 << 32))
                    .ok_or("realm prototype serial exhaustion")?;
            }
        }
        Ok(result)
    }

    pub(crate) fn cell(&self, name: &str) -> Result<CellMeta, String> {
        self.cells
            .get(name)
            .copied()
            .ok_or_else(|| format!("realm cell {name} is absent from the emitted metadata plan"))
    }

    pub(crate) fn bag(&self, name: &str) -> Result<CellMeta, String> {
        self.bags
            .get(name)
            .copied()
            .ok_or_else(|| format!("realm closure {name} has no emitted bag metadata"))
    }

    pub(crate) fn serial(&self, name: &str) -> Result<u64, String> {
        self.cells
            .get(name)
            .filter(|meta| meta.serial != 0)
            .map(|meta| meta.serial)
            .ok_or_else(|| format!("image prototype {name} lacks its declared metadata"))
    }
}
