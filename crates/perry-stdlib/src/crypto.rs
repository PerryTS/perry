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
mod certificate;
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
pub(crate) mod util;
mod x509;

// Private imports keep sibling modules able to share `pub(super)` helpers.
use self::{cipher::*, kdf::*, keys::*, random::*, util::*, x509::*};

// Public re-exports preserve the parent module surface for FFI entry points.
pub use self::{
    certificate::*, cipher::*, ecdh::*, handles::*, hash::*, hash_handles::*, kdf::*, keys::*,
    prime::*, random::*, sign::*, x509::*,
};
