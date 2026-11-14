//! Consume the exact physical plan through the existing cell composers.
use super::plan::{ImagePlan, Receiver};
use super::*;
use crate::layout_ordinals::ProgramLayoutOrdinals;

impl ImagePlan {
    pub(crate) fn emit(
        &self,
        module: &mut LlModule,
        ordinals: &ProgramLayoutOrdinals,
    ) -> Result<Vec<TagFixup>, String> {
        if self.realm.cells.is_empty() {
            if ordinals.realm_image {
                return Err("empty realm has an image table".into());
            }
            return Ok(Vec::new());
        }
        if !ordinals.realm_image {
            return Err("realm cells lack an image table".into());
        }
        if !self.realm.native_namespaces.is_empty() {
            return Err("native namespace emission requires its source-owner integration".into());
        }
        run::begin(module);
        let atoms = atoms::RealmAtoms::emit(module, &self.realm)?;
        let mut tags = Vec::new();
        for (name, cell) in &self.realm.cells {
            let meta = self.metadata.cell(name)?;
            let receiver = self
                .receivers
                .get(name)
                .ok_or_else(|| format!("realm {name} lacks receiver facts"))?;
            match receiver {
                Receiver::Object(row) => {
                    tags.extend(objects::emit_object(
                        module,
                        cell,
                        &objects::ObjectFacts {
                            word: row.pending_word(ordinals)?,
                            meta_flags: meta.flags,
                            proto_serial: meta.serial,
                        },
                        &atoms,
                    )?);
                }
                Receiver::Function {
                    function,
                    bag,
                    state,
                } => {
                    let bag_meta = self.metadata.bag(name)?;
                    tags.extend(functions::emit_function(
                        module,
                        cell,
                        &functions::FunctionLayouts {
                            function: function
                                .as_ref()
                                .map_or(Ok(perry_abi::FUNCTION_DICTIONARY_LAYOUT_WORD), |row| {
                                    row.pending_word(ordinals)
                                })?,
                            bag: bag.pending_word(ordinals)?,
                            state: state
                                .as_ref()
                                .map(|row| row.pending_word(ordinals))
                                .transpose()?,
                            bag_flags: bag_meta.flags,
                            bag_serial: bag_meta.serial,
                        },
                        &atoms,
                    )?);
                }
                Receiver::Array { named, slots } => {
                    let parts = arrays::ArrayParts::from_cell(cell)?;
                    let object = named
                        .as_ref()
                        .map(|row| {
                            Ok::<objects::ObjectFacts, String>(objects::ObjectFacts {
                                word: row.pending_word(ordinals)?,
                                meta_flags: meta.flags,
                                proto_serial: meta.serial,
                            })
                        })
                        .transpose()?;
                    let attrs = parts
                        .named
                        .iter()
                        .any(|property| crate::realm_plan::property_entry(property) != 0);
                    let facts = object.as_ref().map(|object| arrays::NamedArrayFacts {
                        object,
                        slots,
                        reserved: header::bag_word(
                            meta.flags
                                & perry_abi::object_meta_flags::OBJECT_META_FLAG_IS_PROTOTYPE
                                != 0,
                            attrs,
                        ),
                    });
                    tags.extend(arrays::emit_array(
                        module,
                        cell,
                        &parts,
                        facts.as_ref(),
                        &atoms,
                    )?);
                }
                Receiver::Set => tags.extend(sets::emit_set(module, cell, &atoms)?),
            }
        }
        run::finish(module)?;
        // Descriptor-owned canonical lists are outside the intrinsic run.
        // Their rows are registered only through the pending receiver arm.
        tags.extend(self.rows.emit(module, &atoms, ordinals)?);
        Ok(tags)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn realm_program_emission_has_one_packed_run_and_reduced_pending_rows() {
        let ast =
            perry_parser::parse_typescript("console.log(globalThis.Map.prototype);", "image.ts")
                .unwrap();
        let index = perry_hir::realm_reach_index::RealmIndex::new(crate::realm_manifest::REALM);
        let reach = perry_hir::realm_reach::scan(&ast, &index);
        for triple in [
            "x86_64-unknown-linux-gnu",
            "aarch64-apple-darwin",
            "x86_64-pc-windows-msvc",
            "wasm32-wasip2",
        ] {
            let image = ImagePlan::new(&reach, triple).unwrap();
            assert!(image.realm.cells.contains_key("%Global%"));
            assert!(image.realm.cells.contains_key("Map.prototype"));
            let preliminary =
                ProgramLayoutOrdinals::assign(crate::layout_ordinals::native_inputs(), Vec::new())
                    .unwrap();
            let ordinals =
                crate::realm_plan::image_ordinals(&preliminary, &reach, Some(triple)).unwrap();
            assert!(ordinals.realm_image);
            assert!(ordinals.lazy_count > 0);
            let mut module = LlModule::new(triple);
            module.add_raw_global("@perry_layout_image = private constant i64 0".into());
            image.emit(&mut module, &ordinals).unwrap();
            let ir = module.to_ir();
            assert_eq!(ir.matches("@perry_realm_run = global").count(), 1);
            assert!(ir.contains("@perry_realm_global = alias"));
            assert!(ir.contains("@perry_realm_run_start = alias"));
            assert!(ir.contains("@perry_realm_run_end = alias"));
            assert!(!ir.contains("call "));
            let context = inkwell::context::Context::create();
            let parsed =
                crate::inprocess::parse_ir_text(&context, &ir, "realm_program_image").unwrap();
            parsed.verify().unwrap();
        }
    }
    #[test]
    fn realm_private_intrinsic_value_is_held_image_data_before_observation() {
        let ast = perry_parser::parse_typescript(
            "console.log(globalThis.RegExp.prototype);",
            "private-image.ts",
        )
        .unwrap();
        let index = perry_hir::realm_reach_index::RealmIndex::new(crate::realm_manifest::REALM);
        let reach = perry_hir::realm_reach::scan(&ast, &index);
        let image = ImagePlan::new(&reach, "x86_64-unknown-linux-gnu").unwrap();
        assert!(image.realm.cells.contains_key("RegExp.prototype"));
        let global = &image.realm.cells["%Global%"];
        let private = global
            .properties
            .iter()
            .filter(|property| {
                crate::realm_plan::property_entry(property) & perry_abi::LAYOUT_KEY_PRIVATE_FIELD
                    == perry_abi::LAYOUT_KEY_PRIVATE_FIELD
            })
            .collect::<Vec<_>>();
        assert_eq!(private.len(), 1);
        assert_eq!(
            private[0].value,
            Some(perry_hir::realm_manifest::RealmValue::Reference(
                "RegExp.prototype".into()
            ))
        );
        let crate::codegen::realm_image::plan::Receiver::Object(row) =
            &image.receivers["RegExp.prototype"]
        else {
            panic!("intrinsic prototype is an object")
        };
        let plan = crate::realm_plan::image_ordinals(
            &ProgramLayoutOrdinals::assign(crate::layout_ordinals::native_inputs(), Vec::new())
                .unwrap(),
            &reach,
            Some("x86_64-unknown-linux-gnu"),
        )
        .unwrap();
        assert_ne!(
            row.pending_word(&plan).unwrap() & perry_abi::LAYOUT_ROW_PENDING,
            0
        );
    }
}
