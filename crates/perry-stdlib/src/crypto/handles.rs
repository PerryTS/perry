use super::*;

pub struct SignHandle {
    pub(super) alg: RsaDigestKind,
    pub(super) data: std::sync::Mutex<Vec<u8>>,
}

pub struct VerifyHandle {
    pub(super) alg: RsaDigestKind,
    pub(super) data: std::sync::Mutex<Vec<u8>>,
}

pub struct EcdhHandle {
    pub(super) private_key: std::sync::Mutex<Option<P256SecretKey>>,
}

pub struct DiffieHellmanHandle {
    pub(super) prime: Vec<u8>,
    pub(super) generator: Vec<u8>,
    pub(super) private_key: std::sync::Mutex<Option<Vec<u8>>>,
    pub(super) public_key: std::sync::Mutex<Option<Vec<u8>>>,
}

// ───────────────────────────────────────────────────────────────────
// #1367: node:crypto X509Certificate. `new X509Certificate(pem|der)`
// parses the cert and exposes Node's read-only properties. Parsing uses
// RustCrypto's `x509-cert` (the der/spki/const-oid already in the lock).
// ───────────────────────────────────────────────────────────────────
