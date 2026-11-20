// Adapted from Wasmtime 48.0.5 (Bytecode Alliance contributors).
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
// See PROVENANCE.md and LICENSE.

use crate::io::{bindings::wasi::io::poll as async_poll, poll::DynPollable};
use futures_lite::future::block_on;

use wasmtime::component::{Resource, ResourceTable};
use wasmtime::Result;

impl crate::io::sync::wasi::io::poll::Host for ResourceTable {
    fn poll(&mut self, pollables: Vec<Resource<DynPollable>>) -> Result<Vec<u32>> {
        block_on(async { async_poll::Host::poll(self, pollables).await })
    }
}

impl crate::io::sync::wasi::io::poll::HostPollable for ResourceTable {
    fn ready(&mut self, pollable: Resource<DynPollable>) -> Result<bool> {
        block_on(async { async_poll::HostPollable::ready(self, pollable).await })
    }
    fn block(&mut self, pollable: Resource<DynPollable>) -> Result<()> {
        block_on(async { async_poll::HostPollable::block(self, pollable).await })
    }
    fn drop(&mut self, pollable: Resource<DynPollable>) -> Result<()> {
        async_poll::HostPollable::drop(self, pollable)
    }
}
