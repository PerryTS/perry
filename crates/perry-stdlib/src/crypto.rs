//! Crypto module
//!
//! Native implementation of Node.js crypto module functions.
//!
//! This file intentionally keeps the public `crypto` module ABI intact and
//! uses `include!` shards for a low-risk mechanical split of the former
//! monolith. The file-size gate measures per-file review surface; a follow-up
//! can migrate these shards to real Rust submodules for compile-unit isolation.
//!
//! Include order is load-bearing: `crypto/util.rs` declares shared imports,
//! helpers, and private types used by later shards, so it must stay first.
include!("crypto/util.rs");
include!("crypto/hash.rs");
include!("crypto/keys.rs");
include!("crypto/random.rs");
include!("crypto/prime.rs");
include!("crypto/sign.rs");
include!("crypto/handles.rs");
include!("crypto/x509.rs");
include!("crypto/ecdh.rs");
include!("crypto/kdf.rs");
include!("crypto/hash_handles.rs");
include!("crypto/cipher.rs");
