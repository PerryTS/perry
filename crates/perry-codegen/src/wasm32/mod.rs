//! wasm32 lowering for the standalone WASI target (#11375, #11378).
//!
//! Only compiled with the off-by-default `target-wasi` feature, and only ever
//! applied to a module whose triple is wasm32 — no other target's output can
//! reach anything in here.

mod abi_adapt;

pub(crate) use abi_adapt::adapt_runtime_abi;
