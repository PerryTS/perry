//! Turnloop WASI providers. Migration is in progress: this crate currently
//! implements I/O bindings, clocks and entropy, and is not yet the host default.
pub mod clocks;
pub mod io;
pub mod p2;
pub mod random;

use perry_component_io::Driver;
use wasmtime::component::ResourceTable;

pub struct WasiCtx {
    pub clocks: clocks::WasiClocksCtx,
    pub random: random::WasiRandomCtx,
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
            driver,
        })
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
mod tests;
