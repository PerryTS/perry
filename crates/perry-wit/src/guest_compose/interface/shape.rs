//! Public interface annotations use the producer's concrete resource aliases.
//! Only the schema's local type indices change when outer aliases are added.
use super::*;
use wasm_encoder::{
    reencode::{Reencode, ReencodeComponent},
    Alias, ComponentOuterAliasKind, ComponentTypeRef, ComponentTypeSection, InstanceType,
    TypeBounds,
};
use wasmparser::{ComponentType, ComponentTypeRef as ParsedRef, InstanceTypeDeclaration};

struct Indices {
    types: Vec<u32>,
    invalid: bool,
}
impl Reencode for Indices {
    type Error = std::convert::Infallible;
}
impl ReencodeComponent for Indices {
    fn component_type_index(&mut self, index: u32) -> u32 {
        self.types.get(index as usize).copied().unwrap_or_else(|| {
            self.invalid = true;
            0
        })
    }
}

pub(in super::super) fn bind_shape(
    shape: &InstanceType,
    resources: &BTreeMap<String, u32>,
) -> Result<InstanceType> {
    let mut encoded = Component::new();
    let mut section = ComponentTypeSection::new();
    section.instance(shape);
    encoded.section(&section);
    let bytes = encoded.finish();
    let mut declarations = None;
    for payload in Parser::new(0).parse_all(&bytes) {
        if let Payload::ComponentTypeSection(section) = payload? {
            let Some(Ok(ComponentType::Instance(items))) = section.into_iter().next() else {
                anyhow::bail!("Missing public instance schema");
            };
            declarations = Some(items);
        }
    }
    let mut output = InstanceType::new();
    let mut indices = Indices {
        types: Vec::new(),
        invalid: false,
    };
    let mut used = BTreeSet::new();
    for declaration in declarations.context("Missing public instance declarations")? {
        let old = output.type_count();
        if let InstanceTypeDeclaration::Export {
            name,
            ty: ParsedRef::Type(wasmparser::TypeBounds::SubResource),
        } = &declaration
        {
            let source = *resources
                .get(name.name)
                .context("Missing producer resource binding")?;
            used.insert(name.name.to_string());
            output.alias(Alias::Outer {
                kind: ComponentOuterAliasKind::Type,
                count: 1,
                index: source,
            });
            output.export(name.name, ComponentTypeRef::Type(TypeBounds::Eq(old)));
            indices.types.push(old + 1);
        } else {
            // The maintained round-trip encoder owns all payload layouts.
            // Source schema references can only point to preceding indices.
            indices.parse_component_instance_type_declaration(&mut output, declaration)?;
            if output.type_count() != old {
                ensure!(
                    output.type_count() == old + 1,
                    "Unexpected schema type expansion"
                );
                indices.types.push(old);
            }
        }
    }
    ensure!(
        !indices.invalid,
        "Invalid public instance schema type reference"
    );
    ensure!(
        used.len() == resources.len(),
        "Unused producer resource binding"
    );
    Ok(output)
}
