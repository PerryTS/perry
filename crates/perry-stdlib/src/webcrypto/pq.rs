//! Post-quantum key material shared by WebCrypto and `node:crypto`:
//! ML-DSA (FIPS 204) signatures, and one view over both "AKP" families
//! (ML-KEM and ML-DSA) for the import / export paths that treat them alike.
//!
//! Like ML-KEM, an ML-DSA key is stored as DER: seed-only PKCS#8 for a private
//! key, SubjectPublicKeyInfo for a public key. That is also what node exports.

use super::*;

use ml_dsa::pkcs8::{
    DecodePrivateKey as MlDsaDecodePrivateKey, DecodePublicKey as MlDsaDecodePublicKey,
    EncodePrivateKey as MlDsaEncodePrivateKey, EncodePublicKey as MlDsaEncodePublicKey,
};
use ml_dsa::Keypair as _;

pub(super) fn ml_dsa_key_algo_from_name(name: &str) -> Option<KeyAlgo> {
    match name {
        "ML-DSA-44" => Some(KeyAlgo::MlDsa44),
        "ML-DSA-65" => Some(KeyAlgo::MlDsa65),
        "ML-DSA-87" => Some(KeyAlgo::MlDsa87),
        _ => None,
    }
}

pub(super) fn is_ml_dsa_key_algo(algo: KeyAlgo) -> bool {
    matches!(algo, KeyAlgo::MlDsa44 | KeyAlgo::MlDsa65 | KeyAlgo::MlDsa87)
}

pub(super) fn ml_dsa_algorithm_name(algo: KeyAlgo) -> Option<&'static str> {
    match algo {
        KeyAlgo::MlDsa44 => Some("ML-DSA-44"),
        KeyAlgo::MlDsa65 => Some("ML-DSA-65"),
        KeyAlgo::MlDsa87 => Some("ML-DSA-87"),
        _ => None,
    }
}

/// Run `$body` with `$P` bound to the parameter set of `$algo`.
macro_rules! with_ml_dsa_params {
    ($algo:expr, $P:ident => $body:expr) => {
        match $algo {
            KeyAlgo::MlDsa44 => {
                type $P = ml_dsa::MlDsa44;
                $body
            }
            KeyAlgo::MlDsa65 => {
                type $P = ml_dsa::MlDsa65;
                $body
            }
            KeyAlgo::MlDsa87 => {
                type $P = ml_dsa::MlDsa87;
                $body
            }
            _ => None,
        }
    };
}

fn ml_dsa_der_pair_from_seed(algo: KeyAlgo, seed_bytes: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let seed = ml_dsa::Seed::try_from(seed_bytes).ok()?;
    with_ml_dsa_params!(algo, P => {
        let private = ml_dsa::SigningKey::<P>::from_seed(&seed);
        let private_der = private.to_pkcs8_der().ok()?.as_bytes().to_vec();
        let public_der = private
            .verifying_key()
            .to_public_key_der()
            .ok()?
            .as_bytes()
            .to_vec();
        Some((private_der, public_der))
    })
}

fn ml_dsa_public_bytes_from_der(algo: KeyAlgo, der: &[u8]) -> Option<Vec<u8>> {
    with_ml_dsa_params!(algo, P => {
        let public = ml_dsa::VerifyingKey::<P>::from_public_key_der(der).ok()?;
        Some(public.encode().as_slice().to_vec())
    })
}

fn ml_dsa_private_seed_and_public_from_der(
    algo: KeyAlgo,
    der: &[u8],
) -> Option<(Vec<u8>, Vec<u8>)> {
    with_ml_dsa_params!(algo, P => {
        let private = ml_dsa::SigningKey::<P>::from_pkcs8_der(der).ok()?;
        Some((
            private.to_seed().as_slice().to_vec(),
            private.verifying_key().encode().as_slice().to_vec(),
        ))
    })
}

fn ml_dsa_public_der_from_bytes(algo: KeyAlgo, public_bytes: &[u8]) -> Option<Vec<u8>> {
    with_ml_dsa_params!(algo, P => {
        let encoded = ml_dsa::EncodedVerifyingKey::<P>::try_from(public_bytes).ok()?;
        let public = ml_dsa::VerifyingKey::<P>::decode(&encoded);
        Some(public.to_public_key_der().ok()?.as_bytes().to_vec())
    })
}

/// ML-DSA.Sign (FIPS 204 Algorithm 2, the hedged variant node and OpenSSL
/// use) over `message` with the context string `context` (at most 255 bytes).
pub(super) fn ml_dsa_sign(
    algo: KeyAlgo,
    private_der: &[u8],
    message: &[u8],
    context: &[u8],
) -> Option<Vec<u8>> {
    with_ml_dsa_params!(algo, P => {
        let private = ml_dsa::SigningKey::<P>::from_pkcs8_der(private_der).ok()?;
        let mut rng = rand::rng();
        let signature = private
            .expanded_key()
            .sign_randomized(message, context, &mut rng)
            .ok()?;
        Some(signature.encode().as_slice().to_vec())
    })
}

/// ML-DSA.Verify (FIPS 204 Algorithm 3). A signature of the wrong length, or
/// one that does not decode, verifies as `false`; `None` means the key itself
/// is unusable.
pub(super) fn ml_dsa_verify(
    algo: KeyAlgo,
    public_der: &[u8],
    message: &[u8],
    context: &[u8],
    signature: &[u8],
) -> Option<bool> {
    with_ml_dsa_params!(algo, P => {
        let public = ml_dsa::VerifyingKey::<P>::from_public_key_der(public_der).ok()?;
        if context.len() > 255 {
            return Some(false);
        }
        let Ok(signature) = ml_dsa::Signature::<P>::try_from(signature) else {
            return Some(false);
        };
        Some(public.verify_with_context(message, context, &signature))
    })
}

// --- One view over ML-KEM and ML-DSA ("AKP" keys) ---------------------------

pub(super) fn akp_key_algo_from_name(name: &str) -> Option<KeyAlgo> {
    ml_kem_key_algo_from_name(name).or_else(|| ml_dsa_key_algo_from_name(name))
}

pub(super) fn is_akp_key_algo(algo: KeyAlgo) -> bool {
    is_ml_kem_key_algo(algo) || is_ml_dsa_key_algo(algo)
}

pub(super) fn akp_algorithm_name(algo: KeyAlgo) -> Option<&'static str> {
    ml_kem_algorithm_name(algo).or_else(|| ml_dsa_algorithm_name(algo))
}

/// The private seed length: `d || z` for ML-KEM, `xi` for ML-DSA.
pub(super) fn akp_seed_len(algo: KeyAlgo) -> usize {
    if is_ml_dsa_key_algo(algo) {
        32
    } else {
        64
    }
}

pub(super) fn akp_der_pair_from_seed(algo: KeyAlgo, seed: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    if seed.len() != akp_seed_len(algo) {
        return None;
    }
    if is_ml_dsa_key_algo(algo) {
        ml_dsa_der_pair_from_seed(algo, seed)
    } else {
        ml_kem_der_pair_from_seed(algo, seed)
    }
}

pub(super) fn akp_public_bytes_from_der(algo: KeyAlgo, der: &[u8]) -> Option<Vec<u8>> {
    if is_ml_dsa_key_algo(algo) {
        ml_dsa_public_bytes_from_der(algo, der)
    } else {
        ml_kem_public_bytes_from_der(algo, der)
    }
}

pub(super) fn akp_private_seed_and_public_from_der(
    algo: KeyAlgo,
    der: &[u8],
) -> Option<(Vec<u8>, Vec<u8>)> {
    if is_ml_dsa_key_algo(algo) {
        ml_dsa_private_seed_and_public_from_der(algo, der)
    } else {
        ml_kem_private_seed_and_public_from_der(algo, der)
    }
}

pub(super) fn akp_public_der_from_bytes(algo: KeyAlgo, public_bytes: &[u8]) -> Option<Vec<u8>> {
    if is_ml_dsa_key_algo(algo) {
        ml_dsa_public_der_from_bytes(algo, public_bytes)
    } else {
        ml_kem_public_der_from_bytes(algo, public_bytes)
    }
}

// --- `node:crypto` view -----------------------------------------------------
//
// `node:crypto` holds an ML-KEM / ML-DSA KeyObject as the PEM text of the same
// DER (`PRIVATE KEY` = seed-only PKCS#8, `PUBLIC KEY` = SPKI), so
// `key.export({ format: "pem" })` is the stored string, as for RSA and EC.

/// An ML-KEM or ML-DSA parameter set, as `node:crypto` names it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct PqAlgorithm(KeyAlgo);

const PQ_ALGORITHMS: [KeyAlgo; 6] = [
    KeyAlgo::MlKem512,
    KeyAlgo::MlKem768,
    KeyAlgo::MlKem1024,
    KeyAlgo::MlDsa44,
    KeyAlgo::MlDsa65,
    KeyAlgo::MlDsa87,
];

fn pem_encode(label: &str, der: &[u8]) -> String {
    let body = perry_base64::engine::general_purpose::STANDARD.encode(der);
    let mut pem = format!("-----BEGIN {label}-----\n");
    for line in body.as_bytes().chunks(64) {
        pem.push_str(std::str::from_utf8(line).unwrap_or(""));
        pem.push('\n');
    }
    pem.push_str(&format!("-----END {label}-----\n"));
    pem
}

fn pem_decode(label: &str, pem: &str) -> Option<Vec<u8>> {
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let start = pem.find(&begin)? + begin.len();
    let stop = pem[start..].find(&end)? + start;
    let body: String = pem[start..stop]
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    perry_base64::engine::general_purpose::STANDARD
        .decode(body.as_bytes())
        .ok()
}

impl PqAlgorithm {
    /// `"ml-kem-768"`, `"ml-dsa-65"`, … (`generateKeyPairSync`'s type and
    /// `KeyObject.asymmetricKeyType`).
    pub(crate) fn from_node_name(name: &str) -> Option<Self> {
        akp_key_algo_from_name(&name.to_ascii_uppercase()).map(Self)
    }

    /// The `asym_type` tag of a KeyObject string
    /// (`perry_runtime::buffer::mark_as_asymmetric_key`).
    pub(crate) fn asym_type(self) -> u8 {
        match self.0 {
            KeyAlgo::MlKem512 => 7,
            KeyAlgo::MlKem768 => 8,
            KeyAlgo::MlKem1024 => 9,
            KeyAlgo::MlDsa44 => 10,
            KeyAlgo::MlDsa65 => 11,
            _ => 12,
        }
    }

    pub(crate) fn is_signature(self) -> bool {
        is_ml_dsa_key_algo(self.0)
    }

    /// A fresh key pair as `(private PEM, public PEM)`.
    pub(crate) fn generate_pem_pair(self) -> Option<(String, String)> {
        let mut seed = [0u8; 64];
        rand_core_06::RngCore::fill_bytes(&mut rand_core_06::OsRng, &mut seed);
        let (private_der, public_der) =
            akp_der_pair_from_seed(self.0, &seed[..akp_seed_len(self.0)])?;
        Some((
            pem_encode("PRIVATE KEY", &private_der),
            pem_encode("PUBLIC KEY", &public_der),
        ))
    }
}

/// Whether `der` is a public (SPKI) or private (PKCS#8) key of any ML-KEM /
/// ML-DSA parameter set.
pub(super) fn akp_der_is_some_key(der: &[u8], public: bool) -> bool {
    PQ_ALGORITHMS.into_iter().any(|algo| {
        if public {
            akp_public_bytes_from_der(algo, der).is_some()
        } else {
            akp_private_seed_and_public_from_der(algo, der).is_some()
        }
    })
}

/// The KeyObject form of an ML-KEM / ML-DSA CryptoKey's DER: its PEM text and
/// `asym_type` tag.
pub(super) fn akp_key_object_pem(algo: KeyAlgo, kind: KeyKind, der: &[u8]) -> (String, u8) {
    let label = if kind == KeyKind::Public {
        "PUBLIC KEY"
    } else {
        "PRIVATE KEY"
    };
    (pem_encode(label, der), PqAlgorithm(algo).asym_type())
}

/// The CryptoKey algorithm named `name`, if it is the one a KeyObject tagged
/// `asym_type` holds.
pub(super) fn akp_key_algo_for_key_object(name: &str, asym_type: u8) -> Option<KeyAlgo> {
    akp_key_algo_from_name(name).filter(|algo| PqAlgorithm(*algo).asym_type() == asym_type)
}

/// The private key in `pem`, if it is an ML-KEM / ML-DSA `PRIVATE KEY`.
pub(crate) fn pq_private_key_from_pem(pem: &str) -> Option<(PqAlgorithm, Vec<u8>)> {
    let der = pem_decode("PRIVATE KEY", pem)?;
    PQ_ALGORITHMS
        .into_iter()
        .find(|algo| akp_private_seed_and_public_from_der(*algo, &der).is_some())
        .map(|algo| (PqAlgorithm(algo), der))
}

/// The public key in `pem`: an ML-KEM / ML-DSA `PUBLIC KEY`, or the public
/// half of such a `PRIVATE KEY`. Returns SPKI DER.
pub(crate) fn pq_public_key_from_pem(pem: &str) -> Option<(PqAlgorithm, Vec<u8>)> {
    if let Some((algorithm, private_der)) = pq_private_key_from_pem(pem) {
        let (_, public) = akp_private_seed_and_public_from_der(algorithm.0, &private_der)?;
        return Some((algorithm, akp_public_der_from_bytes(algorithm.0, &public)?));
    }
    let der = pem_decode("PUBLIC KEY", pem)?;
    PQ_ALGORITHMS
        .into_iter()
        .find(|algo| akp_public_bytes_from_der(*algo, &der).is_some())
        .map(|algo| (PqAlgorithm(algo), der))
}

pub(crate) fn pq_public_pem(public_der: &[u8]) -> String {
    pem_encode("PUBLIC KEY", public_der)
}

/// ML-KEM.Encaps: `(ciphertext, shared key)`.
pub(crate) fn pq_encapsulate(
    algorithm: PqAlgorithm,
    public_der: &[u8],
) -> Option<(Vec<u8>, Vec<u8>)> {
    super::encapsulation::ml_kem_encapsulate(algorithm.0, public_der)
}

/// ML-KEM.Decaps. `None` for a ciphertext of the wrong length.
pub(crate) fn pq_decapsulate(
    algorithm: PqAlgorithm,
    private_der: &[u8],
    ciphertext: &[u8],
) -> Option<Vec<u8>> {
    super::encapsulation::ml_kem_decapsulate(algorithm.0, private_der, ciphertext)
}

pub(crate) fn pq_sign(
    algorithm: PqAlgorithm,
    private_der: &[u8],
    message: &[u8],
    context: &[u8],
) -> Option<Vec<u8>> {
    ml_dsa_sign(algorithm.0, private_der, message, context)
}

pub(crate) fn pq_verify(
    algorithm: PqAlgorithm,
    public_der: &[u8],
    message: &[u8],
    context: &[u8],
    signature: &[u8],
) -> bool {
    ml_dsa_verify(algorithm.0, public_der, message, context, signature).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ml_dsa_signs_and_verifies_for_every_parameter_set() {
        // FIPS 204 Table 2: public key and signature sizes.
        for (algo, public_len, signature_len) in [
            (KeyAlgo::MlDsa44, 1312, 2420),
            (KeyAlgo::MlDsa65, 1952, 3309),
            (KeyAlgo::MlDsa87, 2592, 4627),
        ] {
            let (private_der, public_der) = akp_der_pair_from_seed(algo, &[7u8; 32]).unwrap();
            // Seed-only PKCS#8, as node exports it.
            assert_eq!(private_der.len(), 54);
            let public = akp_public_bytes_from_der(algo, &public_der).unwrap();
            assert_eq!(public.len(), public_len);
            assert_eq!(
                akp_public_der_from_bytes(algo, &public).unwrap(),
                public_der
            );
            let (seed, derived_public) =
                akp_private_seed_and_public_from_der(algo, &private_der).unwrap();
            assert_eq!(seed, [7u8; 32]);
            assert_eq!(derived_public, public);

            let signature = ml_dsa_sign(algo, &private_der, b"message", b"ctx").unwrap();
            assert_eq!(signature.len(), signature_len);
            assert_eq!(
                ml_dsa_verify(algo, &public_der, b"message", b"ctx", &signature),
                Some(true)
            );
            assert_eq!(
                ml_dsa_verify(algo, &public_der, b"other", b"ctx", &signature),
                Some(false)
            );
            assert_eq!(
                ml_dsa_verify(algo, &public_der, b"message", b"", &signature),
                Some(false)
            );
            assert_eq!(
                ml_dsa_verify(algo, &public_der, b"message", b"ctx", &signature[1..]),
                Some(false)
            );
            assert!(ml_dsa_sign(algo, &private_der, b"message", &[0u8; 256]).is_none());
        }
    }

    #[test]
    fn a_key_of_one_parameter_set_is_not_a_key_of_another() {
        let (private_der, public_der) =
            akp_der_pair_from_seed(KeyAlgo::MlDsa44, &[1u8; 32]).unwrap();
        assert!(akp_public_bytes_from_der(KeyAlgo::MlDsa65, &public_der).is_none());
        assert!(akp_private_seed_and_public_from_der(KeyAlgo::MlDsa87, &private_der).is_none());
        assert!(akp_public_bytes_from_der(KeyAlgo::MlKem768, &public_der).is_none());
        assert!(akp_der_pair_from_seed(KeyAlgo::MlDsa44, &[1u8; 64]).is_none());
    }

    #[test]
    fn node_pem_keys_round_trip_and_classify() {
        for name in [
            "ml-kem-512",
            "ml-kem-768",
            "ml-kem-1024",
            "ml-dsa-44",
            "ml-dsa-65",
            "ml-dsa-87",
        ] {
            let algorithm = PqAlgorithm::from_node_name(name).unwrap();
            let (private_pem, public_pem) = algorithm.generate_pem_pair().unwrap();
            let (private_algorithm, private_der) = pq_private_key_from_pem(&private_pem).unwrap();
            let (public_algorithm, public_der) = pq_public_key_from_pem(&public_pem).unwrap();
            assert_eq!(private_algorithm, algorithm);
            assert_eq!(public_algorithm, algorithm);
            assert!(pq_private_key_from_pem(&public_pem).is_none());
            // A private key yields its own public key.
            assert_eq!(
                pq_public_key_from_pem(&private_pem).unwrap(),
                (algorithm, public_der.clone())
            );
            assert_eq!(pq_public_pem(&public_der), public_pem);
            if algorithm.is_signature() {
                let signature = pq_sign(algorithm, &private_der, b"m", b"").unwrap();
                assert!(pq_verify(algorithm, &public_der, b"m", b"", &signature));
                assert!(!pq_verify(algorithm, &public_der, b"x", b"", &signature));
            } else {
                let (ciphertext, shared) = pq_encapsulate(algorithm, &public_der).unwrap();
                assert_eq!(
                    pq_decapsulate(algorithm, &private_der, &ciphertext).unwrap(),
                    shared
                );
                assert!(pq_decapsulate(algorithm, &private_der, &ciphertext[1..]).is_none());
            }
        }
        assert!(PqAlgorithm::from_node_name("ml-dsa-99").is_none());
    }
}
