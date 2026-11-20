//! Connect the versioned logger directly, preserving the Perry exports.
use anyhow::{ensure, Result};
use wac_graph::{types::Package, CompositionGraph, EncodeOptions};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 3,
        "usage: compose <perry.wasm> <logger.wasm> <output.wasm>"
    );
    let mut graph = CompositionGraph::new();
    let perry = Package::from_file("perry:library", None, &args[0], graph.types_mut())?;
    let perry = graph.register_package(perry)?;
    let logger = Package::from_file("perry:logger", None, &args[1], graph.types_mut())?;
    let logger = graph.register_package(logger)?;
    let logger = graph.instantiate(logger);
    let logging = graph.alias_instance_export(logger, "perry:transform/logging@1.0.0")?;
    let perry = graph.instantiate(perry);
    graph.set_instantiation_argument(perry, "perry:transform/logging@1.0.0", logging)?;
    let transform = graph.alias_instance_export(perry, "perry:transform/transform@1.0.0")?;
    graph.export(transform, "perry:transform/transform@1.0.0")?;
    std::fs::write(&args[2], graph.encode(EncodeOptions::default())?)?;
    Ok(())
}
