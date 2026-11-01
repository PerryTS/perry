//! A component root supplies the resource name context required by canonical
//! constructor/method export names. An instance bag of exports has no such
//! context, even when it also contains the corresponding resource type.
use super::*;
use wac_graph::types::InterfaceId;
mod names;
mod shape;
pub(super) use shape::bind_shape;

mod schema;
pub(super) use schema::{shape, Schema};

pub(super) fn compose(
    outer: &mut CompositionGraph,
    api: InterfaceId,
    source: NodeId,
    members: &[(String, NodeId, bool)],
    ordinal: usize,
    sources: &HashMap<InterfaceId, NodeId>,
) -> Result<NodeId> {
    let mut wrapper = CompositionGraph::new();
    let (types, namespace) = schema::namespace(outer.types(), api);
    *wrapper.types_mut() = types;
    let source_name = &namespace[&api];
    let original = wrapper.import(source_name, ItemKind::Instance(api))?;
    let mut bindings = Vec::new();
    for (name, value, forwarded) in members {
        let node = if *forwarded {
            let local = format!("perry-function-{}", bindings.len());
            let kind = wrapper.types()[api].exports[name];
            let node = wrapper.import(&local, kind)?;
            bindings.push((local, *value));
            node
        } else {
            wrapper.alias_instance_export(original, name)?
        };
        wrapper.export(node, name)?;
    }
    // Exporting a resource creates a named alias. Constructor/method function
    // ascriptions must refer to that alias, rather than the imported spelling.
    let bytes = wrapper.encode(EncodeOptions {
        validate: false,
        ..EncodeOptions::default()
    })?;
    let bytes =
        names::rebind(&bytes).context("Invalid typed public-interface forwarding component")?;
    let package = Package::from_bytes(
        &format!("perry:interface-{ordinal}"),
        None,
        bytes,
        outer.types_mut(),
    )?;
    let imports = outer.types()[package.ty()]
        .imports
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    let package = outer.register_package(package)?;
    let instance = outer.instantiate(package);
    outer.set_instantiation_argument(instance, source_name, source)?;
    let forwarded = bindings
        .iter()
        .map(|(name, _)| name.clone())
        .collect::<BTreeSet<_>>();
    for (name, value) in bindings {
        outer.set_instantiation_argument(instance, &name, value)?;
    }
    for name in imports {
        if name == *source_name || forwarded.contains(&name) {
            continue;
        }
        let id = namespace
            .iter()
            .find_map(|(id, local)| (local == &name).then_some(*id))
            .context("Unbound forwarding interface dependency")?;
        let node = *sources
            .get(&id)
            .context("Missing shared interface source")?;
        outer.set_instantiation_argument(instance, &name, node)?;
    }
    Ok(instance)
}
