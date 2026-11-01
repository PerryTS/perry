//! Preserve the maintained encoder's shared type aliases and record their
//! source interface instead of treating every public interface as closed.
use super::*;
use wac_graph::types::{Type, Types};
use wasm_encoder::{
    reencode::{ReencodeComponent, RoundtripReencoder},
    InstanceType,
};
use wasmparser::{ComponentAlias, ComponentType, ComponentTypeDeclaration, ComponentTypeRef};

pub(in crate::guest_compose) struct Binding {
    pub interface: InterfaceId,
    pub name: String,
}
pub(in crate::guest_compose) struct Schema {
    pub body: InstanceType,
    pub externals: BTreeMap<u32, Binding>,
}

// Canonical interface IDs alone cannot distinguish two imports of the same
// interface under different labels. Private schemas use distinct local IDs;
// bindings still identify the exact original InterfaceId and instance.
pub(super) fn namespace(types: &Types, api: InterfaceId) -> (Types, HashMap<InterfaceId, String>) {
    fn visit(types: &Types, id: InterfaceId, names: &mut HashMap<InterfaceId, String>) {
        if names.contains_key(&id) {
            return;
        }
        names.insert(id, format!("perry:composition/interface-{}", names.len()));
        for used in types[id].uses.values() {
            visit(types, used.interface, names);
        }
    }
    let mut names = HashMap::new();
    visit(types, api, &mut names);
    let mut cloned = types.clone();
    for (id, name) in &names {
        cloned[*id].id = Some(name.clone());
    }
    (cloned, names)
}

pub(in crate::guest_compose) fn shape(types: &Types, api: InterfaceId) -> Result<Schema> {
    let (types, names) = namespace(types, api);
    let mut graph = CompositionGraph::new();
    *graph.types_mut() = types;
    let resources = graph.types()[api]
        .exports
        .values()
        .filter_map(|kind| {
            if let ItemKind::Type(Type::Resource(id)) = kind {
                Some(*id)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    for resource in resources {
        graph.types_mut()[resource].alias = None;
    }
    graph.define_type("public-interface", Type::Interface(api))?;
    let bytes = graph.encode(EncodeOptions::default())?;
    let mut schema = None;
    for payload in Parser::new(0).parse_all(&bytes) {
        match payload? {
            Payload::ComponentTypeSection(section) => {
                ensure!(
                    section.count() == 1 && schema.is_none(),
                    "Unexpected public interface schema"
                );
                let ComponentType::Component(declarations) = section
                    .into_iter()
                    .next()
                    .context("Missing public interface wrapper type")??
                else {
                    anyhow::bail!("Expected public interface wrapper type");
                };
                let mut type_index = 0;
                let mut instances = Vec::new();
                let mut bodies = BTreeMap::new();
                let mut externals = BTreeMap::new();
                let mut public = None;
                for declaration in declarations {
                    match declaration {
                        ComponentTypeDeclaration::Type(ty) => {
                            if let ComponentType::Instance(declarations) = ty {
                                bodies.insert(
                                    type_index,
                                    RoundtripReencoder.component_instance_type(declarations)?,
                                );
                            }
                            type_index += 1;
                        }
                        ComponentTypeDeclaration::Import(import) => {
                            ensure!(
                                matches!(import.ty, ComponentTypeRef::Instance(_)),
                                "Non-interface schema dependency"
                            );
                            let id = names
                                .iter()
                                .find_map(|(id, name)| (name == import.name.name).then_some(*id))
                                .context("Unknown schema dependency")?;
                            instances.push(id);
                        }
                        ComponentTypeDeclaration::Alias(ComponentAlias::InstanceExport {
                            kind: Kind::Type,
                            instance_index,
                            name,
                        }) => {
                            let interface = *instances
                                .get(usize::try_from(instance_index)?)
                                .context("Invalid schema dependency instance")?;
                            externals.insert(
                                type_index,
                                Binding {
                                    interface,
                                    name: name.to_string(),
                                },
                            );
                            type_index += 1;
                        }
                        ComponentTypeDeclaration::Export {
                            name,
                            ty: ComponentTypeRef::Instance(index),
                        } => {
                            ensure!(
                                name.name == names[&api] && public.is_none(),
                                "Unexpected public schema export"
                            );
                            public = Some(index);
                        }
                        _ => anyhow::bail!("Unsupported public interface schema declaration"),
                    }
                }
                let body = bodies
                    .remove(&public.context("Missing public schema export")?)
                    .context("Missing public instance type")?;
                schema = Some(Schema { body, externals });
            }
            Payload::Version { .. }
            | Payload::End(_)
            | Payload::CustomSection(_)
            | Payload::ComponentExportSection(_) => {}
            _ => anyhow::bail!("Unexpected public interface schema section"),
        }
    }
    schema.context("Missing public interface schema")
}
