//! Ordinary realm receivers carry their own prototype metadata and values.
use perry_hir::realm_manifest::RealmDeclaration;

use super::atoms::RealmAtoms;
use super::properties::emit_values;
use super::*;

pub(crate) struct ObjectFacts {
    pub(crate) word: u32,
    pub(crate) meta_flags: u64,
    pub(crate) proto_serial: u64,
}

pub(crate) fn emit_object(
    module: &mut LlModule,
    cell: &RealmDeclaration,
    facts: &ObjectFacts,
    atoms: &RealmAtoms,
) -> Result<Vec<TagFixup>, String> {
    let header = cell
        .header
        .as_ref()
        .filter(|h| h.gc_type == GC_TYPE_OBJECT)
        .ok_or_else(|| format!("realm {} is not an object cell", cell.name))?;
    if header.class_id == Some(0xffff_fffe) && cell.native_namespace.is_none() {
        return Err(format!(
            "realm namespace {} lacks its source-owned internal module identity",
            cell.name
        ));
    }
    let name = crate::realm_plan::cell_symbol(&cell.name);
    let meta = format!("{name}_meta");
    let (slots, mut fixups) = emit_values(module, &name, &cell.properties, atoms)?;
    let prototype = match cell.prototype.as_deref() {
        Some("Object.prototype") | None => Slot::Bits(0),
        Some(prototype) => Slot::Cell(crate::realm_plan::cell_symbol(prototype), POINTER_TAG),
    };
    let has_meta =
        !matches!(prototype, Slot::Bits(0)) || facts.meta_flags != 0 || facts.proto_serial != 0;
    if has_meta {
        fixups.extend(emit_metadata_cell(
            module,
            &meta,
            prototype,
            facts.meta_flags,
            facts.proto_serial,
        ));
    }
    fixups.extend(emit_object_cell_with_fields(
        module,
        &name,
        &ObjectCell {
            exported: true,
            class_id: header.class_id.unwrap_or(0),
            layout_word: facts.word,
            reserved: header.reserved,
            meta: has_meta.then_some(meta.as_str()),
            slots: &slots,
        },
    ));
    Ok(fixups)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_objects_emit_values_back_edges_and_custom_prototypes() {
        let hir = perry_parser::parse_typescript("console.log('hi');", "realm.cjs").unwrap();
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
            for (name, cell) in plan.cells.iter().chain(&plan.native_namespaces) {
                if !cell
                    .header
                    .as_ref()
                    .is_some_and(|h| h.gc_type == GC_TYPE_OBJECT)
                {
                    module.add_raw_global(format!(
                        "@{} = external global i8",
                        crate::realm_plan::cell_symbol(name)
                    ));
                }
            }
            let mut count = 0;
            for cell in plan
                .cells
                .values()
                .chain(plan.native_namespaces.values())
                .filter(|c| {
                    c.header
                        .as_ref()
                        .is_some_and(|h| h.gc_type == GC_TYPE_OBJECT)
                })
            {
                emit_object(
                    &mut module,
                    cell,
                    &ObjectFacts {
                        word: perry_abi::LayoutWord::new(
                            perry_abi::LayoutRow(5000 + count),
                            0,
                            if cell.prototype.is_none() {
                                2
                            } else if cell.prototype.as_deref() == Some("Object.prototype") {
                                0
                            } else {
                                1
                            },
                        )
                        .unwrap()
                        .0,
                        meta_flags: 0,
                        proto_serial: 0,
                    },
                    &atoms,
                )
                .unwrap();
                count += 1;
            }
            assert!(count >= 22);
            let ir = module.to_ir();
            assert!(ir.contains("@perry_realm_global = global"));
            assert!(ir.contains("@perry_realm_Object_prototype = global"));
            let context = inkwell::context::Context::create();
            crate::inprocess::parse_ir_text(&context, &ir, "realm_objects")
                .unwrap()
                .verify()
                .unwrap();
        }
    }
}
