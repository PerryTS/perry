//! Realm arrays split indexed elements from their ordinary named holder.
//! Storage-slot assignment is supplied by the descriptor planner: some runtime
//! consumers (notably @@iterator) have a fixed slot independent of key order.
use super::atoms::RealmAtoms;
use super::objects::ObjectFacts;
use super::properties::emit_values;
use super::*;
use perry_hir::realm_manifest::{RealmDeclaration, RealmProperty, RealmValue};

pub(crate) struct NamedArrayFacts<'a> {
    pub(crate) object: &'a ObjectFacts,
    pub(crate) reserved: u16,
    pub(crate) slots: &'a [u32],
}

pub(crate) struct ArrayParts<'a> {
    pub(crate) length: u32,
    pub(crate) elements: Vec<&'a RealmValue>,
    pub(crate) named: Vec<RealmProperty>,
}

impl<'a> ArrayParts<'a> {
    pub(crate) fn from_cell(cell: &'a RealmDeclaration) -> Result<Self, String> {
        let header = cell
            .header
            .as_ref()
            .filter(|h| h.gc_type == GC_TYPE_ARRAY)
            .ok_or_else(|| format!("realm {} is not an array", cell.name))?;
        let length = cell
            .properties
            .iter()
            .find(|p| !p.symbol && p.key == "length")
            .ok_or_else(|| format!("realm array {} lacks a length birth fact", cell.name))?;
        let Some(RealmValue::NumberBits(bits)) = &length.value else {
            return Err(format!("realm array {} has a nonnumeric length", cell.name));
        };
        let bits = u64::from_str_radix(bits.trim_start_matches("0x"), 16)
            .map_err(|_| format!("realm array {} has an invalid length", cell.name))?;
        let n = f64::from_bits(bits);
        if !n.is_finite() || n < 0.0 || n > u32::MAX as f64 || n.fract() != 0.0 {
            return Err(format!("realm array {} has an invalid length", cell.name));
        }
        // Integrity restrictions are represented by the receiver header, as
        // on heap arrays; do not materialize redundant element descriptors.
        let frozen = header.reserved & perry_abi::OBJ_FLAG_FROZEN != 0;
        let sealed = header.reserved & perry_abi::OBJ_FLAG_SEALED != 0;
        if length.attributes != u8::from(!frozen) || length.get.is_some() || length.set.is_some() {
            return Err(format!(
                "realm array {} needs length descriptor storage",
                cell.name
            ));
        }
        let default_attrs = if frozen {
            2
        } else if sealed {
            3
        } else {
            7
        };
        let mut indexed = std::collections::BTreeMap::new();
        let mut named = Vec::new();
        for property in &cell.properties {
            if !property.symbol && property.key == "length" {
                continue;
            }
            let index = (!property.symbol)
                .then(|| property.key.parse::<u32>().ok())
                .flatten()
                .filter(|i| *i != u32::MAX && i.to_string() == property.key);
            if let Some(index) = index {
                let value = property
                    .value
                    .as_ref()
                    .filter(|_| property.get.is_none() && property.set.is_none())
                    .ok_or_else(|| {
                        format!("realm array {} needs an indexed accessor holder", cell.name)
                    })?;
                if property.attributes != default_attrs || index >= n as u32 {
                    return Err(format!(
                        "realm array {} needs indexed descriptor storage",
                        cell.name
                    ));
                }
                if indexed.insert(index, value).is_some() {
                    return Err("duplicate realm array index".into());
                }
            } else {
                named.push(property.clone());
            }
        }
        // Current source-owned static arrays are dense. A future sparse birth
        // must describe its holes explicitly, not silently emit undefined.
        if indexed.len() != n as usize {
            return Err(format!(
                "realm array {} lacks dense element birth facts",
                cell.name
            ));
        }
        let elements = indexed.into_values().collect();
        Ok(Self {
            length: n as u32,
            elements,
            named,
        })
    }
}

pub(crate) fn emit_array(
    module: &mut LlModule,
    cell: &RealmDeclaration,
    parts: &ArrayParts<'_>,
    named_facts: Option<&NamedArrayFacts<'_>>,
    atoms: &RealmAtoms,
) -> Result<Vec<TagFixup>, String> {
    let header = cell.header.as_ref().ok_or("array lacks a header")?;
    if cell.prototype.as_deref() != Some("Array.prototype")
        && !(cell.name == "Array.prototype"
            && cell.prototype.as_deref() == Some("Object.prototype"))
    {
        return Err(format!(
            "realm array {} needs custom prototype storage",
            cell.name
        ));
    }
    let name = crate::realm_plan::cell_symbol(&cell.name);
    let named = format!("{name}_named");
    let meta = format!("{named}_meta");
    let mut tags = Vec::new();
    if let Some(named_facts) = named_facts {
        let facts = named_facts.object;
        let slots = named_facts.slots;
        if slots.len() != parts.named.len() {
            return Err("array named slot count disagrees with descriptor".into());
        }
        let (values, fixups) = emit_values(module, &named, &parts.named, atoms)?;
        tags.extend(fixups);
        let mut ordered = vec![Slot::Bits(crate::nanbox::TAG_HOLE); slots.len()];
        let mut seen = vec![false; slots.len()];
        for (value, &slot) in values.into_iter().zip(slots) {
            let i = slot as usize;
            if i >= ordered.len() || seen[i] {
                return Err("array named slots must be a permutation".into());
            }
            seen[i] = true;
            ordered[i] = value;
        }
        let has_meta = facts.meta_flags != 0 || facts.proto_serial != 0;
        if has_meta {
            tags.extend(emit_metadata_cell(
                module,
                &meta,
                Slot::Bits(0),
                facts.meta_flags,
                facts.proto_serial,
            ));
        }
        tags.extend(emit_object_cell_with_fields(
            module,
            &named,
            &ObjectCell {
                exported: true,
                class_id: 0,
                layout_word: facts.word,
                reserved: named_facts.reserved,
                meta: has_meta.then_some(meta.as_str()),
                slots: &ordered,
            },
        ));
    } else if !parts.named.is_empty() {
        return Err(format!(
            "realm array {} has no descriptor for its named properties",
            cell.name
        ));
    }
    let elements = parts
        .elements
        .iter()
        .map(|v| atoms.value(v))
        .collect::<Result<Vec<_>, _>>()?;
    tags.extend(emit_array_cell_with_fields(
        module,
        &name,
        &ArrayCell {
            exported: true,
            shared: false,
            reserved: header.reserved,
            length: parts.length,
            capacity: parts.length,
            // The decomposition proved every indexed slot is present and
            // plain data, with integrity held only on the receiver header.
            dense_prefix: parts.length,
            named: named_facts.map(|_| named.as_str()),
            slots: &elements,
        },
    ));
    Ok(tags)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_realm_arrays_keep_dense_elements_and_named_values_separate() {
        let hir = perry_parser::parse_typescript("with ({}) {}", "realm.cjs").unwrap();
        let reach = perry_hir::realm_reach::scan(&hir, crate::realm_manifest::REALM);
        let plan = crate::realm_plan::RealmPlan::from_reach(&reach).unwrap();
        for target in [
            "x86_64-unknown-linux-gnu",
            "aarch64-apple-darwin",
            "x86_64-pc-windows-msvc",
            "wasm32-wasip2",
        ] {
            let mut module = LlModule::new(target);
            let atoms = RealmAtoms::emit(&mut module, &plan).unwrap();
            for (name, cell) in plan
                .cells
                .iter()
                .chain(&plan.providers)
                .chain(&plan.native_namespaces)
            {
                if !cell
                    .header
                    .as_ref()
                    .is_some_and(|h| h.gc_type == GC_TYPE_ARRAY)
                    || plan.providers.contains_key(name)
                {
                    module.add_raw_global(format!(
                        "@{} = external global i8",
                        crate::realm_plan::cell_symbol(name)
                    ));
                }
            }
            let mut arrays = 0;
            let mut frozen = 0;
            for cell in plan.cells.values().filter(|c| {
                c.header
                    .as_ref()
                    .is_some_and(|h| h.gc_type == GC_TYPE_ARRAY)
            }) {
                let parts = ArrayParts::from_cell(cell).unwrap();
                let word = perry_abi::LayoutWord::new(perry_abi::LayoutRow(5000), 0, 0)
                    .unwrap()
                    .0;
                let object = ObjectFacts {
                    word,
                    meta_flags: 0,
                    proto_serial: 0,
                };
                // Exercise storage order independent of reflection order.
                let slots = (0..parts.named.len() as u32).rev().collect::<Vec<_>>();
                let facts = NamedArrayFacts {
                    object: &object,
                    reserved: perry_abi::OBJ_FLAG_HAS_DESCRIPTORS,
                    slots: &slots,
                };
                emit_array(
                    &mut module,
                    cell,
                    &parts,
                    (!parts.named.is_empty()).then_some(&facts),
                    &atoms,
                )
                .unwrap();
                assert_eq!(parts.length as usize, parts.elements.len());
                arrays += 1;
                if cell.header.as_ref().unwrap().reserved & perry_abi::OBJ_FLAG_FROZEN != 0 {
                    frozen += 1;
                }
                if cell.name == "Array.prototype" {
                    assert_eq!(parts.length, 0);
                    assert!(parts
                        .named
                        .iter()
                        .any(|p| p.symbol && p.key == "@@iterator"));
                    assert!(parts.named.iter().any(|p| p.key == "constructor"));
                }
            }
            assert!(arrays >= 60);
            assert!(frozen >= 2);
            let context = inkwell::context::Context::create();
            let parsed =
                crate::inprocess::parse_ir_text(&context, &module.to_ir(), "realm_arrays").unwrap();
            parsed.verify().unwrap();
            let ilp32 = crate::target_layout::target_is_ilp32(target);
            let data = inkwell::targets::TargetData::create(if ilp32 {
                "e-p:32:32-i64:64"
            } else {
                "e-p:64:64-i64:64"
            });
            for cell in plan.cells.values().filter(|c| {
                c.header
                    .as_ref()
                    .is_some_and(|h| h.gc_type == GC_TYPE_ARRAY)
            }) {
                let parts = ArrayParts::from_cell(cell).unwrap();
                let value = parsed
                    .get_global(&crate::realm_plan::cell_symbol(&cell.name))
                    .unwrap()
                    .get_initializer()
                    .unwrap()
                    .into_struct_value();
                let prefix = if ilp32 { 8 } else { 7 };
                let payload = prefix + 2;
                assert_eq!(data.offset_of_element(&value.get_type(), prefix), Some(24));
                assert_eq!(data.offset_of_element(&value.get_type(), payload), Some(32));
                assert_eq!(
                    data.get_abi_size(&value.get_type()),
                    32 + u64::from(parts.length) * 8
                );
                assert_eq!(
                    value
                        .get_field_at_index(prefix)
                        .unwrap()
                        .into_int_value()
                        .get_zero_extended_constant(),
                    Some(u64::from(parts.length))
                );
            }
        }
    }
}
