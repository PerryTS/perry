// Adapted from Wasmtime 48.0.5; Apache-2.0 WITH LLVM-exception. See PROVENANCE.md.
use crate::p2::bindings::sockets::instance_network;
use crate::p2::network::Network;
use crate::sockets::WasiSocketsCtxView;
use wasmtime::component::Resource;

impl instance_network::Host for WasiSocketsCtxView<'_> {
    fn instance_network(&mut self) -> Result<Resource<Network>, wasmtime::Error> {
        let network = Network {
            identity: self.ctx.network_identity.clone(),
        };
        let network = self.table.push(network)?;
        Ok(network)
    }
}
