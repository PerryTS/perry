//! Emit 0.3.0 extern names after composition has resolved interface identities.
//! `implements` is a 0.3.1 annotation. Removing it here keeps each label and
//! every nominal type/alias intact; the graph still needs it while composing.
use anyhow::{Context, Result};
use wasm_encoder::reencode::{component_utils, Error, Reencode, ReencodeComponent};
use wasm_encoder::{
    Component, ComponentExportSection, ComponentImportSection, ComponentType, InstanceType,
};
use wasmparser::{
    ComponentExternName, ComponentTypeDeclaration, InstanceTypeDeclaration, Parser, Validator,
    WasmFeatures,
};

struct PlainNames;
impl Reencode for PlainNames {
    type Error = String;
}

fn plain(mut name: ComponentExternName<'_>) -> Result<ComponentExternName<'_>, Error<String>> {
    if name.external_id.is_some() || name.version_suffix.is_some() {
        return Err(Error::UserError(
            "Unsupported generated extern annotation for the 0.3.0 profile".into(),
        ));
    }
    name.implements = None;
    Ok(name)
}

impl ReencodeComponent for PlainNames {
    fn parse_component_import_section(
        &mut self,
        imports: &mut ComponentImportSection,
        section: wasmparser::ComponentImportSectionReader<'_>,
    ) -> Result<(), Error<String>> {
        for import in section {
            let import = import?;
            imports.import(plain(import.name)?, self.component_type_ref(import.ty)?);
        }
        Ok(())
    }

    fn parse_component_export(
        &mut self,
        exports: &mut ComponentExportSection,
        mut export: wasmparser::ComponentExport<'_>,
    ) -> Result<(), Error<String>> {
        export.name = plain(export.name)?;
        component_utils::parse_component_export(self, exports, export)
    }

    fn parse_component_type_declaration(
        &mut self,
        component: &mut ComponentType,
        mut declaration: ComponentTypeDeclaration<'_>,
    ) -> Result<(), Error<String>> {
        match &mut declaration {
            ComponentTypeDeclaration::Import(import) => import.name = plain(import.name)?,
            ComponentTypeDeclaration::Export { name, .. } => *name = plain(*name)?,
            _ => {}
        }
        component_utils::parse_component_type_declaration(self, component, declaration)
    }

    fn parse_component_instance_type_declaration(
        &mut self,
        instance: &mut InstanceType,
        mut declaration: InstanceTypeDeclaration<'_>,
    ) -> Result<(), Error<String>> {
        if let InstanceTypeDeclaration::Export { name, .. } = &mut declaration {
            *name = plain(*name)?;
        }
        component_utils::parse_component_instance_type_declaration(self, instance, declaration)
    }
}

fn features_030() -> WasmFeatures {
    // WASI 0.3.0 adopts only the async Component Model gate. Core Wasm
    // proposals have their own engine requirements; don't restrict them here.
    // Include CM64 in this namespace, alongside the CM_* flags.
    // Keep test-wasi/p3-entropy/contract.rs in agreement (a different parser pin).
    let mut features = WasmFeatures::all();
    for (name, feature) in WasmFeatures::all().iter_names() {
        if name.starts_with("CM") {
            features.set(feature, feature == WasmFeatures::CM_ASYNC);
        }
    }
    features
}

pub(super) fn wasi_030(bytes: Vec<u8>) -> Result<Vec<u8>> {
    let features = features_030();
    if Validator::new_with_features(features)
        .validate_all(&bytes)
        .is_ok()
    {
        return Ok(bytes);
    }
    let mut component = Component::new();
    PlainNames
        .parse_component(&mut component, Parser::new(0), &bytes)
        .map_err(|error| anyhow::anyhow!("Encoding 0.3.0 component extern names: {error}"))?;
    let bytes = component.finish();
    Validator::new_with_features(features)
        .validate_all(&bytes)
        .context("Generated guest requires a Component Model feature outside WASI 0.3.0")?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_are_rejected_even_without_extern_annotations() {
        let bytes = wat::parse_str("(component (type (map string u32)))").unwrap();
        Validator::new_with_features(WasmFeatures::all())
            .validate_all(&bytes)
            .unwrap();
        let error = wasi_030(bytes).unwrap_err();
        assert!(format!("{error:#}").contains("map"));
    }

    #[test]
    fn async_types_and_core_proposals_remain_byte_identical() {
        let bytes = wat::parse_str(
            r#"(component
                (type (future u32))
                (type (stream u32))
                (import "start" (func async))
                (core module (memory i64 1)))"#,
        )
        .unwrap();
        assert_eq!(wasi_030(bytes.clone()).unwrap(), bytes);
    }

    #[test]
    fn annotations_in_nested_components_and_type_declarations_use_plain_names() {
        let original = wat::parse_str(
            r#"(component
            (type $iface (instance (export "item" (type (sub resource)))))
            (type (instance
                (alias outer 1 $iface (type $t))
                (export "nested" (implements "test:labels/foreign") (instance (type $t)))))
            (type (component
                (alias outer 1 $iface (type $t))
                (import "input" (implements "test:labels/foreign") (instance (type $t)))
                (export "output" (implements "test:labels/foreign") (instance (type $t)))))
            (component
                (type $iface (instance (export "item" (type (sub resource)))))
                (import "left" (implements "test:labels/foreign") (instance $left (type $iface)))
                (export "right" (implements "test:labels/foreign") (instance $left))))"#,
        )
        .unwrap();
        Validator::new_with_features(WasmFeatures::all())
            .validate_all(&original)
            .unwrap();
        let features = features_030();
        assert!(Validator::new_with_features(features)
            .validate_all(&original)
            .is_err());
        let bytes = wasi_030(original.clone()).unwrap();
        assert_ne!(bytes, original);
        Validator::new_with_features(features)
            .validate_all(&bytes)
            .unwrap();
        // Already compatible components remain byte-identical, including core modules.
        assert_eq!(wasi_030(bytes.clone()).unwrap(), bytes);
    }
}
