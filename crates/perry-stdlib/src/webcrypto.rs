//! Web Crypto API: `crypto.subtle.digest` / `importKey` / `sign` / `verify`
//! / `encrypt` / `decrypt`.
//!
//! The implementation is split into real Rust submodules so each algorithm
//! family has its own namespace and compilation unit while preserving the
//! public `webcrypto` module ABI expected by generated runtime bindings.
//!
//! `util` declares shared imports, helpers, and private types that are
//! re-exported only inside this module for sibling shards.
mod aes;
mod digest;
mod hmac;
mod jwk;
mod kdf;
mod keys;
mod util;
mod wrap;

#[allow(unused_imports)]
// Private imports keep sibling modules able to share `pub(super)` helpers.
use self::{aes::*, digest::*, hmac::*, jwk::*, kdf::*, keys::*, util::*, wrap::*};

// Public re-exports preserve the parent module surface for FFI entry points.
pub use self::{aes::*, digest::*, hmac::*, jwk::*, kdf::*, keys::*, wrap::*};
