//! Own-property values and accessor pairs have one physical representation on
//! objects and constructor bags. Property order is the RealmPlan's slot order.
use perry_hir::realm_manifest::{RealmDeclaration, RealmProperty};

use super::atoms::RealmAtoms;
use super::*;

pub(crate) fn emit_values(
    module: &mut LlModule,
    name: &str,
    properties: &[RealmProperty],
    atoms: &RealmAtoms,
) -> Result<(Vec<Slot>, Vec<TagFixup>), String> {
    let mut values = Vec::with_capacity(properties.len());
    let mut fixups = Vec::new();
    for (index, property) in properties.iter().enumerate() {
        if let Some(value) = &property.value {
            if property.get.is_some() || property.set.is_some() {
                return Err(format!(
                    "realm {name}.{} has data and accessor facts",
                    property.key
                ));
            }
            values.push(atoms.value(value)?);
        } else {
            let pair = format!("{name}_accessor_{index}");
            let get = property.get.as_deref().map(crate::realm_plan::cell_symbol);
            let set = property.set.as_deref().map(crate::realm_plan::cell_symbol);
            fixups.extend(emit_accessor_pair(
                module,
                &pair,
                get.as_deref(),
                set.as_deref(),
            ));
            values.push(Slot::Cell(pair, POINTER_TAG));
        }
    }
    Ok((values, fixups))
}

/// Bodies carry their default prototype kind in JsFunctionInfo. Only an
/// override needs the existing function-state record's `p` slot.
pub(crate) fn function_has_prototype_state(cell: &RealmDeclaration) -> Result<bool, String> {
    let info = cell
        .function_info
        .as_ref()
        .ok_or_else(|| format!("realm function {} lacks info facts", cell.name))?;
    let default = perry_abi::FunctionProtoKind::of_flags(info.flags).prototype_name();
    Ok(cell.prototype.as_deref() != Some(default))
}

/// The existing closure mutation funnels use the dictionary shape for an
/// accessor, a symbol key or a recorded prototype. String data properties
/// alone use a keyed Function shape whose keys belong to the props bag.
pub(crate) fn function_requires_dictionary(cell: &RealmDeclaration) -> Result<bool, String> {
    Ok(function_has_prototype_state(cell)?
        || cell
            .properties
            .iter()
            .any(|property| property.symbol || property.value.is_none()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::realm_plan::property_entry;

    #[test]
    fn accessor_halves_and_data_references_emit_in_manifest_order() {
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
            for cell in plan.cells.keys().chain(plan.native_namespaces.keys()) {
                module.add_raw_global(format!(
                    "@{} = external global i8",
                    crate::realm_plan::cell_symbol(cell)
                ));
            }
            let mut accessors = 0;
            for (name, cell) in &plan.cells {
                let (slots, _) = emit_values(
                    &mut module,
                    &crate::realm_plan::cell_symbol(name),
                    &cell.properties,
                    &atoms,
                )
                .unwrap();
                assert_eq!(slots.len(), cell.properties.len());
                for (property, slot) in cell.properties.iter().zip(slots) {
                    if property.value.is_none() {
                        accessors += 1;
                        assert!(matches!(slot, Slot::Cell(_, POINTER_TAG)));
                        assert_ne!(property_entry(property) & perry_abi::LAYOUT_KEY_ACCESSOR, 0);
                    } else {
                        assert_eq!(property_entry(property) & perry_abi::LAYOUT_KEY_ACCESSOR, 0);
                    }
                }
            }
            assert!(
                accessors > 0,
                "the fixture must exercise real accessor cells"
            );
            let context = inkwell::context::Context::create();
            crate::inprocess::parse_ir_text(&context, &module.to_ir(), "realm_properties")
                .unwrap()
                .verify()
                .unwrap();
        }
    }

    #[test]
    fn inherited_constructor_prototype_uses_function_state() {
        let hir = perry_parser::parse_typescript("console.log('hi');", "realm.cjs").unwrap();
        let reach = perry_hir::realm_reach::scan(&hir, crate::realm_manifest::REALM);
        let plan = crate::realm_plan::RealmPlan::from_reach(&reach).unwrap();
        assert!(!function_has_prototype_state(&plan.cells["Array"]).unwrap());
        assert!(
            function_requires_dictionary(&plan.cells["Array"]).unwrap(),
            "@@species is an accessor on a symbol"
        );
        assert!(!function_requires_dictionary(&plan.cells["Error"]).unwrap());
        assert!(!function_has_prototype_state(&plan.cells["Error"]).unwrap());
        assert!(function_has_prototype_state(&plan.cells["TypeError"]).unwrap());
        assert!(function_has_prototype_state(&plan.cells["AggregateError"]).unwrap());
    }

    #[test]
    fn body_kind_defaults_resolve_manifest_cells_and_overrides_need_state() {
        let hir = perry_parser::parse_typescript("with ({}) {}", "realm.cjs").unwrap();
        let reach = perry_hir::realm_reach::scan(&hir, crate::realm_manifest::REALM);
        let plan = crate::realm_plan::RealmPlan::from_reach(&reach).unwrap();
        for flags in [
            0,
            perry_abi::FN_ASYNC,
            perry_abi::FN_GENERATOR,
            perry_abi::FN_ASYNC | perry_abi::FN_GENERATOR | perry_abi::FN_ASYNC_GENERATOR,
        ] {
            let prototype = perry_abi::FunctionProtoKind::of_flags(flags).prototype_name();
            assert!(
                plan.cells.contains_key(prototype),
                "unresolved function prototype: {prototype}"
            );
            let mut cell = plan.cells["Error"].clone();
            cell.function_info.as_mut().unwrap().flags = flags;
            cell.prototype = Some(prototype.into());
            assert!(!function_has_prototype_state(&cell).unwrap());
            cell.prototype = None;
            assert!(function_has_prototype_state(&cell).unwrap());
            assert!(function_requires_dictionary(&cell).unwrap());
        }
    }
}
