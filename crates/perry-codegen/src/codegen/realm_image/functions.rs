//! Function objects, their own-property bags and rare prototype-state records.
//! All edges are born as image relocations, including constructor back-edges.
use perry_hir::realm_manifest::RealmDeclaration;

use super::atoms::RealmAtoms;
use super::properties::{emit_values, function_has_prototype_state, function_requires_dictionary};
use super::*;

pub(crate) struct FunctionLayouts {
    pub(crate) function: u32,
    pub(crate) bag: u32,
    pub(crate) state: Option<u32>,
    pub(crate) bag_flags: u64,
    pub(crate) bag_serial: u64,
}

pub(crate) fn emit_function(
    module: &mut LlModule,
    cell: &RealmDeclaration,
    layouts: &FunctionLayouts,
    atoms: &RealmAtoms,
) -> Result<Vec<TagFixup>, String> {
    let header = cell
        .header
        .as_ref()
        .filter(|h| h.gc_type == GC_TYPE_CLOSURE)
        .ok_or_else(|| format!("realm {} is not a function cell", cell.name))?;
    let info = cell
        .info_symbol
        .as_deref()
        .ok_or_else(|| format!("realm function {} lacks an info export", cell.name))?;
    let needs_state = function_has_prototype_state(cell)?;
    if needs_state != layouts.state.is_some() {
        return Err(format!(
            "realm function {} has inconsistent prototype-state layout",
            cell.name
        ));
    }
    if function_requires_dictionary(cell)?
        && layouts.function != perry_abi::FUNCTION_DICTIONARY_LAYOUT_WORD
    {
        return Err(format!(
            "realm function {} requires the dictionary layout",
            cell.name
        ));
    }
    let name = crate::realm_plan::cell_symbol(&cell.name);
    let bag = format!("{name}_props");
    let meta = format!("{bag}_meta");
    let state = format!("{name}_state");
    let (values, mut fixups) = emit_values(module, &name, &cell.properties, atoms)?;
    let has_meta = needs_state || layouts.bag_flags != 0 || layouts.bag_serial != 0;
    let has_bag = !values.is_empty() || has_meta;
    if let Some(word) = layouts.state {
        let prototype = cell
            .prototype
            .as_deref()
            .map_or(Slot::Bits(crate::nanbox::TAG_NULL), |p| {
                Slot::Cell(crate::realm_plan::cell_symbol(p), POINTER_TAG)
            });
        fixups.extend(emit_object_cell_with_fields(
            module,
            &state,
            &ObjectCell {
                exported: false,
                class_id: 0,
                layout_word: word,
                reserved: perry_abi::OBJ_FLAG_NULL_PROTO | perry_abi::OBJ_FLAG_PLAIN_ORDINARY,
                meta: None,
                slots: &[prototype],
            },
        ));
    }
    if has_meta {
        fixups.extend(emit_metadata_with_expando(
            module,
            &meta,
            Slot::Bits(0),
            layouts.bag_flags,
            layouts.bag_serial,
            if needs_state {
                Slot::Cell(state, POINTER_TAG)
            } else {
                Slot::Bits(0)
            },
        ));
    }
    if has_bag {
        let attrs = cell
            .properties
            .iter()
            .any(|p| crate::realm_plan::property_entry(p) != 0);
        fixups.extend(emit_object_cell_with_fields(
            module,
            &bag,
            &ObjectCell {
                exported: true,
                class_id: 0,
                layout_word: layouts.bag,
                reserved: perry_abi::OBJ_FLAG_NULL_PROTO
                    | perry_abi::OBJ_FLAG_PLAIN_ORDINARY
                    | if layouts.bag_flags
                        & perry_abi::object_meta_flags::OBJECT_META_FLAG_IS_PROTOTYPE
                        != 0
                    {
                        perry_abi::OBJ_FLAG_NEVER_MOVE
                    } else {
                        0
                    }
                    | if attrs {
                        perry_abi::OBJ_FLAG_HAS_DESCRIPTORS
                    } else {
                        0
                    },
                meta: has_meta.then_some(meta.as_str()),
                slots: &values,
            },
        ));
    }
    let captures = cell
        .captures
        .iter()
        .map(|value| atoms.value(value))
        .collect::<Result<Vec<_>, _>>()?;
    // Reflection can describe lazy name/length properties before the heap
    // function owns a bag. An image bag is born now, so apply the same header
    // fact change as layout_note_closure_edge_installed: pointer-free -> unknown.
    let reserved = if has_bag {
        perry_abi::closure_reserved_with_props(header.reserved)
    } else {
        header.reserved
    };
    fixups.extend(emit_closure_cell_with_fields(
        module,
        &name,
        &ClosureCell {
            exported: true,
            info_symbol: info,
            shape: layouts.function,
            reserved,
            capture_flags: header.capture_flags,
            props: has_bag.then_some(bag.as_str()),
            captures: &captures,
        },
    ));
    Ok(fixups)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_reached_function_owns_its_info_bag_captures_and_prototype_state() {
        for source in ["console.log('hi');", "with ({}) {}"] {
            let hir = perry_parser::parse_typescript(source, "realm.cjs").unwrap();
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
                    .chain(&plan.native_namespaces)
                    .chain(&plan.providers)
                {
                    if !cell
                        .header
                        .as_ref()
                        .is_some_and(|h| h.gc_type == GC_TYPE_CLOSURE)
                    {
                        module.add_raw_global(format!(
                            "@{} = external global i8",
                            crate::realm_plan::cell_symbol(name)
                        ));
                    }
                }
                let mut count = 0;
                for cell in plan.cells.values().filter(|c| {
                    c.header
                        .as_ref()
                        .is_some_and(|h| h.gc_type == GC_TYPE_CLOSURE)
                }) {
                    let state = function_has_prototype_state(cell).unwrap();
                    let layouts = FunctionLayouts {
                        function: if function_requires_dictionary(cell).unwrap() {
                            perry_abi::intrinsic_function_layout_word(4)
                        } else {
                            perry_abi::LayoutWord::new(perry_abi::LayoutRow(6000), 2, 1)
                                .unwrap()
                                .0
                        },
                        bag: perry_abi::LayoutWord::new(perry_abi::LayoutRow(5000), 0, 2)
                            .unwrap()
                            .0,
                        state: state.then(|| {
                            perry_abi::LayoutWord::new(perry_abi::LayoutRow(5001), 0, 2)
                                .unwrap()
                                .0
                        }),
                        bag_flags: 0,
                        bag_serial: 0,
                    };
                    emit_function(&mut module, cell, &layouts, &atoms).unwrap();
                    count += 1;
                }
                assert!(count >= if reach.is_full() { 2500 } else { 280 });
                let ir = module.to_ir();
                assert!(
                    ir.contains("_props_meta"),
                    "must exercise a constructor's recorded prototype"
                );
                let context = inkwell::context::Context::create();
                crate::inprocess::parse_ir_text(&context, &ir, "realm_functions")
                    .unwrap()
                    .verify()
                    .unwrap();
            }
        }
    }
}
