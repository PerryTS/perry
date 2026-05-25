//! Web Crypto API: `crypto.subtle.digest` / `importKey` / `sign` / `verify`
//! / `encrypt` / `decrypt`.
//!
//! This file intentionally keeps the public `webcrypto` module ABI intact and
//! uses `include!` shards for a low-risk mechanical split of the former
//! monolith. The file-size gate measures per-file review surface; a follow-up
//! can migrate these shards to real Rust submodules for compile-unit isolation.
//!
//! Include order is load-bearing: `webcrypto/util.rs` declares shared imports,
//! helpers, and private types used by later shards, so it must stay first.
include!("webcrypto/util.rs");
include!("webcrypto/digest.rs");
include!("webcrypto/jwk.rs");
include!("webcrypto/hmac.rs");
include!("webcrypto/kdf.rs");
include!("webcrypto/aes.rs");
include!("webcrypto/keys.rs");
include!("webcrypto/wrap.rs");
