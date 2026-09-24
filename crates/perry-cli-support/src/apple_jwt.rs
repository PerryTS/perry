//! Apple API token framing only. P-256 signing stays in RustCrypto and is
//! compiled only for CLI configurations that enable Apple API operations.
#[cfg(feature = "apple-jwt")]
pub fn generate(key_id: &str, issuer_id: &str, pem: &str) -> anyhow::Result<String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    sign(key_id, issuer_id, pem, now)
}
#[cfg(feature = "apple-jwt")]
fn sign(key_id: &str, issuer_id: &str, pem: &str, now: u64) -> anyhow::Result<String> {
    use anyhow::Context;
    use p256::{
        ecdsa::{signature::Signer, Signature, SigningKey},
        pkcs8::DecodePrivateKey,
    };
    use perry_base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
    let header = serde_json::json!({"alg": "ES256", "kid": key_id, "typ": "JWT"});
    let claims = serde_json::json!({"iss": issuer_id, "iat": now, "exp": now.checked_add(1200).context("JWT expiry overflow")?, "aud": "appstoreconnect-v1"});
    let key = SigningKey::from_pkcs8_pem(pem).map_err(|e| {
        anyhow::anyhow!("Failed to parse .p8 key — expected a P-256 private key: {e}")
    })?;
    let input = format!(
        "{}.{}",
        B64.encode(serde_json::to_vec(&header)?),
        B64.encode(serde_json::to_vec(&claims)?)
    );
    let signature: Signature = key.sign(input.as_bytes());
    Ok(format!("{input}.{}", B64.encode(signature.to_bytes())))
}
#[cfg(all(test, feature = "apple-jwt"))]
mod tests {
    use super::*;
    use p256::{
        ecdsa::{signature::Verifier, Signature, SigningKey},
        pkcs8::{EncodePrivateKey, LineEnding},
    };
    use perry_base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
    #[test]
    fn apple_claims_and_raw_es256_signature() {
        let key = SigningKey::from_slice(&[1; 32]).unwrap();
        let pem = key.to_pkcs8_pem(LineEnding::LF).unwrap();
        let token = sign("key\"id", "issuer", &pem, 1700000000).unwrap();
        let parts: Vec<_> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
        let header: serde_json::Value =
            serde_json::from_slice(&B64.decode(parts[0]).unwrap()).unwrap();
        let claims: serde_json::Value =
            serde_json::from_slice(&B64.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(header["kid"], "key\"id");
        assert_eq!(header["alg"], "ES256");
        assert_eq!(claims["exp"], 1700001200u64);
        assert_eq!(claims["aud"], "appstoreconnect-v1");
        let raw = B64.decode(parts[2]).unwrap();
        assert_eq!(raw.len(), 64);
        key.verifying_key()
            .verify(
                format!("{}.{}", parts[0], parts[1]).as_bytes(),
                &Signature::from_slice(&raw).unwrap(),
            )
            .unwrap();
        assert!(sign("key", "issuer", "invalid", 0).is_err());
    }
}
