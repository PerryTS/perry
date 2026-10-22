pub mod filesystem;
pub use filesystem::{FsError, FsResult};
mod host {
    pub mod filesystem;
}
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
        imports: {
            "wasi:filesystem/types.[method]descriptor.advise": async | trappable,
            "wasi:filesystem/types.[method]descriptor.create-directory-at": async | trappable,
            "wasi:filesystem/types.[method]descriptor.get-flags": async | trappable,
            "wasi:filesystem/types.[method]descriptor.get-type": async | trappable,
            "wasi:filesystem/types.[method]descriptor.is-same-object": async | trappable,
            "wasi:filesystem/types.[method]descriptor.link-at": async | trappable,
            "wasi:filesystem/types.[method]descriptor.metadata-hash": async | trappable,
            "wasi:filesystem/types.[method]descriptor.metadata-hash-at": async | trappable,
            "wasi:filesystem/types.[method]descriptor.open-at": async | trappable,
            "wasi:filesystem/types.[method]descriptor.read": async | trappable,
            "wasi:filesystem/types.[method]descriptor.read-directory": async | trappable,
            "wasi:filesystem/types.[method]descriptor.readlink-at": async | trappable,
            "wasi:filesystem/types.[method]descriptor.remove-directory-at": async | trappable,
            "wasi:filesystem/types.[method]descriptor.rename-at": async | trappable,
            "wasi:filesystem/types.[method]descriptor.set-size": async | trappable,
            "wasi:filesystem/types.[method]descriptor.set-times": async | trappable,
            "wasi:filesystem/types.[method]descriptor.set-times-at": async | trappable,
            "wasi:filesystem/types.[method]descriptor.stat": async | trappable,
            "wasi:filesystem/types.[method]descriptor.stat-at": async | trappable,
            "wasi:filesystem/types.[method]descriptor.symlink-at": async | trappable,
            "wasi:filesystem/types.[method]descriptor.sync": async | trappable,
            "wasi:filesystem/types.[method]descriptor.sync-data": async | trappable,
            "wasi:filesystem/types.[method]descriptor.unlink-file-at": async | trappable,
            "wasi:filesystem/types.[method]descriptor.write": async | trappable,
            "wasi:filesystem/types.[method]directory-entry-stream.read-directory-entry": async | trappable,
            default: trappable
        },
        trappable_error_type: { "wasi:filesystem/types.error-code" => crate::p2::FsError },
        with: {
            "wasi:io": crate::io::bindings::wasi::io,
            "wasi:filesystem/types.descriptor": crate::filesystem::Descriptor,
            "wasi:filesystem/types.directory-entry-stream": crate::p2::filesystem::ReaddirIterator,
        },
    });
    pub use wasi::*;
    pub mod sync {
        wasmtime::component::bindgen!({
            path: "wit", world: "providers", require_store_data_send: true,
            imports: { default: trappable },
            trappable_error_type: { "wasi:filesystem/types.error-code" => crate::p2::FsError },
            with: {
                "wasi:io": crate::io::sync::wasi::io,
                "wasi:clocks": crate::p2::bindings::clocks,
                "wasi:random": crate::p2::bindings::random,
                "wasi:filesystem/preopens": crate::p2::bindings::filesystem::preopens,
                "wasi:filesystem/types.descriptor": crate::filesystem::Descriptor,
                "wasi:filesystem/types.directory-entry-stream": crate::p2::filesystem::ReaddirIterator,
            },
        });
        pub use wasi::*;
    }
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
    bindings::filesystem::preopens::add_to_linker::<T, crate::filesystem::WasiFilesystem>(
        linker,
        |t| {
            let view = t.ctx();
            crate::filesystem::WasiFilesystemCtxView {
                ctx: &mut view.ctx.filesystem,
                table: view.table,
            }
        },
    )?;
    Ok(())
}

pub fn add_to_linker_sync<T: WasiView + 'static>(linker: &mut Linker<T>) -> wasmtime::Result<()> {
    crate::io::bindings::wasi::io::error::add_to_linker::<T, WasiIo>(linker, |t| t.ctx().table)?;
    crate::io::sync::wasi::io::poll::add_to_linker::<T, WasiIo>(linker, |t| t.ctx().table)?;
    crate::io::sync::wasi::io::streams::add_to_linker::<T, WasiIo>(linker, |t| t.ctx().table)?;
    bindings::sync::filesystem::types::add_to_linker::<T, crate::filesystem::WasiFilesystem>(
        linker,
        |t| {
            let view = t.ctx();
            crate::filesystem::WasiFilesystemCtxView {
                ctx: &mut view.ctx.filesystem,
                table: view.table,
            }
        },
    )?;
    common(linker)
}

pub fn add_to_linker_async<T: WasiView + 'static>(linker: &mut Linker<T>) -> wasmtime::Result<()> {
    crate::io::bindings::wasi::io::error::add_to_linker::<T, WasiIo>(linker, |t| t.ctx().table)?;
    crate::io::bindings::wasi::io::poll::add_to_linker::<T, WasiIo>(linker, |t| t.ctx().table)?;
    crate::io::bindings::wasi::io::streams::add_to_linker::<T, WasiIo>(linker, |t| t.ctx().table)?;
    bindings::filesystem::types::add_to_linker::<T, crate::filesystem::WasiFilesystem>(
        linker,
        |t| {
            let view = t.ctx();
            crate::filesystem::WasiFilesystemCtxView {
                ctx: &mut view.ctx.filesystem,
                table: view.table,
            }
        },
    )?;
    common(linker)
}
