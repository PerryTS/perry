//! Expected instrumentation denial for the plain-runtime probes.
use anyhow::{bail, ensure, Result};
use futures_lite::future::{block_on, poll_once};
use perry_component_host::{Driver, DriverClient};

/// An expected guest bootstrap failure must retire the Store and flush its
/// diagnostics normally. Ordinary probe helpers panic on unexpected errors;
/// their abort profile skips this guard's destructor.
pub fn plain_rejection(driver: &mut Driver, client: &DriverClient) -> Result<()> {
    let mut call = client.call("", "status", &[])?;
    for _ in 0..256 {
        if let Some(reply) = block_on(poll_once(&mut *driver)) {
            let error = match reply {
                Err(error) => error,
                Ok(_) => bail!("instrument denial must fail the driver"),
            };
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
