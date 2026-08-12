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
    /// The `[update]` section.
    ///
    /// ★ This field is why the section survives a save. `update_checker` read
    /// `[update] server` through its own private structs, but `PerryConfig` —
    /// which is what `save_config` writes — had no field for it. serde
    /// reconstructs the file from this struct, so every save silently deleted
    /// the user's `[update]` section, and `save_config` is called from the
    /// telemetry prompt, the compatibility-report prompt, the beta notice and
    /// the setup wizards.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) update: Option<crate::update_policy::UpdateConfig>,
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
    load_config_checked().unwrap_or_default()
}

/// `load_config`, but able to say WHY it produced nothing.
///
/// `Err` means the file exists and does not parse — the case a plain
/// `unwrap_or_default()` cannot tell apart from "no file yet". The difference
/// matters at save time, because writing a default struct over a damaged but
/// hand-recoverable config destroys the user's license key and tokens along with
/// the syntax error they were about to fix.
pub(crate) fn load_config_checked() -> std::result::Result<PerryConfig, String> {
    let path = config_path();
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        // A missing file is the ONLY read failure that means "no config yet".
        // A permission change or a transient I/O error must not be answered with
        // defaults, because `update_config_file` would accept those defaults and
        // write them over a file that still holds the license key and tokens —
        // the very loss this function exists to prevent.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PerryConfig::default());
        }
        Err(error) => return Err(format!("{} could not be read: {error}", path.display())),
    };
    toml::from_str(&content).map_err(|error| error.to_string())
}

/// Read, modify, write — refusing to write when the read failed.
///
/// Every setting-writer goes through this. Handing `load_config`'s default
/// struct to `save_config` is how a damaged file becomes an erased one, and a
/// caller cannot tell the two apart on its own.
pub(crate) fn update_config_file(edit: impl FnOnce(&mut PerryConfig)) -> Result<()> {
    let mut config = load_config_checked().map_err(|error| {
        anyhow::anyhow!(
            "~/.perry/config.toml could not be loaded, so it was left untouched: \
             {error}. Fix the file (or delete it) and try again."
        )
    })?;
    edit(&mut config);
    save_config(&config)
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

#[cfg(test)]
mod saved_config_tests {
    use super::*;

    /// ★ The erasure regression.
    ///
    /// `update_checker` read `[update] server` through its own private structs,
    /// but `PerryConfig` — which is what `save_config` writes — had no field
    /// for it. serde rebuilds the file from this struct, so any save deleted
    /// the section: the telemetry prompt, the compatibility-report prompt, the
    /// beta notice and the setup wizards all call `save_config`, so a user
    /// answering one prompt silently lost their update settings.
    ///
    /// String-level rather than filesystem-level on purpose: the bug is in the
    /// serde round trip, and a test that wrote to `~/.perry` would depend on
    /// the developer's home directory.
    #[test]
    fn the_update_section_survives_a_round_trip() {
        let original = r#"
license_key = "keep-me"

[telemetry]
enabled = true
client_id = "abc"

[update]
mode = "prompt"
server = "https://updates.example.test/latest"
check_interval_hours = 6
"#;
        let config: PerryConfig =
            toml::from_str(original).expect("the fixture must parse as a whole config");
        let written = toml::to_string_pretty(&config).expect("serialize");

        assert!(
            written.contains("[update]"),
            "the [update] section was dropped on save:\n{written}"
        );
        assert!(
            written.contains("updates.example.test"),
            "the update server was dropped on save:\n{written}"
        );
        assert!(
            written.contains("check_interval_hours"),
            "an [update] key was dropped on save:\n{written}"
        );
        // ...and nothing else was lost on the way past.
        assert!(written.contains("keep-me") && written.contains("[telemetry]"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A config that does not parse must be left alone, not replaced by defaults.
    ///
    /// This is the data-loss case. `load_config` cannot tell "no file yet" from
    /// "damaged file", so a writer built on it turns one stray character into an
    /// erased license key — the user's own tokens, gone while they were fixing a
    /// typo.
    #[test]
    fn a_damaged_config_is_never_overwritten_with_defaults() {
        let _lock = crate::test_env_lock::env_lock();
        let home = tempfile::tempdir().expect("tempdir");
        let saved = std::env::var_os("HOME");
        std::env::set_var("HOME", home.path());

        let path = config_path();
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        let damaged = "license_key = \"keep-me\"\nthis is not toml\n";
        std::fs::write(&path, damaged).expect("write");

        let error = update_config_file(|config| {
            config.update.get_or_insert_with(Default::default).mode =
                Some(crate::update_policy::UpdateMode::Notify);
        })
        .expect_err("a damaged config must refuse the write");
        assert!(
            format!("{error:#}").contains("left untouched"),
            "the message must say the file was not written: {error:#}"
        );
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            damaged,
            "the file was rewritten despite the refusal"
        );

        // A file that exists but cannot be read is the same danger wearing a
        // different hat: treating a permission error as "no config yet" would
        // serialize defaults over a file whose contents we never saw.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let kept = "license_key = \"keep-me\"\n";
            std::fs::write(&path, kept).expect("write");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).expect("chmod");
            // Root ignores the mode bits, so there is nothing to prove when the
            // suite runs as root. Skip in that case rather than fail.
            let still_readable = std::fs::read_to_string(&path).is_ok();
            let outcome = update_config_file(|config| {
                config.update.get_or_insert_with(Default::default).mode =
                    Some(crate::update_policy::UpdateMode::Notify);
            });
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .expect("restore");
            if !still_readable {
                let error = outcome.expect_err("an unreadable config must refuse the write");
                let text = format!("{error:#}");
                // The REASON matters, not just the failure. A write that fails on
                // the same permissions would also produce an error, and a test
                // that accepts any error passes whether or not the read is
                // checked at all.
                assert!(
                    text.contains("could not be read"),
                    "the refusal must name the read failure, not a later write \
                     failure: {text}"
                );
                assert!(text.contains("left untouched"), "{text}");
                assert_eq!(
                    std::fs::read_to_string(&path).expect("read"),
                    kept,
                    "the file was rewritten despite being unreadable"
                );
            }
        }

        // A missing file is still the "use defaults" case, so a first-time write
        // has to succeed.
        std::fs::remove_file(&path).expect("remove");
        update_config_file(|config| {
            config.update.get_or_insert_with(Default::default).mode =
                Some(crate::update_policy::UpdateMode::Notify);
        })
        .expect("a fresh config must be writable");
        assert!(
            std::fs::read_to_string(&path)
                .expect("read")
                .contains("mode"),
            "the setting was not persisted"
        );

        match saved {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }
}
