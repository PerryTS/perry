//! ECDSA over the three NIST curves Node signs with through `createSign`,
//! `createVerify`, `crypto.sign` and `crypto.verify`: P-256 (`prime256v1`),
//! P-384 (`secp384r1`) and P-521 (`secp521r1`).
//!
//! Every native path used to accept only P-256 PKCS#8 keys. A P-384/P-521 key
//! was classified as nothing, so `createPrivateKey` returned an unmarked string
//! and jsonwebtoken's ES384/ES512 rejected it with "secretOrPrivateKey must be
//! an asymmetric key". SEC1 (`-----BEGIN EC PRIVATE KEY-----`) keys, which is
//! what `openssl ecparam -genkey` writes, were rejected the same way.
//!
//! Node hashes with the algorithm named at `createSign(alg)` time — the curve
//! does not pick the digest — so signing works on a prehash of that digest.

use super::*;
use p256::ecdsa::signature::hazmat::{PrehashSigner, PrehashVerifier, RandomizedPrehashSigner};
use p256::pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePublicKey};

/// `asymmetric_key_meta` type ids for EC keys: 2 keeps its historical P-256
/// meaning; the runtime's `asymmetricKeyType`/`asymmetricKeyDetails` getters
/// map 5 and 6 to `"ec"` with the matching `namedCurve`.
pub(crate) const ASYM_EC_P256: u8 = 2;
pub(crate) const ASYM_EC_P384: u8 = 5;
pub(crate) const ASYM_EC_P521: u8 = 6;

pub(crate) enum EcSigningKey {
    P256(p256::SecretKey),
    P384(p384::SecretKey),
    P521(p521::SecretKey),
}

pub(crate) enum EcPublicKey {
    P256(p256::PublicKey),
    P384(p384::PublicKey),
    P521(p521::PublicKey),
}

/// A private EC key in PKCS#8 or SEC1 PEM form.
pub(crate) fn parse_ec_signing_key_pem(pem: &str) -> Option<EcSigningKey> {
    if let Ok(k) =
        p256::SecretKey::from_pkcs8_pem(pem).or_else(|_| p256::SecretKey::from_sec1_pem(pem))
    {
        return Some(EcSigningKey::P256(k));
    }
    if let Ok(k) =
        p384::SecretKey::from_pkcs8_pem(pem).or_else(|_| p384::SecretKey::from_sec1_pem(pem))
    {
        return Some(EcSigningKey::P384(k));
    }
    if let Ok(k) =
        p521::SecretKey::from_pkcs8_pem(pem).or_else(|_| p521::SecretKey::from_sec1_pem(pem))
    {
        return Some(EcSigningKey::P521(k));
    }
    None
}

/// A public EC key from SPKI PEM, or the public half of a private EC key PEM
/// (Node's `createVerify().verify(privateKey, …)` accepts either).
pub(crate) fn parse_ec_public_key_pem(pem: &str) -> Option<EcPublicKey> {
    if let Ok(k) = p256::PublicKey::from_public_key_pem(pem) {
        return Some(EcPublicKey::P256(k));
    }
    if let Ok(k) = p384::PublicKey::from_public_key_pem(pem) {
        return Some(EcPublicKey::P384(k));
    }
    if let Ok(k) = p521::PublicKey::from_public_key_pem(pem) {
        return Some(EcPublicKey::P521(k));
    }
    parse_ec_signing_key_pem(pem).map(|k| k.public_key())
}

impl EcSigningKey {
    pub(crate) fn asym_type(&self) -> u8 {
        match self {
            EcSigningKey::P256(_) => ASYM_EC_P256,
            EcSigningKey::P384(_) => ASYM_EC_P384,
            EcSigningKey::P521(_) => ASYM_EC_P521,
        }
    }

    fn field_len(&self) -> usize {
        match self {
            EcSigningKey::P256(_) => 32,
            EcSigningKey::P384(_) => 48,
            EcSigningKey::P521(_) => 66,
        }
    }

    pub(crate) fn public_key(&self) -> EcPublicKey {
        match self {
            EcSigningKey::P256(k) => EcPublicKey::P256(k.public_key()),
            EcSigningKey::P384(k) => EcPublicKey::P384(k.public_key()),
            EcSigningKey::P521(k) => EcPublicKey::P521(k.public_key()),
        }
    }

    /// Sign `data` hashed with `alg`. DER by default, raw `r || s` for
    /// `dsaEncoding: 'ieee-p1363'`.
    pub(crate) fn sign(&self, alg: RsaDigestKind, data: &[u8], p1363: bool) -> Option<Vec<u8>> {
        let prehash = ecdsa_prehash(alg, data, self.field_len());
        Some(match self {
            EcSigningKey::P256(k) => {
                let sig: p256::ecdsa::Signature = p256::ecdsa::SigningKey::from(k)
                    .sign_prehash(&prehash)
                    .ok()?;
                encode_signature(sig.to_bytes().as_slice(), sig.to_der().as_bytes(), p1363)
            }
            EcSigningKey::P384(k) => {
                let sig: p384::ecdsa::Signature = p384::ecdsa::SigningKey::from(k)
                    .sign_prehash(&prehash)
                    .ok()?;
                encode_signature(sig.to_bytes().as_slice(), sig.to_der().as_bytes(), p1363)
            }
            EcSigningKey::P521(k) => {
                let signer = p521::ecdsa::SigningKey::from_bytes(&k.to_bytes()).ok()?;
                let sig: p521::ecdsa::Signature = signer
                    .sign_prehash_with_rng(&mut rand_core_06::OsRng, &prehash)
                    .ok()?;
                encode_signature(sig.to_bytes().as_slice(), sig.to_der().as_bytes(), p1363)
            }
        })
    }
}

/// `alg`'s digest of `data`, left-padded with zeros to half the curve's field
/// size. The `ecdsa` crate rejects a prehash shorter than that, while OpenSSL
/// (and so Node) signs SHA-256 with P-521 or SHA-1 with P-384. ECDSA reads a
/// digest shorter than the field as a plain integer, so leading zeros do not
/// change the value that is signed.
fn ecdsa_prehash(alg: RsaDigestKind, data: &[u8], field_len: usize) -> Vec<u8> {
    let digest = alg.digest(data);
    let min = field_len / 2;
    if digest.len() >= min {
        return digest;
    }
    let mut padded = vec![0u8; min - digest.len()];
    padded.extend_from_slice(&digest);
    padded
}

fn encode_signature(raw: &[u8], der: &[u8], p1363: bool) -> Vec<u8> {
    if p1363 {
        raw.to_vec()
    } else {
        der.to_vec()
    }
}

impl EcPublicKey {
    pub(crate) fn to_public_key_pem(&self) -> Option<String> {
        let pem = match self {
            EcPublicKey::P256(k) => k.to_public_key_pem(Default::default()),
            EcPublicKey::P384(k) => k.to_public_key_pem(Default::default()),
            EcPublicKey::P521(k) => k.to_public_key_pem(Default::default()),
        };
        pem.ok()
    }

    fn field_len(&self) -> usize {
        match self {
            EcPublicKey::P256(_) => 32,
            EcPublicKey::P384(_) => 48,
            EcPublicKey::P521(_) => 66,
        }
    }

    pub(crate) fn asym_type(&self) -> u8 {
        match self {
            EcPublicKey::P256(_) => ASYM_EC_P256,
            EcPublicKey::P384(_) => ASYM_EC_P384,
            EcPublicKey::P521(_) => ASYM_EC_P521,
        }
    }

    pub(crate) fn verify(&self, alg: RsaDigestKind, data: &[u8], sig: &[u8], p1363: bool) -> bool {
        let prehash = ecdsa_prehash(alg, data, self.field_len());
        match self {
            EcPublicKey::P256(k) => {
                let sig = if p1363 {
                    p256::ecdsa::Signature::from_slice(sig)
                } else {
                    p256::ecdsa::Signature::from_der(sig)
                };
                sig.is_ok_and(|sig| {
                    p256::ecdsa::VerifyingKey::from(k)
                        .verify_prehash(&prehash, &sig)
                        .is_ok()
                })
            }
            EcPublicKey::P384(k) => {
                let sig = if p1363 {
                    p384::ecdsa::Signature::from_slice(sig)
                } else {
                    p384::ecdsa::Signature::from_der(sig)
                };
                sig.is_ok_and(|sig| {
                    p384::ecdsa::VerifyingKey::from(k)
                        .verify_prehash(&prehash, &sig)
                        .is_ok()
                })
            }
            EcPublicKey::P521(k) => {
                use p521::elliptic_curve::sec1::ToEncodedPoint as _;
                let sig = if p1363 {
                    p521::ecdsa::Signature::from_slice(sig)
                } else {
                    p521::ecdsa::Signature::from_der(sig)
                };
                let Ok(verifier) =
                    p521::ecdsa::VerifyingKey::from_encoded_point(&k.to_encoded_point(false))
                else {
                    return false;
                };
                sig.is_ok_and(|sig| verifier.verify_prehash(&prehash, &sig).is_ok())
            }
        }
    }
}

/// `asymmetric_key_meta` type id for a Node `namedCurve` spelling.
pub(crate) fn ec_curve_asym_type(name: &str) -> Option<u8> {
    match name.to_ascii_lowercase().as_str() {
        "prime256v1" | "secp256r1" | "p-256" => Some(ASYM_EC_P256),
        "secp384r1" | "p-384" => Some(ASYM_EC_P384),
        "secp521r1" | "p-521" => Some(ASYM_EC_P521),
        _ => None,
    }
}

/// A fresh P-384 or P-521 key pair as `(private PKCS#8 PEM, public SPKI PEM)`.
/// P-256 keeps its existing generator (it also serves the JWK encodings).
pub(crate) fn generate_ec_pem_pair(asym_type: u8) -> Option<(String, String)> {
    use p256::pkcs8::EncodePrivateKey;
    let mut rng = rand_core_06::OsRng;
    let (private, public) = match asym_type {
        ASYM_EC_P384 => {
            let k = p384::SecretKey::random(&mut rng);
            (
                k.to_pkcs8_pem(Default::default()).ok()?.to_string(),
                k.public_key().to_public_key_pem(Default::default()).ok()?,
            )
        }
        ASYM_EC_P521 => {
            let k = p521::SecretKey::random(&mut rng);
            (
                k.to_pkcs8_pem(Default::default()).ok()?.to_string(),
                k.public_key().to_public_key_pem(Default::default()).ok()?,
            )
        }
        _ => return None,
    };
    Some((private, public))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DATA: &[u8] = b"header.payload";

    fn round_trip(key: EcSigningKey, expected_type: u8) {
        assert_eq!(key.asym_type(), expected_type);
        let public = key.public_key();
        assert_eq!(public.asym_type(), expected_type);
        let pem = public.to_public_key_pem().expect("public PEM");
        let reparsed = parse_ec_public_key_pem(&pem).expect("reparse SPKI");
        for alg in [
            RsaDigestKind::Sha1,
            RsaDigestKind::Sha224,
            RsaDigestKind::Sha256,
            RsaDigestKind::Sha384,
            RsaDigestKind::Sha512,
        ] {
            for p1363 in [false, true] {
                let sig = key
                    .sign(alg, DATA, p1363)
                    .expect("every digest signs on every curve");
                assert!(reparsed.verify(alg, DATA, &sig, p1363));
                assert!(!reparsed.verify(alg, b"tampered", &sig, p1363));
            }
        }
    }

    #[test]
    fn every_nist_curve_signs_and_verifies_with_every_digest() {
        use p256::pkcs8::EncodePrivateKey as _;
        let mut rng = rand_core_06::OsRng;
        let k256 = p256::SecretKey::random(&mut rng);
        let k384 = p384::SecretKey::random(&mut rng);
        let k521 = p521::SecretKey::random(&mut rng);
        let pem256 = k256.to_pkcs8_pem(Default::default()).unwrap();
        let pem384 = k384.to_sec1_pem(Default::default()).unwrap();
        let pem521 = k521.to_pkcs8_pem(Default::default()).unwrap();
        round_trip(parse_ec_signing_key_pem(&pem256).unwrap(), ASYM_EC_P256);
        round_trip(parse_ec_signing_key_pem(&pem384).unwrap(), ASYM_EC_P384);
        round_trip(parse_ec_signing_key_pem(&pem521).unwrap(), ASYM_EC_P521);
        for t in [ASYM_EC_P384, ASYM_EC_P521] {
            let (private, public) = generate_ec_pem_pair(t).unwrap();
            assert_eq!(parse_ec_signing_key_pem(&private).unwrap().asym_type(), t);
            assert_eq!(parse_ec_public_key_pem(&public).unwrap().asym_type(), t);
        }
        assert_eq!(ec_curve_asym_type("P-384"), Some(ASYM_EC_P384));
        assert_eq!(ec_curve_asym_type("secp521r1"), Some(ASYM_EC_P521));
        assert_eq!(ec_curve_asym_type("prime256v1"), Some(ASYM_EC_P256));
    }

    #[test]
    fn a_curve_mismatch_does_not_verify() {
        let mut rng = rand_core_06::OsRng;
        let signer = EcSigningKey::P384(p384::SecretKey::random(&mut rng));
        let other = EcSigningKey::P384(p384::SecretKey::random(&mut rng)).public_key();
        let sig = signer.sign(RsaDigestKind::Sha384, DATA, false).unwrap();
        assert!(!other.verify(RsaDigestKind::Sha384, DATA, &sig, false));
    }
}
