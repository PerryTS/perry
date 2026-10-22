use crate::{
    clocks::{WasiClocks, WasiClocksCtxView},
    random::WasiRandom,
    WasiView,
};
use wasmtime::component::{HasData, Linker, ResourceTable};

pub mod bindings {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "providers",
        require_store_data_send: true,
        imports: { default: trappable },
        with: { "wasi:io": crate::io::bindings::wasi::io },
    });
    pub use wasi::*;
}

struct WasiIo;
impl HasData for WasiIo {
    type Data<'a> = &'a mut ResourceTable;
}

fn common<T: WasiView + 'static>(linker: &mut Linker<T>) -> wasmtime::Result<()> {
    bindings::clocks::wall_clock::add_to_linker::<T, WasiClocks>(linker, |t| {
        let view = t.ctx();
        WasiClocksCtxView {
            ctx: &mut view.ctx.clocks,
            table: view.table,
        }
    })?;
    bindings::clocks::monotonic_clock::add_to_linker::<T, WasiClocks>(linker, |t| {
        let view = t.ctx();
        WasiClocksCtxView {
            ctx: &mut view.ctx.clocks,
            table: view.table,
        }
    })?;
    bindings::random::random::add_to_linker::<T, WasiRandom>(linker, |t| &mut t.ctx().ctx.random)?;
    bindings::random::insecure::add_to_linker::<T, WasiRandom>(linker, |t| {
        &mut t.ctx().ctx.random
    })?;
    bindings::random::insecure_seed::add_to_linker::<T, WasiRandom>(linker, |t| {
        &mut t.ctx().ctx.random
    })?;
    Ok(())
}

pub fn add_to_linker_sync<T: WasiView + 'static>(linker: &mut Linker<T>) -> wasmtime::Result<()> {
    crate::io::bindings::wasi::io::error::add_to_linker::<T, WasiIo>(linker, |t| t.ctx().table)?;
    crate::io::sync::wasi::io::poll::add_to_linker::<T, WasiIo>(linker, |t| t.ctx().table)?;
    crate::io::sync::wasi::io::streams::add_to_linker::<T, WasiIo>(linker, |t| t.ctx().table)?;
    common(linker)
}

pub fn add_to_linker_async<T: WasiView + 'static>(linker: &mut Linker<T>) -> wasmtime::Result<()> {
    crate::io::bindings::wasi::io::error::add_to_linker::<T, WasiIo>(linker, |t| t.ctx().table)?;
    crate::io::bindings::wasi::io::poll::add_to_linker::<T, WasiIo>(linker, |t| t.ctx().table)?;
    crate::io::bindings::wasi::io::streams::add_to_linker::<T, WasiIo>(linker, |t| t.ctx().table)?;
    common(linker)
}
