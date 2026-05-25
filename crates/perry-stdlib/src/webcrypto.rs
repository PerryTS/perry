//! Web Crypto API: `crypto.subtle.digest` / `importKey` / `sign` / `verify`
//! / `encrypt` / `decrypt`.
//!
//! The implementation is split into topical include files while preserving the
//! single public `webcrypto` module ABI expected by runtime bindings.
include!("webcrypto/util.rs");
include!("webcrypto/digest.rs");
include!("webcrypto/jwk.rs");
include!("webcrypto/hmac.rs");
include!("webcrypto/kdf.rs");
include!("webcrypto/aes.rs");
include!("webcrypto/keys.rs");
include!("webcrypto/wrap.rs");
