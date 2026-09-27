//! Internal `crypto` method names for non-escaping hash/HMAC chains (#11516).
//!
//! `perry_transform::crypto_hash_chain` rewrites a `createHash`/`createHmac`
//! whose value provably never escapes (`crypto.createHash(a).update(x)
//! .digest(e)`, or a block-local `const h = createHash(a)` used only as the
//! receiver of `update`/`digest`) into calls on these names. Codegen lowers
//! them to a digest state held in a stack slot of the current frame, so no
//! native handle id is registered, parked or traced for it.
//!
//! The names start with `__perry` so no user-written `crypto.<name>` can
//! collide with them; they only ever appear on `Expr::NativeModuleRef("crypto")`.

/// `crypto.__perryHashChainInit(algorithm, options?)` -> opaque state value.
pub const CHAIN_INIT_HASH: &str = "__perryHashChainInit";
/// `crypto.__perryHmacChainInit(algorithm, key)` -> opaque state value.
pub const CHAIN_INIT_HMAC: &str = "__perryHmacChainInit";
/// `crypto.__perryHashChainUpdate(state, data, inputEncoding?)` -> `state`.
pub const CHAIN_UPDATE: &str = "__perryHashChainUpdate";
/// `crypto.__perryHashChainDigest(state, outputEncoding?)` -> digest.
pub const CHAIN_DIGEST: &str = "__perryHashChainDigest";

/// True for any of the internal chain method names above.
pub fn is_chain_method(name: &str) -> bool {
    matches!(
        name,
        CHAIN_INIT_HASH | CHAIN_INIT_HMAC | CHAIN_UPDATE | CHAIN_DIGEST
    )
}
