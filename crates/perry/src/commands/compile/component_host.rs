//! Selection of the optional native Component Model engine. Guest adapters
//! target WASI and never link this native archive.
use super::wasi_flavour::Flavour;
use super::{library_search, types::CompilationContext};
use anyhow::{bail, Result};
use std::path::PathBuf;

pub(super) const REGISTER: &str = "perry_component_host_register";

pub(super) fn library(ctx: &CompilationContext, target: Option<&str>) -> Result<Option<PathBuf>> {
    if target.is_some_and(|t| Flavour::from_target(t).is_some())
        || !ctx.native_module_imports.contains("perry/component")
    {
        return Ok(None);
    }
    let name = if super::is_windows_target(target) {
        "perry_component_host.lib"
    } else {
        "libperry_component_host.a"
    };
    match library_search::find_library(name, target) {
        Some(path) => Ok(Some(path)),
        None => bail!("perry/component requires {name}; build the matching optional engine with cargo build --release -p perry-component-host"),
    }
}
