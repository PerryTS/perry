//! Native WASI 0.3 providers. Clock waits use the component's Turnloop driver.
//! CLI, filesystem, socket and HTTP interfaces are added as they are ported.
pub mod bindings {
    wasmtime::component::bindgen!({
        path: "wit-p3",
        world: "providers",
        require_store_data_send: true,
        imports: { default: trappable },
    });
    pub use wasi::*;
}

#[cfg(test)]
mod canonical_tests;
mod random;
#[cfg(test)]
mod tests;

use crate::{
    clocks::{WasiClocks, WasiClocksCtxView},
    random::WasiRandom,
    WasiView,
};
use wasmtime::component::Linker;

pub fn add_to_linker<T: WasiView + 'static>(linker: &mut Linker<T>) -> wasmtime::Result<()> {
    bindings::clocks::types::add_to_linker::<T, WasiClocks>(linker, |state| {
        let view = state.ctx();
        WasiClocksCtxView {
            ctx: &mut view.ctx.clocks,
            table: view.table,
        }
    })?;
    bindings::clocks::monotonic_clock::add_to_linker::<T, WasiClocks>(linker, |state| {
        let view = state.ctx();
        WasiClocksCtxView {
            ctx: &mut view.ctx.clocks,
            table: view.table,
        }
    })?;
    bindings::clocks::system_clock::add_to_linker::<T, WasiClocks>(linker, |state| {
        let view = state.ctx();
        WasiClocksCtxView {
            ctx: &mut view.ctx.clocks,
            table: view.table,
        }
    })?;
    bindings::random::random::add_to_linker::<T, WasiRandom>(linker, |state| {
        &mut state.ctx().ctx.random
    })?;
    bindings::random::insecure::add_to_linker::<T, WasiRandom>(linker, |state| {
        &mut state.ctx().ctx.random
    })?;
    bindings::random::insecure_seed::add_to_linker::<T, WasiRandom>(linker, |state| {
        &mut state.ctx().ctx.random
    })?;
    Ok(())
}
