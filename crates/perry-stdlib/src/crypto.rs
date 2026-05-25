//! Crypto module
//!
//! Native implementation of Node.js crypto module functions.
//!
//! The implementation is split into real Rust submodules so each algorithm
//! family has its own namespace and compilation unit while preserving the
//! public `crypto` module ABI expected by generated runtime bindings.
//!
//! `util` declares shared imports, helpers, and private types that are
//! re-exported only inside this module for sibling shards.
mod cipher;
mod ecdh;
mod handles;
mod hash;
mod hash_handles;
mod kdf;
mod keys;
mod prime;
mod random;
mod sign;
mod util;
mod x509;

#[allow(unused_imports)]
// Private imports keep sibling modules able to share `pub(super)` helpers.
use self::{
    cipher::*, ecdh::*, handles::*, hash::*, hash_handles::*, kdf::*, keys::*, prime::*, random::*,
    sign::*, util::*, x509::*,
};

// Public re-exports preserve the parent module surface for FFI entry points.
pub use self::{
    cipher::*, ecdh::*, handles::*, hash::*, hash_handles::*, kdf::*, keys::*, prime::*, random::*,
    sign::*, x509::*,
};
