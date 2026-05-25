// Included from `crypto.rs`; shares that module's imports, helpers, and private namespace.

pub struct SignHandle {
    alg: RsaDigestKind,
    data: std::sync::Mutex<Vec<u8>>,
}

pub struct VerifyHandle {
    alg: RsaDigestKind,
    data: std::sync::Mutex<Vec<u8>>,
}

pub struct EcdhHandle {
    private_key: std::sync::Mutex<Option<P256SecretKey>>,
}

pub struct DiffieHellmanHandle {
    prime: Vec<u8>,
    generator: Vec<u8>,
    private_key: std::sync::Mutex<Option<Vec<u8>>>,
    public_key: std::sync::Mutex<Option<Vec<u8>>>,
}

// ───────────────────────────────────────────────────────────────────
// #1367: node:crypto X509Certificate. `new X509Certificate(pem|der)`
// parses the cert and exposes Node's read-only properties. Parsing uses
// RustCrypto's `x509-cert` (the der/spki/const-oid already in the lock).
// ───────────────────────────────────────────────────────────────────

