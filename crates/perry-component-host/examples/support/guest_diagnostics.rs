//! Keep guest diagnostics nonblocking, including module initialization and
//! synchronous exports. Flush only after the guest Store has retired.
use anyhow::{bail, ensure, Result};
use futures_lite::future::{block_on, poll_once};
use perry_component_host::{Driver, DriverClient, WasiOptions};
use perry_wasi::pipe::MemoryOutputPipe;
use std::io::Write;

pub struct GuestDiagnostics(MemoryOutputPipe);

impl Drop for GuestDiagnostics {
    fn drop(&mut self) {
        let _ = std::io::stderr().write_all(&self.0.contents());
    }
}

pub fn configure(wasi: &mut WasiOptions, moving: bool, seed: u64) -> Option<GuestDiagnostics> {
    if !moving {
        return None;
    }
    let output = MemoryOutputPipe::new(16 * 1024 * 1024);
    wasi.inherit_stderr = false;
    wasi.stderr_buffer = Some(output.clone());
    for name in [
        "PERRY_GC_FORCE_EVACUATE",
        "PERRY_GC_VERIFY_EVACUATION",
        "PERRY_GC_DIAG",
    ] {
        wasi.env.insert(name.into(), "1".into());
    }
    // This instrument-specific knob must reject the plain release archive.
    // Basic evacuation/verification knobs alone do not prove that distinction.
    wasi.env
        .insert("PERRY_GC_SCHEDULE_SEED".into(), seed.to_string());
    wasi.env.insert("PERRY_GC_SCHEDULE_RATE".into(), "1".into());
    Some(GuestDiagnostics(output))
}

/// An expected guest bootstrap failure must retire the Store and flush its
/// diagnostics normally. Ordinary probe helpers panic on unexpected errors;
/// their abort profile skips this guard's destructor.
pub fn plain_rejection(driver: &mut Driver, client: &DriverClient) -> Result<()> {
    let mut call = client.call("", "status", &[])?;
    for _ in 0..256 {
        if let Some(reply) = block_on(poll_once(&mut *driver)) {
            let error = reply.expect_err("instrument denial must fail the driver");
            ensure!(!driver.is_active(), "failed Store must retire");
            ensure!(client.call("", "status", &[]).is_err());
            return Err(error.context("plain guest refused instrument-only GC controls"));
        }
        if let Some(reply) = block_on(poll_once(&mut call)) {
            bail!("plain guest did not reject the instrument request: {reply:?}");
        }
    }
    bail!("plain guest did not finish its instrument denial")
}
