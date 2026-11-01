//! Rebind method/constructor type ascriptions to exported resource aliases.
//! Imports retain their original types and nominal identity; the validator
//! checks every ascription against the function being forwarded.
use super::*;
use wasm_encoder::{ComponentTypeRef, ComponentTypeSection, ComponentValType as EncVal};
use wasmparser::{
    component_types::{ComponentAnyTypeId, ComponentDefinedType, ComponentValType},
    types::Types,
};

fn value(types: &Types, ty: ComponentValType) -> Result<EncVal> {
    Ok(match ty {
        ComponentValType::Primitive(p) => EncVal::Primitive(p.into()),
        ComponentValType::Type(id) => EncVal::Type(
            (0..types.as_ref().component_type_count())
                .find(|i| types.component_any_type_at(*i) == ComponentAnyTypeId::Defined(id))
                .context("Forwarded value type has no enclosing alias")?,
        ),
    })
}

pub(super) fn rebind(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut output = Component::new();
    let mut exports = Vec::new();
    let mut depth = 0;
    for payload in Parser::new(0).parse_all(bytes) {
        let payload = payload?;
        if depth > 0 {
            match payload {
                Payload::ModuleSection { .. } | Payload::ComponentSection { .. } => depth += 1,
                Payload::End(_) => depth -= 1,
                _ => {}
            }
            continue;
        }
        if let Payload::ComponentExportSection(section) = payload {
            for export in section {
                let export = export?;
                ensure!(
                    export.ty.is_none(),
                    "Unexpected interface export ascription"
                );
                exports.push((export.name.name.to_owned(), export.kind, export.index));
            }
            continue;
        }
        if let Some((id, range)) = payload.as_section() {
            output.section(&RawSection {
                id,
                data: bytes
                    .get(usize::try_from(range.start)?..usize::try_from(range.end)?)
                    .context("Invalid interface section range")?,
            });
        }
        if matches!(
            payload,
            Payload::ModuleSection { .. } | Payload::ComponentSection { .. }
        ) {
            depth += 1;
        }
    }
    let prefix =
        Validator::new_with_features(WasmFeatures::all()).validate_all(output.as_slice())?;
    let mut next = prefix.as_ref().component_type_count();
    let mut resources = BTreeMap::new();
    let mut section = ComponentExportSection::new();
    for (name, kind, index) in &exports {
        if *kind != Kind::Type {
            continue;
        }
        if matches!(
            prefix.component_any_type_at(*index),
            ComponentAnyTypeId::Resource(_)
        ) {
            resources.insert(name.as_str(), next);
        }
        section.export(name.as_str(), (*kind).into(), *index, None);
        next += 1;
    }
    output.section(&section);
    let types =
        Validator::new_with_features(WasmFeatures::all()).validate_all(output.as_slice())?;
    for (name, kind, index) in &exports {
        if *kind == Kind::Type {
            continue;
        }
        let resource = name
            .strip_prefix("[constructor]")
            .map(|name| (true, name))
            .or_else(|| {
                name.strip_prefix("[method]")?
                    .split_once('.')
                    .map(|(r, _)| (false, r))
            });
        let ascription = if let Some((constructor, resource)) = resource {
            ensure!(*kind == Kind::Func, "Resource method is not a function");
            let resource = *resources
                .get(resource)
                .context("Missing named exported resource")?;
            let function = &types[types.component_function_at(*index)];
            let mut params = function
                .params
                .iter()
                .map(|(name, ty)| Ok((name.as_str(), value(&types, *ty)?)))
                .collect::<Result<Vec<_>>>()?;
            let mut result = function.result.map(|ty| value(&types, ty)).transpose()?;
            let mut definitions = ComponentTypeSection::new();
            let handle = next;
            next += 1;
            if constructor {
                definitions.defined_type().own(resource);
                let Some(ComponentValType::Type(id)) = function.result else {
                    anyhow::bail!("Constructor has no owned result");
                };
                result = Some(match &types[id] {
                    ComponentDefinedType::Own(_) => EncVal::Type(handle),
                    ComponentDefinedType::Result {
                        ok: Some(ComponentValType::Type(own)),
                        err,
                    } if matches!(types[*own], ComponentDefinedType::Own(_)) => {
                        definitions.defined_type().result(
                            Some(EncVal::Type(handle)),
                            err.map(|ty| value(&types, ty)).transpose()?,
                        );
                        let result = EncVal::Type(next);
                        next += 1;
                        result
                    }
                    _ => anyhow::bail!("Constructor does not return an owned resource"),
                });
            } else {
                ensure!(!params.is_empty(), "Method has no borrowed receiver");
                definitions.defined_type().borrow(resource);
                params[0].1 = EncVal::Type(handle);
            }
            let signature = next;
            next += 1;
            definitions
                .function()
                .async_(function.async_)
                .params(params)
                .result(result);
            output.section(&definitions);
            Some(ComponentTypeRef::Func(signature))
        } else {
            None
        };
        let mut section = ComponentExportSection::new();
        section.export(name.as_str(), (*kind).into(), *index, ascription);
        output.section(&section);
    }
    let bytes = output.finish();
    Validator::new_with_features(WasmFeatures::all()).validate_all(&bytes)?;
    Ok(bytes)
}
