use anyhow::Result;
use dialoguer::Input;

use super::*;

pub fn generate_asc_jwt(key_id: &str, issuer_id: &str, p8_content: &str) -> Result<String> {
    crate::apple_jwt::generate(key_id, issuer_id, p8_content)
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
