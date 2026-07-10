//! Authenticated manifest format for the Perry CLI self-updater.

use anyhow::{bail, Context, Result};
use base64::Engine as _;
use ed25519_dalek::{Signature, Signer, Verifier, VerifyingKey};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

const DOMAIN: &[u8] = b"perry-cli-update-manifest-v1\0";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CliUpdateManifest {
    pub schema_version: u32,
    pub key_id: String,
    pub version: String,
    pub platform: String,
    pub artifact: CliUpdateArtifact,
    /// Base64 Ed25519 signature over `signing_payload`.
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CliUpdateArtifact {
    pub name: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

pub fn cli_manifest_signing_payload(manifest: &CliUpdateManifest) -> Result<Vec<u8>> {
    if manifest.schema_version != 1 {
        bail!(
            "unsupported CLI update manifest schema {}",
            manifest.schema_version
        );
    }
    if manifest.key_id.is_empty()
        || manifest.version.is_empty()
        || manifest.platform.is_empty()
        || manifest.artifact.name.is_empty()
        || manifest.artifact.url.is_empty()
    {
        bail!("CLI update manifest contains an empty security-critical field");
    }
    Version::parse(manifest.version.trim_start_matches('v'))
        .context("manifest version is not valid semver")?;
    let digest = decode_sha256(&manifest.artifact.sha256)?;
    let mut out = Vec::with_capacity(192 + manifest.artifact.url.len());
    out.extend_from_slice(DOMAIN);
    out.extend_from_slice(&manifest.schema_version.to_be_bytes());
    for field in [
        &manifest.key_id,
        &manifest.version,
        &manifest.platform,
        &manifest.artifact.name,
        &manifest.artifact.url,
    ] {
        let bytes = field.as_bytes();
        let len = u32::try_from(bytes.len()).context("manifest field is too long")?;
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(bytes);
    }
    out.extend_from_slice(&digest);
    out.extend_from_slice(&manifest.artifact.size.to_be_bytes());
    Ok(out)
}

pub fn sign_cli_manifest(
    manifest: &mut CliUpdateManifest,
    signing_key: &ed25519_dalek::SigningKey,
) -> Result<()> {
    manifest.signature = base64::engine::general_purpose::STANDARD.encode(
        signing_key
            .sign(&cli_manifest_signing_payload(manifest)?)
            .to_bytes(),
    );
    Ok(())
}

pub fn verify_cli_manifest(
    manifest: &CliUpdateManifest,
    expected_platform: &str,
    current_version: &str,
    trusted_keys: &[(&str, &str)],
) -> Result<()> {
    if manifest.platform != expected_platform {
        bail!(
            "manifest platform {} does not match {}",
            manifest.platform,
            expected_platform
        );
    }
    if Version::parse(manifest.version.trim_start_matches('v'))?
        <= Version::parse(current_version.trim_start_matches('v'))?
    {
        bail!(
            "manifest version {} is not newer than installed {} (replay/downgrade rejected)",
            manifest.version,
            current_version
        );
    }
    let key_b64 = trusted_keys
        .iter()
        .find(|(id, _)| *id == manifest.key_id)
        .map(|(_, key)| *key)
        .with_context(|| format!("manifest key id {:?} is not trusted", manifest.key_id))?;
    let public_key = decode_public_key(key_b64)?;
    let signature = decode_signature(&manifest.signature)?;
    public_key
        .verify(&cli_manifest_signing_payload(manifest)?, &signature)
        .context("CLI update manifest signature verification failed")
}

pub fn verify_cli_artifact(path: &Path, artifact: &CliUpdateArtifact) -> Result<()> {
    let metadata =
        std::fs::metadata(path).with_context(|| format!("cannot stat {}", path.display()))?;
    if !metadata.is_file() {
        bail!("downloaded update artifact is not a regular file");
    }
    if metadata.len() != artifact.size {
        bail!(
            "downloaded update artifact size mismatch (expected {}, got {})",
            artifact.size,
            metadata.len()
        );
    }
    let expected = decode_sha256(&artifact.sha256)?;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0_u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let actual: [u8; 32] = hasher.finalize().into();
    if actual != expected {
        bail!("downloaded update artifact SHA-256 mismatch");
    }
    Ok(())
}

pub fn decode_sha256(value: &str) -> Result<[u8; 32]> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("sha256 must be exactly 64 hexadecimal characters");
    }
    let bytes = hex::decode(value)?;
    Ok(bytes.try_into().expect("hex length checked"))
}

fn decode_public_key(value: &str) -> Result<VerifyingKey> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(value.trim())
        .context("trusted public key is not base64")?;
    let raw: [u8; 32] = bytes.try_into().map_err(|v: Vec<u8>| {
        anyhow::anyhow!("trusted public key must be 32 bytes, got {}", v.len())
    })?;
    VerifyingKey::from_bytes(&raw).context("trusted public key is invalid")
}
fn decode_signature(value: &str) -> Result<Signature> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(value.trim())
        .context("manifest signature is not base64")?;
    let raw: [u8; 64] = bytes.try_into().map_err(|v: Vec<u8>| {
        anyhow::anyhow!("manifest signature must be 64 bytes, got {}", v.len())
    })?;
    Ok(Signature::from_bytes(&raw))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn signed() -> (CliUpdateManifest, String) {
        let signing = SigningKey::from_bytes(&[7; 32]);
        let public =
            base64::engine::general_purpose::STANDARD.encode(signing.verifying_key().to_bytes());
        let mut m = CliUpdateManifest {
            schema_version: 1,
            key_id: "release-2026".into(),
            version: "9.9.9".into(),
            platform: "perry-linux-x86_64.tar.gz".into(),
            artifact: CliUpdateArtifact {
                name: "perry-linux-x86_64.tar.gz".into(),
                url: "https://example.invalid/perry-linux-x86_64.tar.gz".into(),
                sha256: hex::encode(Sha256::digest(b"artifact")),
                size: 8,
            },
            signature: String::new(),
        };
        m.signature = base64::engine::general_purpose::STANDARD.encode(
            signing
                .sign(&cli_manifest_signing_payload(&m).unwrap())
                .to_bytes(),
        );
        (m, public)
    }
    #[test]
    fn rejects_wrong_signature_digest_version_platform_and_replay() {
        let (m, public) = signed();
        let keys = [("release-2026", public.as_str())];
        verify_cli_manifest(&m, &m.platform, "1.0.0", &keys).unwrap();
        for mutate in [0, 1, 2, 3] {
            let mut bad = m.clone();
            match mutate {
                0 => bad.signature = "AAAA".into(),
                1 => bad.artifact.sha256 = "00".repeat(32),
                2 => bad.version = "9.9.8".into(),
                _ => bad.platform = "perry-windows-x86_64.zip".into(),
            }
            assert!(
                verify_cli_manifest(&bad, "perry-linux-x86_64.tar.gz", "1.0.0", &keys).is_err()
            );
        }
        assert!(
            verify_cli_manifest(&m, &m.platform, "9.9.9", &keys).is_err(),
            "same version is replay"
        );
        assert!(
            verify_cli_manifest(&m, &m.platform, "10.0.0", &keys).is_err(),
            "downgrade is replay"
        );
    }
    #[test]
    fn rejects_invalid_manifest_fields_and_keys() {
        let (m, public) = signed();
        let trusted = [("release-2026", public.as_str())];

        let unknown = [("other-key", public.as_str())];
        assert!(verify_cli_manifest(&m, &m.platform, "1.0.0", &unknown).is_err());

        let mut invalid_signature = m.clone();
        invalid_signature.signature = "not-base64".into();
        assert!(verify_cli_manifest(&invalid_signature, &m.platform, "1.0.0", &trusted).is_err());

        let invalid_public_key = [("release-2026", "not-base64")];
        assert!(verify_cli_manifest(&m, &m.platform, "1.0.0", &invalid_public_key).is_err());

        for mutate in [0, 1, 2] {
            let mut invalid = m.clone();
            match mutate {
                0 => invalid.version = "not-semver".into(),
                1 => invalid.key_id.clear(),
                _ => invalid.schema_version = 2,
            }
            assert!(cli_manifest_signing_payload(&invalid).is_err());
            assert!(verify_cli_manifest(&invalid, &m.platform, "1.0.0", &trusted).is_err());
        }
    }

    #[test]
    fn artifact_hash_and_size_are_checked() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a");
        std::fs::write(&file, b"artifact").unwrap();
        let (m, _) = signed();
        verify_cli_artifact(&file, &m.artifact).unwrap();
        let mut bad = m.artifact.clone();
        bad.size += 1;
        assert!(verify_cli_artifact(&file, &bad).is_err());
        bad.size = 8;
        bad.sha256 = "00".repeat(32);
        assert!(verify_cli_artifact(&file, &bad).is_err());
    }
}
