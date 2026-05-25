//! Crypto module
//!
//! Native implementation of Node.js crypto module functions.
//!
//! The implementation is split into topical include files so each algorithm
//! family remains reviewable while preserving the single public `crypto`
//! module ABI expected by generated runtime bindings.
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
