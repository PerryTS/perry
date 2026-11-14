//! Compiler-only row composition. Shared rows have identical complete facts;
//! each selected cell still owns its values and materialize-family byte.
use super::*;
use crate::layout_ordinals::{LayoutKey, LayoutRole, OrdinalInput, ProgramLayoutOrdinals};
use perry_hir::realm_manifest::{RealmProperty, RealmValue};
use std::collections::BTreeMap;

#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
pub(crate) enum Parent {
    Object,
    Null,
    Image { name: String, serial: u64 },
}

impl Parent {
    fn kind(&self) -> u8 {
        match self {
            Self::Object => 0,
            Self::Null => 2,
            Self::Image { .. } => 1,
        }
    }

    fn layout(&self) -> layouts::Prototype {
        match self {
            Self::Object => layouts::Prototype::Object,
            Self::Null => layouts::Prototype::Null,
            Self::Image { name, serial } => layouts::Prototype::Image {
                serial: *serial,
                cell: crate::realm_plan::cell_symbol(name),
            },
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
pub(crate) struct Key {
    pub(crate) atom: RealmValue,
    pub(crate) slot: u32,
    pub(crate) attrs: u8,
}

#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
pub(crate) struct Row {
    pub(crate) kind: u8,
    pub(crate) family: u8,
    pub(crate) rank: u8,
    pub(crate) integrity: u8,
    pub(crate) class_id: u32,
    pub(crate) parent: Parent,
    pub(crate) keys: Vec<Key>,
    pub(crate) slots: u32,
}

impl Row {
    pub(crate) fn properties(
        properties: &[RealmProperty],
        slots: &[u32],
        kind: perry_abi::ShapeObjectKind,
        family: u8,
        integrity: u8,
        class_id: u32,
        parent: Parent,
    ) -> Result<Self, String> {
        if slots.len() != properties.len() {
            return Err("realm property/slot counts disagree".into());
        }
        let keys = properties
            .iter()
            .zip(slots)
            .map(|(property, &slot)| Key {
                atom: if property.symbol {
                    RealmValue::Symbol(property.key.clone())
                } else {
                    RealmValue::String(property.key.clone())
                },
                slot,
                attrs: crate::realm_plan::property_entry(property),
            })
            .collect();
        let slots = slots.iter().max().map_or(Ok(0), |slot| {
            slot.checked_add(1).ok_or("realm slot count overflow")
        })?;
        Ok(Self {
            kind: kind.code() as u8,
            family,
            rank: 0,
            integrity,
            class_id,
            parent,
            keys,
            slots,
        })
    }

    pub(crate) fn with_rank(mut self, rank: u8) -> Self {
        self.rank = rank;
        self
    }

    fn content(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("row facts serialize")
    }

    fn symbol(&self) -> String {
        let hash = self
            .content()
            .iter()
            .fold(0xcbf29ce484222325u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
            });
        format!("perry_realm_layout_{hash:016x}")
    }

    pub(crate) fn input(&self) -> OrdinalInput {
        OrdinalInput {
            key: LayoutKey::Descriptor {
                module: "$realm".into(),
                symbol: self.symbol(),
                content: self.content(),
            },
            requested_shape_id: 0,
            band: if self.kind == perry_abi::ShapeObjectKind::Function.code() as u8 {
                2
            } else {
                0
            },
            proto_kind: self.parent.kind(),
            role: LayoutRole::LazyImage,
        }
    }

    pub(crate) fn word(&self, plan: &ProgramLayoutOrdinals) -> Result<u32, String> {
        let input = self.input();
        plan.entry(&input.key)
            .map(|entry| entry.word())
            .ok_or_else(|| format!("realm row {} absent from final ordinal plan", self.symbol()))
    }

    pub(crate) fn pending_word(&self, plan: &ProgramLayoutOrdinals) -> Result<u32, String> {
        self.word(plan)
            .map(|word| word | perry_abi::LAYOUT_ROW_PENDING)
    }

    fn layout(
        &self,
        module: &mut LlModule,
        atoms: &atoms::RealmAtoms,
        plan: &ProgramLayoutOrdinals,
    ) -> Result<layouts::LayoutFacts, String> {
        let kind = [
            perry_abi::ShapeObjectKind::Ordinary,
            perry_abi::ShapeObjectKind::OrdinaryUnmarked,
            perry_abi::ShapeObjectKind::CallableOrdinary,
            perry_abi::ShapeObjectKind::Function,
            perry_abi::ShapeObjectKind::NativeNamespace,
        ]
        .into_iter()
        .find(|kind| kind.code() as u8 == self.kind)
        .ok_or_else(|| format!("unsupported realm row kind {}", self.kind))?;
        let keys = self
            .keys
            .iter()
            .map(|key| atoms.layout_key(module, &key.atom, key.slot, key.attrs))
            .collect::<Result<_, _>>()?;
        Ok(layouts::LayoutFacts {
            symbol: self.symbol(),
            word: self.word(plan)?,
            family: 0,
            realm_family: self.family,
            realm_rank: self.rank,
            variant: 0,
            kind,
            integrity: self.integrity,
            slot_count: self.slots,
            rep: 0,
            prototype: self.parent.layout(),
            keys,
            class_id: self.class_id,
            parent_class_id: 0,
        })
    }
}

/// The compiler deduplicates by full content. The short symbol is only a
/// linker spelling, and collisions are rejected before any emission.
pub(crate) struct Rows {
    by_symbol: BTreeMap<String, Row>,
}

impl Rows {
    pub(crate) fn new() -> Self {
        Self {
            by_symbol: BTreeMap::new(),
        }
    }

    pub(crate) fn insert(&mut self, row: Row) -> Result<Row, String> {
        let symbol = row.symbol();
        if let Some(previous) = self.by_symbol.get(&symbol) {
            if previous != &row {
                return Err(format!("realm row symbol collision {symbol}"));
            }
        } else {
            self.by_symbol.insert(symbol, row.clone());
        }
        Ok(row)
    }

    pub(crate) fn inputs(&self) -> Vec<OrdinalInput> {
        self.by_symbol.values().map(Row::input).collect()
    }

    pub(crate) fn emit(
        &self,
        module: &mut LlModule,
        atoms: &atoms::RealmAtoms,
        plan: &ProgramLayoutOrdinals,
    ) -> Result<Vec<TagFixup>, String> {
        let mut facts = self
            .by_symbol
            .values()
            .map(|row| row.layout(module, atoms, plan))
            .collect::<Result<Vec<_>, _>>()?;
        facts.sort_by_key(|row| perry_abi::LayoutWord(row.word).row().0);
        if facts.len() != plan.lazy_count as usize {
            return Err("realm row span differs from final ordinal plan".into());
        }
        layouts::emit_layouts(module, plan.lazy_first, &facts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn realm_rows_share_full_layout_facts_without_sharing_values_or_families() {
        let property = |name: &str| RealmProperty {
            key: "name".into(),
            symbol: false,
            attributes: 4,
            value: Some(RealmValue::String(name.into())),
            get: None,
            set: None,
        };
        let row = |name: &str, family| {
            Row::properties(
                &[property(name)],
                &[0],
                perry_abi::ShapeObjectKind::Ordinary,
                family,
                0,
                0,
                Parent::Null,
            )
            .unwrap()
        };
        let mut rows = Rows::new();
        let first = rows.insert(row("a", 7)).unwrap();
        assert_eq!(first, rows.insert(row("b", 7)).unwrap());
        let other = rows.insert(row("a", 8)).unwrap();
        assert_ne!(first, other);
        assert_eq!(rows.inputs().len(), 2);
        let plan =
            ProgramLayoutOrdinals::assign(crate::layout_ordinals::native_inputs(), rows.inputs())
                .unwrap();
        assert_ne!(first.word(&plan).unwrap(), other.word(&plan).unwrap());
        assert_eq!(
            first.pending_word(&plan).unwrap() & !perry_abi::LAYOUT_ROW_PENDING,
            first.word(&plan).unwrap()
        );
        assert!(plan
            .entries
            .iter()
            .filter(|row| row.role == LayoutRole::LazyImage)
            .all(|row| row.ordinal >= plan.lazy_first));
    }
}
