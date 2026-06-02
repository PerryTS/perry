//! Image verification and security modules.

use crate::container::mod_private::get_global_backend_instance;
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

pub const CHAINGUARD_IDENTITY: &str =
    "https://github.com/chainguard-images/images/.github/workflows/sign.yaml@refs/heads/main";
pub const CHAINGUARD_ISSUER: &str = "https://token.actions.githubusercontent.com";

#[derive(Debug, Clone)]
pub enum VerificationResult {
    Verified,
    Failed(String),
}

static VERIFICATION_CACHE: OnceLock<RwLock<HashMap<String, VerificationResult>>> = OnceLock::new();

pub async fn fetch_image_digest(reference: &str) -> Result<String, String> {
    let backend = get_global_backend_instance().await?;
    let info = backend
        .inspect_image(reference)
        .await
        .map_err(|e| e.to_string())?;
    Ok(info.id)
}

pub async fn run_cosign_verify(reference: &str, digest: &str) -> VerificationResult {
    let output = tokio::process::Command::new("cosign")
        .args([
            "verify",
            "--certificate-identity",
            CHAINGUARD_IDENTITY,
            "--certificate-oidc-issuer",
            CHAINGUARD_ISSUER,
            &format!("{}@{}", reference, digest),
        ])
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => VerificationResult::Verified,
        Ok(out) => VerificationResult::Failed(String::from_utf8_lossy(&out.stderr).to_string()),
        Err(e) => VerificationResult::Failed(e.to_string()),
    }
}

pub async fn verify_image(reference: &str) -> Result<String, String> {
    // 1. Fetch digest (tag -> digest resolution)
    let digest = fetch_image_digest(reference).await?;

    // 2. Check cache
    let cache = VERIFICATION_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    {
        let cache_read = cache.read().unwrap();
        if let Some(result) = cache_read.get(&digest) {
            return match result {
                VerificationResult::Verified => Ok(digest),
                VerificationResult::Failed(reason) => {
                    Err(format!("Verification failed: {}", reason))
                }
            };
        }
    }

    // 3. Run cosign verify
    let result = run_cosign_verify(reference, &digest).await;

    // 4. Cache result
    {
        let mut cache_write = cache.write().unwrap();
        cache_write.insert(digest.clone(), result.clone());
    }

    match result {
        VerificationResult::Verified => Ok(digest),
        VerificationResult::Failed(reason) => Err(format!("Verification failed: {}", reason)),
    }
}

pub fn get_chainguard_image(tool: &str) -> Option<String> {
    match tool {
        "git" => Some("cgr.dev/chainguard/git".to_string()),
        "curl" => Some("cgr.dev/chainguard/curl".to_string()),
        "wget" => Some("cgr.dev/chainguard/wget".to_string()),
        "openssl" => Some("cgr.dev/chainguard/openssl".to_string()),
        "bash" => Some("cgr.dev/chainguard/bash".to_string()),
        "sh" => Some("cgr.dev/chainguard/busybox".to_string()),
        "node" => Some("cgr.dev/chainguard/node".to_string()),
        "python" => Some("cgr.dev/chainguard/python".to_string()),
        "ruby" => Some("cgr.dev/chainguard/ruby".to_string()),
        "go" => Some("cgr.dev/chainguard/go".to_string()),
        "rust" => Some("cgr.dev/chainguard/rust".to_string()),
        _ => None,
    }
}

pub fn get_default_base_image() -> &'static str {
    "cgr.dev/chainguard/alpine-base"
}

pub fn get_static_base_image() -> &'static str {
    "cgr.dev/chainguard/wolfi-base"
}

pub fn clear_verification_cache() {
    if let Some(cache) = VERIFICATION_CACHE.get() {
        let mut write = cache.write().unwrap();
        write.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chainguard_image_lookup() {
        assert_eq!(
            get_chainguard_image("git"),
            Some("cgr.dev/chainguard/git".to_string())
        );
        assert_eq!(
            get_chainguard_image("rust"),
            Some("cgr.dev/chainguard/rust".to_string())
        );
        assert_eq!(get_chainguard_image("unknown-tool"), None);
    }

    #[test]
    fn test_base_image_defaults() {
        assert!(get_default_base_image().contains("chainguard"));
        assert!(get_static_base_image().contains("wolfi"));
    }
}
