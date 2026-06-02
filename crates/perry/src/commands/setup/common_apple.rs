use anyhow::{anyhow, bail, Context, Result};
use clap::Args;
use console::style;
use dialoguer::{Confirm, Input, Password, Select};
use std::path::PathBuf;
use std::process::Command;

use super::super::publish::{
    config_path, is_interactive, load_config, save_config, AndroidSavedConfig, AppleSavedConfig,
    HarmonyosSavedConfig, PerryConfig,
};

use super::*;

pub fn generate_asc_jwt(key_id: &str, issuer_id: &str, p8_content: &str) -> Result<String> {
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let header = Header {
        alg: Algorithm::ES256,
        kid: Some(key_id.to_string()),
        typ: Some("JWT".to_string()),
        ..Default::default()
    };

    let claims = serde_json::json!({
        "iss": issuer_id,
        "iat": now,
        "exp": now + 1200,
        "aud": "appstoreconnect-v1"
    });

    let encoding_key = EncodingKey::from_ec_pem(p8_content.as_bytes())
        .context("Failed to parse .p8 key — ensure it's a valid EC private key")?;

    let token = encode(&header, &claims, &encoding_key).context("Failed to generate JWT")?;

    Ok(token)
}

/// Prompt for App Store Connect API credentials
pub fn prompt_api_credentials() -> Result<(String, String, String, String)> {
    let p8_path = prompt_file_path("  Path to .p8 key file", ".p8")?;
    let key_id = Input::<String>::new()
        .with_prompt("  Key ID (e.g. ABC123XYZ)")
        .interact_text()?;
    let issuer_id = Input::<String>::new()
        .with_prompt("  Issuer ID (UUID format)")
        .interact_text()?;
    let team_id = Input::<String>::new()
        .with_prompt("  Apple Developer Team ID (10 chars)")
        .interact_text()?;
    Ok((p8_path, key_id, issuer_id, team_id))
}

// ---------------------------------------------------------------------------
// macOS wizard
// ---------------------------------------------------------------------------
