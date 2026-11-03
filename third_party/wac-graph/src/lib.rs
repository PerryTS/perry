//! A library for defining, encoding, and decoding WebAssembly composition graphs.
//!
//! An example of composing two components together using a `CompositionGraph`:
//!
//! ```rust,no_run
//! use wac_graph::{CompositionGraph, EncodeOptions, types::Package};
//!
//! # fn main() -> anyhow::Result<()> {
//! let mut graph = CompositionGraph::new();
//!
//! // Register the packages with the graph
//! // It is assumed that `my:package1` exports a function named `a`,
//! // while `my:package2` imports a function named `b`.
//! let pkg = Package::from_file("my:package1", None, "package1.wasm", graph.types_mut())?;
//! let package1 = graph.register_package(pkg)?;
//! let pkg = Package::from_file("my:package2", None, "package2.wasm", graph.types_mut())?;
//! let package2 = graph.register_package(pkg)?;
//!
//! // Instantiate package `my:package1`
//! let instantiation1 = graph.instantiate(package1);
//!
//! // Alias the `a` export of the `my:package1` instance
//! let a = graph.alias_instance_export(instantiation1, "a")?;
//!
//! // Instantiate package `my:package2`
//! let instantiation2 = graph.instantiate(package2);
//!
//! // Set argument `b` of the instantiation of `my:package2` to `a`
//! graph.set_instantiation_argument(instantiation2, "b", a)?;
//!
//! // Finally, encode the graph into a new component
//! let bytes = graph.encode(EncodeOptions::default())?;
//!
//! # Ok(())
//! # }
//! ````

#![deny(missing_docs)]

pub(crate) mod encoding;
mod graph;
mod plug;

pub use graph::*;
pub use plug::*;
pub use wac_types as types;
