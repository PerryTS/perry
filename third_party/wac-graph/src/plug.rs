use crate::{
    types::{are_semver_compatible, SubtypeChecker},
    CompositionGraph, PackageId,
};
use thiserror::Error;

/// Represents an error that can occur when plugging components together.
#[derive(Debug, Error)]
pub enum PlugError {
    /// The socket component had no matching imports for the plugs that were provided
    #[error("the socket component had no matching imports for the plugs that were provided")]
    NoPlugHappened,
    /// An error occurred when building the composition graph
    #[error("an error occurred when building the composition graph")]
    GraphError {
        /// The underlying graph error
        #[source]
        source: anyhow::Error,
    },
}

/// Take the exports of the plug components and plug them into the socket component.
///
/// Note that the `PackageId`s in `plugs` and `socket` must be registered with the `graph`
/// before calling this function.
pub fn plug(
    graph: &mut CompositionGraph,
    plugs: Vec<PackageId>,
    socket: PackageId,
) -> Result<(), PlugError> {
    let socket_instantiation = graph.instantiate(socket);

    for plug in plugs {
        // Collect matching (plug_export_name, socket_import_name) pairs.
        // The names may differ when matched via semver compatibility.
        let mut plug_exports: Vec<(String, String)> = Vec::new();
        let mut cache = Default::default();
        let mut checker = SubtypeChecker::new(&mut cache);
        for (name, plug_ty) in &graph.types()[graph[plug].ty()].exports {
            // Try exact name match first, then fall back to semver-compatible match
            let matching_import = graph.types()[graph[socket].ty()]
                .imports
                .get(name)
                .map(|ty| (name.clone(), ty))
                .or_else(|| {
                    graph.types()[graph[socket].ty()]
                        .imports
                        .iter()
                        .find(|(import_name, _)| are_semver_compatible(name, import_name))
                        .map(|(import_name, ty)| (import_name.clone(), ty))
                });

            if let Some((socket_name, socket_ty)) = matching_import {
                if checker
                    .is_subtype(*plug_ty, graph.types(), *socket_ty, graph.types())
                    .is_ok()
                {
                    plug_exports.push((name.clone(), socket_name));
                }
            }
        }

        // Instantiate the plug component
        let mut plug_instantiation = None;
        for (plug_name, socket_name) in plug_exports {
            log::debug!("using export `{plug_name}` for plug");
            let plug_instantiation =
                *plug_instantiation.get_or_insert_with(|| graph.instantiate(plug));
            let export = graph
                .alias_instance_export(plug_instantiation, &plug_name)
                .map_err(|err| PlugError::GraphError { source: err.into() })?;
            graph
                .set_instantiation_argument(socket_instantiation, &socket_name, export)
                .map_err(|err| PlugError::GraphError { source: err.into() })?;
        }
    }

    // Check we've actually done any plugging.
    if graph
        .get_instantiation_arguments(socket_instantiation)
        .next()
        .is_none()
    {
        return Err(PlugError::NoPlugHappened);
    }

    // Export all exports from the socket component.
    for name in graph.types()[graph[socket].ty()]
        .exports
        .keys()
        .cloned()
        .collect::<Vec<_>>()
    {
        let export = graph
            .alias_instance_export(socket_instantiation, &name)
            .map_err(|err| PlugError::GraphError { source: err.into() })?;

        graph
            .export(export, &name)
            .map_err(|err| PlugError::GraphError { source: err.into() })?;
    }

    Ok(())
}
