use super::*;

// --- Saved config (~/.perry/config.toml) ---

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BetaConfig {
    /// User has seen and acknowledged the public beta notice
    pub(crate) acknowledged: bool,
    /// User opted in to automatic error reporting for beta commands
    #[serde(default)]
    pub(crate) report_errors: bool,
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub(crate) struct PerryConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) license_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) server: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) default_target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) api_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) github_username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) apple: Option<AppleSavedConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) ios: Option<IosSavedConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) android: Option<AndroidSavedConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) harmonyos: Option<HarmonyosSavedConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) telemetry: Option<crate::telemetry::TelemetryConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) beta: Option<BetaConfig>,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AppleSavedConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) team_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) p8_key_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) key_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) issuer_id: Option<String>,
}

/// Legacy struct kept for backward compatibility when reading old config files.
/// New configs no longer save iOS-specific fields to the global config.
#[derive(Default, Debug, Serialize, Deserialize)]
pub(crate) struct IosSavedConfig {}

#[derive(Default, Debug, Serialize, Deserialize)]
pub(crate) struct AndroidSavedConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) keystore_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) key_alias: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) google_play_key_path: Option<String>,
}

/// HarmonyOS signing materials. Populated by `perry setup harmonyos`.
///
/// The p12 password is stored plaintext in `~/.perry/config.toml` (the file is
/// already protected by the user's home dir perms; macOS-Keychain integration
/// is a future improvement). DevEco itself stores the same password
/// AES-encrypted in `build-profile.json5` with a machine-bound key that isn't
/// extractable to external tools — so the wizard prompts the user once and
/// caches it here for subsequent compiles.
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub(crate) struct HarmonyosSavedConfig {
    /// Path to the .p12 keystore (typically `~/.ohos/config/default_*.p12`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) p12_path: Option<String>,
    /// Plaintext password for the .p12 keystore. Same value is used as the
    /// key password — DevEco's auto-generated debug cert uses one password
    /// for both store and key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) p12_password: Option<String>,
    /// Path to the provisioning profile (.p7b).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) profile_path: Option<String>,
    /// Path to the cert chain (.cer / .pem). hap-sign-tool requires this as
    /// `-appCertFile`, distinct from `-profileFile`. DevEco's auto-signing
    /// names it `<bundleName>.cer`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) cert_path: Option<String>,
    /// bundleName the profile is bound to (e.g. `com.example.myapplication`).
    /// Auto-extracted from the .p7b's embedded JSON.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) bundle_name: Option<String>,
    /// Key alias inside the .p12 (DevEco's auto-generated cert uses
    /// `debugKey`; users with their own keystore may have a different alias).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) key_alias: Option<String>,
}

pub(crate) fn config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".perry")
        .join("config.toml")
}

pub(crate) fn load_config() -> PerryConfig {
    let path = config_path();
    if let Ok(content) = fs::read_to_string(&path) {
        toml::from_str(&content).unwrap_or_default()
    } else {
        PerryConfig::default()
    }
}

pub(crate) fn save_config(config: &PerryConfig) -> Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = toml::to_string_pretty(config).context("Failed to serialize config")?;
    fs::write(&path, content)?;
    Ok(())
}

pub(crate) fn is_interactive() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

/// Show a one-time public beta notice for publish/verify commands.
/// Returns true if the user acknowledges (or has previously acknowledged).
/// Non-interactive sessions skip the prompt and proceed.
pub(crate) fn check_beta_consent(command: &str) -> bool {
    let mut config = load_config();

    // Already acknowledged — nothing to do
    if let Some(ref beta) = config.beta {
        if beta.acknowledged {
            return true;
        }
    }

    // Non-interactive: proceed without prompting (errors won't be reported)
    if !is_interactive() {
        return true;
    }

    eprintln!();
    eprintln!(
        "  {} perry {} is in {}.",
        style("NOTE").yellow().bold(),
        command,
        style("public beta").yellow().bold(),
    );
    eprintln!("  It should work, but if you encounter issues please let us know.");
    eprintln!(
        "  Report issues: {}",
        style("https://github.com/PerryTS/perry/issues")
            .cyan()
            .underlined()
    );
    eprintln!();

    let report = Confirm::new()
        .with_prompt("  Automatically report errors to help us fix issues faster?")
        .default(true)
        .interact()
        .unwrap_or(false);

    let proceed = Confirm::new()
        .with_prompt("  Continue?")
        .default(true)
        .interact()
        .unwrap_or(false);

    if !proceed {
        return false;
    }

    config.beta = Some(BetaConfig {
        acknowledged: true,
        report_errors: report,
    });
    let _ = save_config(&config);

    true
}

/// Send a sanitized error report for a beta command failure.
/// Fire-and-forget on a background thread. No credentials or file paths are included.
pub(crate) fn report_beta_error(command: &str, error: &str, target: Option<&str>) {
    let config = load_config();
    let should_report = config
        .beta
        .as_ref()
        .is_some_and(|b| b.acknowledged && b.report_errors);

    if !should_report {
        return;
    }

    // Sanitize: strip anything that looks like a file path or credential
    let sanitized = sanitize_error_for_report(error);

    crate::telemetry::send_event(
        &format!("beta_error_{}", command),
        &[
            ("error", &sanitized),
            ("target", target.unwrap_or("unknown")),
            ("version", env!("CARGO_PKG_VERSION")),
            ("platform", std::env::consts::OS),
        ],
    );
}

/// Strip file paths, tokens, and other potentially sensitive data from error messages.
pub(super) fn sanitize_error_for_report(error: &str) -> String {
    let mut result = String::new();
    for word in error.split_whitespace() {
        if !result.is_empty() {
            result.push(' ');
        }
        // Redact absolute file paths
        if word.starts_with('/')
            || (word.len() >= 3 && word.as_bytes()[1] == b':' && word.as_bytes()[2] == b'\\')
        {
            result.push_str("<path>");
        // Redact long alphanumeric strings (tokens, keys, base64 blobs)
        } else if word.len() >= 32
            && word
                .chars()
                .all(|c| c.is_alphanumeric() || c == '+' || c == '/' || c == '=')
        {
            result.push_str("<redacted>");
        } else {
            result.push_str(word);
        }
    }

    // Truncate to 500 chars max
    if result.len() > 500 {
        result.truncate(500);
        result.push_str("...");
    }

    result
}

/// Prompt user for text input with an optional default value.
/// Returns None if the user enters empty string.
pub(crate) fn prompt_input(prompt: &str, default: Option<&str>) -> Option<String> {
    let mut builder = Input::<String>::new().with_prompt(prompt);
    if let Some(d) = default {
        builder = builder.default(d.to_string());
    }
    builder = builder.allow_empty(true);
    match builder.interact_text() {
        Ok(val) if val.is_empty() => None,
        Ok(val) => Some(val),
        Err(_) => None,
    }
}
