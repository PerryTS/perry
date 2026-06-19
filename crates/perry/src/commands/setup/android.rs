use anyhow::{bail, Result};
use console::style;
use dialoguer::{Confirm, Input, Password};

use super::super::publish::{config_path, AndroidSavedConfig, PerryConfig};

use super::*;

pub fn android_wizard(saved: &mut PerryConfig) -> Result<()> {
    println!("  {}", style("Android Setup").bold());
    println!();

    // --- Step 1: Keystore ---
    println!("  {} Keystore", style("Step 1/2 —").cyan().bold());
    println!();

    let has_keystore = Confirm::new()
        .with_prompt("  Do you have an existing Android keystore?")
        .default(true)
        .interact()?;

    let (keystore_path, key_alias) = if has_keystore {
        let path = Input::<String>::new()
            .with_prompt("  Keystore path")
            .interact_text()?;
        let path = expand_tilde(&path);
        let alias = Input::<String>::new()
            .with_prompt("  Key alias")
            .default("key0".to_string())
            .interact_text()?;
        if !std::path::Path::new(&path).exists() {
            bail!("Keystore file not found: {path}");
        }
        (path, alias)
    } else {
        // Check for keytool
        if std::process::Command::new("keytool")
            .arg("-help")
            .stderr(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .status()
            .is_err()
        {
            bail!(
                "keytool not found — install a JDK first (e.g. brew install --cask temurin) \
                 and try again."
            );
        }

        println!("  Generating a new Android release keystore...");
        println!();

        let path = prompt_output_path("  Output path (e.g. ~/release-key.keystore)")?;
        let alias = Input::<String>::new()
            .with_prompt("  Key alias")
            .default("key0".to_string())
            .interact_text()?;
        let password = Password::new()
            .with_prompt("  Keystore password")
            .with_confirmation("  Confirm password", "Passwords didn't match")
            .interact()?;

        let status = std::process::Command::new("keytool")
            .args([
                "-genkeypair",
                "-v",
                "-keystore",
                &path,
                "-keyalg",
                "RSA",
                "-keysize",
                "2048",
                "-validity",
                "10000",
                "-alias",
                &alias,
                "-storepass",
                &password,
                "-keypass",
                &password,
                "-dname",
                "CN=Android, O=Android, C=US",
            ])
            .status()?;

        if !status.success() {
            bail!("keytool failed to generate keystore");
        }

        println!();
        println!(
            "  {} Keystore created at {}",
            style("✓").green(),
            style(&path).bold()
        );
        (path, alias)
    };

    let android = saved
        .android
        .get_or_insert_with(AndroidSavedConfig::default);
    android.keystore_path = Some(keystore_path.clone());
    android.key_alias = Some(key_alias.clone());

    println!(
        "  {} Keystore: {}",
        style("✓").green(),
        style(&keystore_path).bold()
    );
    println!(
        "  {} Key alias: {}",
        style("✓").green(),
        style(&key_alias).bold()
    );
    println!();

    // --- Step 2: Google Play Service Account ---
    println!(
        "  {} Google Play Service Account",
        style("Step 2/2 —").cyan().bold()
    );
    println!();
    println!("  Follow these steps to enable automated Play Store uploads:");
    println!();
    println!("  1. Enable the Google Play Android Developer API:");
    println!("     https://console.cloud.google.com/apis/library/androidpublisher.googleapis.com");
    println!("     → Hit Enable.");
    println!();
    println!("  2. Create a service account + download its JSON key:");
    println!("     https://console.cloud.google.com/iam-admin/serviceaccounts");
    println!("     → Create Service Account → Keys tab → Add Key → JSON → download.");
    println!();
    println!("  3. Grant permissions in Play Console:");
    println!("     → Users & Permissions → Invite new users");
    println!("     → Paste the service account email → grant Release Manager permissions.");
    println!();
    println!(
        "  {} The first release MUST be uploaded manually via Play Console before",
        style("!").yellow()
    );
    println!("     automated uploads will work.");
    println!();

    press_enter_to_continue("  Press Enter when ready");

    let json_path = Input::<String>::new()
        .with_prompt("  Path to service account JSON key")
        .interact_text()?;
    let json_path = expand_tilde(&json_path);

    if !std::path::Path::new(&json_path).exists() {
        bail!("Service account JSON not found: {json_path}");
    }

    // Validate JSON content
    let json_content = std::fs::read_to_string(&json_path)?;
    let parsed: serde_json::Value =
        serde_json::from_str(&json_content).map_err(|e| anyhow::anyhow!("Invalid JSON: {e}"))?;
    let client_email = parsed["client_email"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing 'client_email' in service account JSON"))?;
    if parsed["private_key"].as_str().is_none() {
        bail!("Missing 'private_key' in service account JSON");
    }

    println!(
        "  {} Service account: {}",
        style("✓").green(),
        style(client_email).bold()
    );

    let android = saved
        .android
        .get_or_insert_with(AndroidSavedConfig::default);
    android.google_play_key_path = Some(json_path);

    // Update project perry.toml with distribute = "playstore"
    let perry_toml_path = std::env::current_dir()?.join("perry.toml");
    // Create perry.toml if it doesn't exist — project-specific config belongs here
    if !perry_toml_path.exists() {
        std::fs::write(&perry_toml_path, "")?;
    }
    let gp_key = saved
        .android
        .as_ref()
        .and_then(|a| a.google_play_key_path.as_deref());
    match update_perry_toml_android(&perry_toml_path, &keystore_path, &key_alias, gp_key) {
        Ok(()) => {}
        Err(e) => {
            println!();
            println!("  {} Could not update perry.toml: {e}", style("!").yellow());
            println!("  Add these manually to your perry.toml [android] section:");
            println!("    keystore = \"{}\"", keystore_path);
            println!("    key_alias = \"{}\"", key_alias);
            println!("    distribute = \"playstore\"");
        }
    }

    // --- Summary ---
    println!();
    println!("  {}", style("Setup complete!").green().bold());
    println!();
    println!(
        "  {} {} {}",
        style("Global").bold(),
        style("→").dim(),
        style(config_path().display()).dim(),
    );
    println!("    keystore_path, key_alias, google_play_key_path");
    println!();
    println!(
        "  {} {} {}",
        style("Project").bold(),
        style("→").dim(),
        style(perry_toml_path.display()).dim(),
    );
    println!("    keystore, key_alias, google_play_key, distribute");
    println!();
    println!("  Tip: to target a specific track, use:");
    println!(
        "  distribute = \"playstore:beta\"  {} :internal, :alpha, :beta, :production",
        style("#").dim()
    );
    println!();
    println!("  Then run: {}", style("perry publish android").bold());

    Ok(())
}

// ---------------------------------------------------------------------------
// iOS wizard
// ---------------------------------------------------------------------------
