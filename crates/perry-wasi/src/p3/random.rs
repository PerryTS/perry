use crate::p2::bindings::random as p2;
use crate::p3::bindings::random::{insecure, insecure_seed, random};
use crate::random::WasiRandomCtx;

fn bounded_len(ctx: &WasiRandomCtx, max_len: u64) -> wasmtime::Result<u64> {
    wasmtime::ensure!(
        max_len == 0 || ctx.max_size > 0,
        "random byte capacity must be positive"
    );
    // Unlike P2, P3 explicitly permits short reads. Never turn an untrusted
    // u64 maximum into a large host allocation or reject a representable read.
    Ok(max_len
        .min(ctx.max_size)
        .min(crate::MAX_READ_SIZE_ALLOC as u64))
}

impl random::Host for WasiRandomCtx {
    fn get_random_bytes(&mut self, max_len: u64) -> wasmtime::Result<Vec<u8>> {
        let len = bounded_len(self, max_len)?;
        p2::random::Host::get_random_bytes(self, len)
    }

    fn get_random_u64(&mut self) -> wasmtime::Result<u64> {
        p2::random::Host::get_random_u64(self)
    }
}

impl insecure::Host for WasiRandomCtx {
    fn get_insecure_random_bytes(&mut self, max_len: u64) -> wasmtime::Result<Vec<u8>> {
        let len = bounded_len(self, max_len)?;
        p2::insecure::Host::get_insecure_random_bytes(self, len)
    }

    fn get_insecure_random_u64(&mut self) -> wasmtime::Result<u64> {
        p2::insecure::Host::get_insecure_random_u64(self)
    }
}

impl insecure_seed::Host for WasiRandomCtx {
    fn get_insecure_seed(&mut self) -> wasmtime::Result<(u64, u64)> {
        p2::insecure_seed::Host::insecure_seed(self)
    }
}
