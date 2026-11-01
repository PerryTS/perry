//! A component root supplies the resource name context required by canonical
//! constructor/method export names. An instance bag of exports has no such
//! context, even when it also contains the corresponding resource type.
use super::*;
use wac_graph::types::InterfaceId;
mod names;

pub(super) fn compose(
    outer: &mut CompositionGraph,
    api: InterfaceId,
    source: NodeId,
    members: &[(String, NodeId, bool)],
    ordinal: usize,
) -> Result<NodeId> {
    let mut wrapper = CompositionGraph::new();
    *wrapper.types_mut() = outer.types().clone();
    let source_name = wrapper.types()[api]
        .id
        .clone()
        .unwrap_or_else(|| "perry:original/api".into());
    wrapper.types_mut()[api].id = Some(source_name.clone());
    let original = wrapper.import(&source_name, ItemKind::Instance(api))?;
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
    let package = outer.register_package(package)?;
    let instance = outer.instantiate(package);
    outer.set_instantiation_argument(instance, &source_name, source)?;
    for (name, value) in bindings {
        outer.set_instantiation_argument(instance, &name, value)?;
    }
    Ok(instance)
}
