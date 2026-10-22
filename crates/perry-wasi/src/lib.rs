//! Turnloop WASI providers. Migration is in progress: this crate currently
//! implements I/O, clocks, entropy and capability filesystems; it is not yet the host default.
pub mod clocks;
mod error;
pub mod filesystem;
pub use error::{I32Exit, TrappableError};
const MAX_READ_SIZE_ALLOC: usize = 64 * 1024;
pub mod io;
pub mod p2;
pub mod pipe;
pub mod random;

use perry_component_io::Driver;
use wasmtime::component::ResourceTable;

pub struct WasiCtx {
    pub clocks: clocks::WasiClocksCtx,
    pub random: random::WasiRandomCtx,
    pub filesystem: filesystem::WasiFilesystemCtx,
    // Native I/O is stopped when its component context is disposed, regardless
    // of whether a resource's weak submitter still exists in the table.
    driver: Driver,
}
impl WasiCtx {
    pub fn new(task_limit: usize) -> wasmtime::Result<Self> {
        let driver = Driver::new(task_limit)?;
        Ok(Self {
            clocks: clocks::WasiClocksCtx::new(driver.submitter()),
            random: random::WasiRandomCtx::default(),
            filesystem: filesystem::WasiFilesystemCtx::default(),
            driver,
        })
    }

    /// Grant a host directory capability before guest execution. Child opens
    /// retain its permissions and resolve paths relative to this native handle.
    pub fn preopened_dir(
        &mut self,
        host: impl AsRef<std::path::Path>,
        guest: impl Into<String>,
        perms: filesystem::FsPerms,
    ) -> wasmtime::Result<&mut Self> {
        use filesystem::{Dir, FsPerms, OpenMode};
        let dir = filesystem::primitives::open_ambient_dir(host.as_ref())?;
        let mode = if perms == FsPerms::ReadOnly {
            OpenMode::READ
        } else {
            OpenMode::READ | OpenMode::WRITE
        };
        self.filesystem.preopens.push((
            Dir::new(dir, perms, mode, self.driver.submitter()),
            guest.into(),
        ));
        Ok(self)
    }

    pub fn outstanding_io(&self) -> usize {
        self.driver.outstanding()
    }
}

pub struct WasiCtxView<'a> {
    pub ctx: &'a mut WasiCtx,
    pub table: &'a mut ResourceTable,
}

pub trait WasiView: Send {
    fn ctx(&mut self) -> WasiCtxView<'_>;
}

#[cfg(test)]
mod stream_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod filesystem_tests;
