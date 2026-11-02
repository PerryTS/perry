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

pub(super) fn wasi_030(bytes: Vec<u8>) -> Result<Vec<u8>> {
    let features = WasmFeatures::all() - WasmFeatures::CM_IMPLEMENTS;
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
        .context("Generated guest still requires a post-0.3.0 extern annotation")?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let features = WasmFeatures::all() - WasmFeatures::CM_IMPLEMENTS;
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
