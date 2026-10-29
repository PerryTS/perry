//! A Set's entries are GC storage; its readonly scalar index is the same
//! owner-local index heap births build, borrowed until the first mutation.
use perry_hir::realm_manifest::{RealmDeclaration, RealmStorageFacts, RealmValue};

use super::atoms::RealmAtoms;
use super::*;

fn index(entries: &[RealmValue]) -> Result<Option<perry_abi::OwnedSetIndex>, String> {
    let hashes = entries
        .iter()
        .map(|value| {
            let RealmValue::String(text) = value else {
                return Err(
                    "realm Set requires source-derived stable key hashes for this key kind".into(),
                );
            };
            Ok(perry_abi::set_string_index_hash(
                crate::layout_desc::atom_hash(text),
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let first = perry_abi::SMALL_SET_SCAN_MAX as usize + 1;
    if entries.len() < first {
        return Ok(None);
    }
    let mut index = perry_abi::OwnedSetIndex::new(first);
    // Match the birth's insertion history, including the shared builder's
    // rehash order; sizing once from final length gives different buckets.
    for (slot, hash) in hashes.into_iter().enumerate() {
        index.insert_hash(
            hash,
            u32::try_from(slot).map_err(|_| "realm Set too large")?,
        );
    }
    Ok(Some(index))
}

pub(crate) fn emit_set(
    module: &mut LlModule,
    cell: &RealmDeclaration,
    atoms: &RealmAtoms,
) -> Result<Vec<TagFixup>, String> {
    let Some(RealmStorageFacts::Set { entries }) = &cell.storage else {
        return Err(format!(
            "realm Set {} lacks internal storage facts",
            cell.name
        ));
    };
    let header = cell
        .header
        .as_ref()
        .filter(|h| h.gc_type == 12)
        .ok_or_else(|| format!("realm {} is not a Set cell", cell.name))?;
    if !cell.properties.is_empty() || cell.prototype.as_deref() != Some("Set.prototype") {
        return Err(format!(
            "realm Set {} requires own-property or custom-prototype storage facts",
            cell.name
        ));
    }
    let size = u32::try_from(entries.len()).map_err(|_| "realm Set too large")?;
    if size >= 1 << 31 {
        return Err("realm Set exceeds receiver-word capacity".into());
    }
    let capacity = if size == 0 { 4 } else { size };
    let name = crate::realm_plan::cell_symbol(&cell.name);
    let store = format!("{name}_store");
    let meta = format!("{name}_meta");
    let index_name = format!("{name}_index");
    let ilp32 = crate::target_layout::target_is_ilp32(&module.target_triple);
    let integer = if ilp32 { "i32" } else { "i64" };
    let store_header = if ilp32 { 32 } else { 48 };
    let elements_offset = GC_HEADER_SIZE + store_header;
    let elements = format!("getelementptr (i8, ptr @{store}, i64 {elements_offset})");
    let mut fixups = Vec::new();
    let index = index(entries)?;
    if let Some(index) = &index {
        let count = index.buckets().len();
        let buckets = index
            .buckets()
            .iter()
            .map(|b| {
                format!(
                    "{{ i32, i32 }} {{ i32 {}, i32 {} }}",
                    b.hash as i32, b.index as i32
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        module.add_raw_global(format!("@{index_name}_buckets = private constant [{count} x {{ i32, i32 }}] [{buckets}], align 4"));
        module.add_raw_global(format!("@{index_name} = private constant {{ ptr, {integer}, {integer}, {integer} }} {{ ptr @{index_name}_buckets, {integer} {count}, {integer} {}, {integer} {} }}, align {}", index.live_len(), index.occupied_len(), if ilp32 { 4 } else { 8 }));
        fixups.extend(records::emit_metadata_record(
            module,
            &meta,
            Slot::Bits(0),
            0,
            0,
            Slot::Bits(0),
            Slot::Address {
                symbol: index_name,
                offset: 0,
                tag: 0,
            },
        ));
    }
    let mut types = Vec::new();
    let mut values = Vec::new();
    for slot in 0..capacity {
        let value = if let Some(value) = entries.get(slot as usize) {
            atoms.value(value)?
        } else {
            Slot::Bits(0)
        };
        let (ty, value, tag) = slot_init(&module.target_triple, &value);
        types.push(ty);
        values.push(format!("{ty} {value}"));
        if let Some(tag) = tag {
            fixups.push(TagFixup {
                cell: store.clone(),
                offset: elements_offset + u64::from(slot) * 8,
                tag,
            });
        }
    }
    let (pad_type, pad_value) = if ilp32 {
        ("i32, ", "i32 0, ")
    } else {
        ("", "")
    };
    let payload = types.join(", ");
    let store_size = elements_offset + u64::from(capacity) * 8;
    module.add_raw_global(format!(
        "@{store} = internal global {{ i8, i8, i16, i32, i64, ptr, {integer}, {integer}, ptr, ptr, {pad_type}{{ {payload} }} }} {{ i8 36, i8 {IMAGE_CELL_FLAGS}, i16 0, i32 {store_size}, i64 0, ptr {elements}, {integer} {capacity}, {integer} {size}, ptr null, ptr null, {pad_value}{{ {payload} }} {{ {} }} }}, section \"{}\", align 8",
        values.join(", "), section(&module.target_triple),
    ));
    let (tail_type, tail_value, header_size) = if ilp32 {
        (", i32", ", i32 0", 40)
    } else {
        ("", "", 48)
    };
    let meta = if index.is_some() {
        user_address(&meta)
    } else {
        "null".into()
    };
    module.add_raw_global(format!(
        "@{name} = global {{ i8, i8, i16, i32, i32, i32, ptr, ptr, i32, i32, ptr{tail_type} }} {{ i8 12, i8 {IMAGE_CELL_FLAGS}, i16 {}, i32 {header_size}, i32 {size}, i32 {capacity}, ptr {elements}, ptr {meta}, i32 {size}, i32 0, ptr {}{tail_value} }}, section \"{}\", align 8",
        header.reserved, user_address(&store), section(&module.target_triple),
    ));
    Ok(fixups)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_realm_sets_have_entries_and_readonly_indexes_on_each_target() {
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
            let mut count = 0;
            for cell in plan
                .cells
                .values()
                .filter(|c| matches!(c.storage, Some(RealmStorageFacts::Set { .. })))
            {
                let entries = cell.storage.as_ref().unwrap().values();
                assert!(entries.len() > 200);
                let tags = emit_set(&mut module, cell, &atoms).unwrap();
                assert_eq!(
                    tags.len(),
                    if target.contains("apple") {
                        entries.len()
                    } else {
                        0
                    }
                );
                let index = index(entries).unwrap().unwrap();
                assert_eq!(index.live_len(), entries.len());
                count += 1;
            }
            assert_eq!(count, 2);
            let ir = module.to_ir();
            assert!(ir.contains("_index = private constant"));
            let context = inkwell::context::Context::create();
            let parsed = crate::inprocess::parse_ir_text(&context, &ir, "realm_sets").unwrap();
            parsed.verify().unwrap();
            // LLVM syntax alone does not prove the ILP32 padding or that the
            // elements relocation addresses the actual inline payload.
            let ilp32 = crate::target_layout::target_is_ilp32(target);
            let data = inkwell::targets::TargetData::create(if ilp32 {
                "e-p:32:32-i64:64"
            } else {
                "e-p:64:64-i64:64"
            });
            for cell in plan.cells.values().filter(|c| c.storage.is_some()) {
                let name = crate::realm_plan::cell_symbol(&cell.name);
                let ty = parsed
                    .get_global(&name)
                    .unwrap()
                    .get_initializer()
                    .unwrap()
                    .into_struct_value()
                    .get_type();
                assert_eq!(data.get_abi_size(&ty), if ilp32 { 40 } else { 48 });
                assert_eq!(data.offset_of_element(&ty, 6), Some(16)); // elements
                assert_eq!(
                    data.offset_of_element(&ty, 7),
                    Some(if ilp32 { 20 } else { 24 })
                ); // meta
                assert_eq!(
                    data.offset_of_element(&ty, 10),
                    Some(if ilp32 { 32 } else { 40 })
                ); // store
                let store = parsed
                    .get_global(&format!("{name}_store"))
                    .unwrap()
                    .get_initializer()
                    .unwrap()
                    .into_struct_value()
                    .get_type();
                let payload = if ilp32 { 11 } else { 10 };
                let offset = if ilp32 { 40 } else { 56 };
                assert_eq!(data.offset_of_element(&store, payload), Some(offset));
                let count = cell.storage.as_ref().unwrap().values().len() as u64;
                assert_eq!(data.get_abi_size(&store), offset + count * 8);
            }
        }
    }
}
