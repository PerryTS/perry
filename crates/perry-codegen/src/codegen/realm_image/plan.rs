//! Complete compiler-side receiver facts, before assigning any physical row.
use super::families::Families;
use super::metadata::Metadata;
use super::rows::{Parent, Row, Rows};
use super::*;
use perry_hir::realm_manifest::{RealmDeclaration, RealmProperty};
use std::collections::BTreeMap;

pub(crate) enum Receiver {
    Object(Row),
    Function {
        function: Option<Row>,
        bag: Row,
        state: Option<Row>,
    },
    Array {
        named: Option<Row>,
        slots: Vec<u32>,
    },
    Set,
}

pub(crate) struct ImagePlan {
    pub(crate) realm: crate::realm_plan::RealmPlan,
    pub(crate) receivers: BTreeMap<String, Receiver>,
    pub(crate) rows: Rows,
    pub(crate) metadata: Metadata,
}

fn parent(cell: &RealmDeclaration, metadata: &Metadata) -> Result<Parent, String> {
    match cell.prototype.as_deref() {
        None => Ok(Parent::Null),
        Some("Object.prototype") => Ok(Parent::Object),
        Some(name) => Ok(Parent::Image {
            name: name.into(),
            serial: metadata.serial(name)?,
        }),
    }
}

fn ordinary_kind(flags: u64) -> perry_abi::ShapeObjectKind {
    if flags & perry_abi::object_meta_flags::OBJECT_META_FLAG_IS_PROTOTYPE != 0 {
        perry_abi::ShapeObjectKind::OrdinaryUnmarked
    } else {
        perry_abi::ShapeObjectKind::Ordinary
    }
}

fn sequential(properties: &[RealmProperty]) -> Result<Vec<u32>, String> {
    (0..properties.len())
        .map(|slot| u32::try_from(slot).map_err(|_| "realm property count overflow".into()))
        .collect()
}

fn function_state() -> RealmProperty {
    use perry_hir::realm_manifest::RealmValue;
    RealmProperty {
        key: "p".into(),
        symbol: false,
        attributes: 7,
        value: Some(RealmValue::Undefined),
        get: None,
        set: None,
    }
}

impl ImagePlan {
    pub(crate) fn new(
        reach: &perry_hir::realm_reach::RealmReach,
        triple: &str,
    ) -> Result<Self, String> {
        let mut realm = crate::realm_plan::RealmPlan::from_reach(reach)?;
        let index = perry_hir::realm_reach_index::RealmIndex::new(crate::realm_manifest::REALM);
        let declarations = index
            .rows()
            .map(|entry| {
                serde_json::from_value::<RealmDeclaration>(entry.facts().clone())
                    .map(|cell| (cell.name.clone(), cell))
                    .map_err(|error| format!("realm {}: {error}", entry.name()))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let creates_realm = index
            .last("%Global%")
            .is_some_and(|global| reach.executes(&index, global));
        if !creates_realm {
            realm.cells.clear();
            realm.providers.clear();
            realm.native_namespaces.clear();
        } else {
            let native = crate::realm_plan::native_owned_cells(&declarations)?;
            for name in native {
                if let Some(cell) = realm
                    .cells
                    .remove(&name)
                    .or_else(|| realm.native_namespaces.remove(&name))
                {
                    realm.providers.insert(name, cell);
                }
            }
        }
        realm.refresh_atoms()?;
        // Private storage comes from the same source-owned runtime layout,
        // independently of public member selection. Its attrs remain private
        // in the reduced image row. A held intrinsic target is an image
        // pointer from birth; observing the private slot need not birth or
        // publish that target's pending row.
        let held = realm
            .cells
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        for layout in realm.runtime_layouts.values() {
            for owner in &layout.owners {
                let Some(cell) = realm.cells.get_mut(owner) else {
                    continue;
                };
                for key in &layout.for_target(triple)?.keys {
                    if key.attributes & perry_abi::LAYOUT_KEY_PRIVATE_FIELD
                        != perry_abi::LAYOUT_KEY_PRIVATE_FIELD
                    {
                        continue;
                    }
                    let perry_hir::realm_manifest::RealmValue::String(name) =
                        key.atom.realm_value()?
                    else {
                        return Err("private realm storage requires a string key".into());
                    };
                    if cell.properties.iter().any(|p| p.key == name && !p.symbol) {
                        return Err("private storage repeats a public member".into());
                    }
                    let value = perry_abi::realm_private::intrinsic_target(&name)
                        .and_then(|(kind, part)| {
                            let target = match part {
                                perry_abi::realm_private::IntrinsicPart::Prototype => {
                                    format!("{kind}.prototype")
                                }
                                perry_abi::realm_private::IntrinsicPart::Constructor => {
                                    kind.to_owned()
                                }
                            };
                            held.contains(&target)
                                .then_some(perry_hir::realm_manifest::RealmValue::Reference(target))
                        })
                        .unwrap_or(perry_hir::realm_manifest::RealmValue::Undefined);
                    cell.properties.push(RealmProperty {
                        key: name,
                        symbol: false,
                        attributes: (!key.attributes & 7) | (key.attributes & 64),
                        value: Some(value),
                        get: None,
                        set: None,
                    });
                }
            }
        }
        let families = if realm.cells.is_empty() {
            Families::empty()
        } else {
            Families::from_declarations(&declarations)?
        };
        let metadata = Metadata::new(&realm, &declarations, &families)?;
        let mut rows = Rows::new();
        let mut receivers = BTreeMap::new();
        for (name, cell) in &realm.cells {
            let header = cell
                .header
                .as_ref()
                .ok_or_else(|| format!("realm {name} lacks header"))?;
            let family = families.family(name)?;
            let rank = if reach.whole.contains(name) {
                perry_abi::REALM_RANK_WHOLE
            } else {
                perry_abi::REALM_RANK_STRUCTURE
            };
            let meta = metadata.cell(name)?;
            let receiver = match header.gc_type {
                perry_abi::gc_object_type_pat!() => {
                    let row = Row::properties(
                        &cell.properties,
                        &sequential(&cell.properties)?,
                        if header.reserved & perry_abi::GC_OBJECT_CALLABLE != 0 {
                            perry_abi::ShapeObjectKind::CallableOrdinary
                        } else {
                            ordinary_kind(meta.flags)
                        },
                        family,
                        (header.reserved
                            & (perry_abi::OBJ_FLAG_FROZEN
                                | perry_abi::OBJ_FLAG_SEALED
                                | perry_abi::OBJ_FLAG_NO_EXTEND)) as u8,
                        header.class_id.unwrap_or(0),
                        parent(cell, &metadata)?,
                    )?
                    .with_rank(rank);
                    Receiver::Object(rows.insert(row)?)
                }
                4 => {
                    let bag_meta = metadata.bag(name)?;
                    let keys = sequential(&cell.properties)?;
                    let bag = rows.insert(
                        Row::properties(
                            &cell.properties,
                            &keys,
                            ordinary_kind(bag_meta.flags),
                            family,
                            0,
                            0,
                            Parent::Null,
                        )?
                        .with_rank(rank),
                    )?;
                    let function = if properties::function_requires_dictionary(cell)? {
                        None
                    } else {
                        Some(
                            rows.insert(
                                Row::properties(
                                    &cell.properties,
                                    &keys,
                                    perry_abi::ShapeObjectKind::Function,
                                    family,
                                    0,
                                    0,
                                    parent(cell, &metadata)?,
                                )?
                                .with_rank(rank),
                            )?,
                        )
                    };
                    let state = if properties::function_has_prototype_state(cell)? {
                        Some(
                            rows.insert(
                                Row::properties(
                                    &[function_state()],
                                    &[0],
                                    perry_abi::ShapeObjectKind::Ordinary,
                                    family,
                                    0,
                                    0,
                                    Parent::Null,
                                )?
                                .with_rank(rank),
                            )?,
                        )
                    } else {
                        None
                    };
                    Receiver::Function {
                        function,
                        bag,
                        state,
                    }
                }
                perry_abi::gc_array_type_pat!() => {
                    let parts = arrays::ArrayParts::from_cell(cell)?;
                    let layouts = realm
                        .runtime_layouts
                        .values()
                        .filter(|layout| layout.owners.iter().any(|owner| owner == name))
                        .collect::<Vec<_>>();
                    if layouts.len() > 1 {
                        return Err(format!("realm array {name} has ambiguous named storage"));
                    }
                    let slots = if let Some(layout) = layouts.first() {
                        let layout = layout.for_target(triple)?;
                        let mut slots = Vec::new();
                        for property in &parts.named {
                            let expected = if property.symbol {
                                perry_hir::realm_manifest::RealmValue::Symbol(property.key.clone())
                            } else {
                                perry_hir::realm_manifest::RealmValue::String(property.key.clone())
                            };
                            let keys = layout
                                .keys
                                .iter()
                                .filter(|key| key.atom.realm_value().as_ref() == Ok(&expected))
                                .collect::<Vec<_>>();
                            if keys.len() != 1 {
                                return Err(format!(
                                    "realm array {name}.{} lacks one declared slot",
                                    property.key
                                ));
                            }
                            slots.push(keys[0].slot);
                        }
                        slots
                    } else {
                        sequential(&parts.named)?
                    };
                    let named = if parts.named.is_empty() && meta.flags == 0 && meta.serial == 0 {
                        None
                    } else {
                        Some(
                            rows.insert(
                                Row::properties(
                                    &parts.named,
                                    &slots,
                                    ordinary_kind(meta.flags),
                                    family,
                                    0,
                                    0,
                                    Parent::Null,
                                )?
                                .with_rank(rank),
                            )?,
                        )
                    };
                    Receiver::Array { named, slots }
                }
                12 => Receiver::Set,
                other => return Err(format!("realm {name} has unsupported GC type {other}")),
            };
            receivers.insert(name.clone(), receiver);
        }
        Ok(Self {
            realm,
            receivers,
            rows,
            metadata,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn realm_keyed_function_row_keeps_its_declared_prototype_without_owning_the_constructor() {
        let index = perry_hir::realm_reach_index::RealmIndex::new(crate::realm_manifest::REALM);
        let mut reach = perry_hir::realm_reach::RealmReach::default();
        reach.own("BigInt");
        reach.exec("js_get_global_this");
        reach.close(&index);
        assert!(!reach.own_names.contains("Function"));
        for triple in [
            "x86_64-unknown-linux-gnu",
            "aarch64-apple-darwin",
            "x86_64-pc-windows-msvc",
            "wasm32-wasip2",
        ] {
            let image = ImagePlan::new(&reach, triple).unwrap();
            let Receiver::Function {
                function: Some(function),
                bag,
                ..
            } = &image.receivers["BigInt"]
            else {
                panic!("BigInt must exercise a keyed function row")
            };
            assert_eq!(
                function.parent,
                Parent::Image {
                    name: "Function.prototype".into(),
                    serial: perry_abi::intrinsic_serials::INTRINSIC_SERIAL_FUNCTION
                }
            );
            assert_eq!(
                bag.parent,
                Parent::Null,
                "the private props bag stays separate from the function's prototype"
            );
        }
    }
    #[test]
    fn realm_receiver_descriptors_carry_the_closed_rank_independently_of_selected_keys() {
        let index = perry_hir::realm_reach_index::RealmIndex::new(crate::realm_manifest::REALM);
        let mut reach = perry_hir::realm_reach::RealmReach::default();
        reach.own("BigInt");
        reach.exec("js_get_global_this");
        reach.close(&index);
        let image = ImagePlan::new(&reach, "x86_64-unknown-linux-gnu").unwrap();
        let mut seen = [false; 2];
        for (name, receiver) in &image.receivers {
            let whole = reach.whole.contains(name);
            let expected = if whole {
                perry_abi::REALM_RANK_WHOLE
            } else {
                perry_abi::REALM_RANK_STRUCTURE
            };
            let mut check = |row: &Row| {
                assert_eq!(row.rank, expected, "{name}");
                seen[usize::from(whole)] = true;
            };
            match receiver {
                Receiver::Object(row) => check(row),
                Receiver::Function {
                    function,
                    bag,
                    state,
                } => {
                    check(bag);
                    if let Some(row) = function {
                        check(row);
                    }
                    if let Some(row) = state {
                        check(row);
                    }
                }
                Receiver::Array {
                    named: Some(row), ..
                } => check(row),
                _ => {}
            }
        }
        assert_eq!(
            seen,
            [true, true],
            "the fixture exercises both closed ranks"
        );
    }
}
