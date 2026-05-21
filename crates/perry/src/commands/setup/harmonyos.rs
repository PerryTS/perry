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

pub fn harmonyos_wizard(saved: &mut PerryConfig) -> Result<()> {
    println!("  {}", style("HarmonyOS Setup").bold());
    println!();
    println!("  Perry signs HarmonyOS HAPs with a Huawei-issued development cert +");
    println!("  provisioning profile. DevEco Studio auto-generates these the first");
    println!("  time you build a HarmonyOS project; this wizard configures Perry");
    println!("  to use them so `perry compile --target harmonyos` produces a");
    println!("  signed HAP that `hdc install` can deploy to the emulator or device.");
    println!();

    let existing = saved.harmonyos.clone().unwrap_or_default();

    // ---- Step 1: locate the signing materials ----
    println!(
        "  {} Locate signing materials",
        style("Step 1/3 —").cyan().bold()
    );
    println!();

    let ohos_config = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("could not determine home directory"))?
        .join(".ohos")
        .join("config");

    if !ohos_config.is_dir() {
        bail!(
            "  {}: ~/.ohos/config/ doesn't exist.\n\n  \
             DevEco Studio creates this directory the first time you build a\n  \
             HarmonyOS project with auto-debug signing enabled. Open DevEco,\n  \
             create or open any HarmonyOS app, click ▶ Run on the emulator, and\n  \
             let it generate the debug cert. Then re-run `perry setup harmonyos`.",
            style("Not found").red().bold()
        );
    }

    // Probe for the matching .p12 / .p7b / .cer triple.
    let entries: Vec<_> = std::fs::read_dir(&ohos_config)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();

    let p12_candidates: Vec<PathBuf> = entries
        .iter()
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("p12"))
        .cloned()
        .collect();

    if p12_candidates.is_empty() {
        bail!(
            "  {}: no .p12 keystore found in {}\n\n  \
             DevEco's auto-debug cert workflow should produce a file named\n  \
             `default_<project>_<hash>.p12`. If it didn't, open DevEco, click\n  \
             ▶ Run on a HarmonyOS project, accept the auto-sign prompt, and\n  \
             re-run this wizard.",
            style("Not found").red().bold(),
            ohos_config.display()
        );
    }

    let p12_path = if p12_candidates.len() == 1 {
        let only = p12_candidates[0].clone();
        println!(
            "  Found {} signing materials in {}:",
            style("1").green(),
            ohos_config.display()
        );
        println!(
            "    {}",
            only.file_name().unwrap_or_default().to_string_lossy()
        );
        println!();
        only
    } else {
        println!(
            "  Found {} signing materials in {}:",
            style(p12_candidates.len().to_string()).green(),
            ohos_config.display()
        );
        let labels: Vec<String> = p12_candidates
            .iter()
            .map(|p| {
                p.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
            })
            .collect();
        let label_refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
        let pick = Select::new()
            .with_prompt("  Which keystore to use?")
            .items(&label_refs)
            .default(0)
            .interact()?;
        p12_candidates[pick].clone()
    };

    // The matching .p7b + .cer live next to the .p12 with the same stem.
    let stem = p12_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("invalid .p12 filename"))?;
    let profile_path = ohos_config.join(format!("{}.p7b", stem));
    let cert_path = ohos_config.join(format!("{}.cer", stem));
    if !profile_path.exists() {
        bail!(
            "  {}: provisioning profile {} not found.\n  \
             DevEco's auto-sign workflow should produce both .p12 and .p7b together.",
            style("Mismatch").red().bold(),
            profile_path.display()
        );
    }
    if !cert_path.exists() {
        bail!(
            "  {}: cert chain {} not found.\n  \
             DevEco's auto-sign workflow should produce a matching .cer alongside\n  \
             the .p12 and .p7b. If it didn't, regenerate the signing config in\n  \
             DevEco (Project Structure → Signing Configs → Generate).",
            style("Mismatch").red().bold(),
            cert_path.display()
        );
    }

    // Extract the bundleName from the .p7b's embedded JSON. The profile is a
    // PKCS#7 envelope; the inner JSON contains a "bundle-name" field. Rather
    // than parse PKCS#7, we just grep the binary for the JSON object — the
    // bundle-name string is plain UTF-8 inside.
    let profile_bytes = std::fs::read(&profile_path)?;
    let profile_text = String::from_utf8_lossy(&profile_bytes);
    let bundle_name = profile_text.find("\"bundle-name\":\"").and_then(|i| {
        let after = &profile_text[i + 15..];
        after.find('"').map(|j| after[..j].to_string())
    });

    println!("  {} {}", style("✓").green(), p12_path.display());
    println!("  {} {}", style("✓").green(), profile_path.display());
    println!("  {} {}", style("✓").green(), cert_path.display());
    if let Some(ref bn) = bundle_name {
        println!("  {} bundle: {}", style("✓").green(), style(bn).cyan());
    }
    println!();

    // ---- Step 2: keystore password ----
    println!(
        "  {} p12 keystore password",
        style("Step 2/3 —").cyan().bold()
    );
    println!();
    println!("  DevEco encrypts the password in build-profile.json5 with a");
    println!("  machine-bound key that's not externally accessible. The simplest");
    println!("  way to obtain a password Perry can use:");
    println!();
    println!("    1. In DevEco, delete the existing signing config:");
    println!("       File → Project Structure → Signing Configs → Delete \"default\"");
    println!("    2. Click ▶ Run on the emulator. DevEco regenerates the cert");
    println!("       and PROMPTS you for a new password — choose anything memorable");
    println!("       (e.g. \"perrytest\"). Use the same password for both fields.");
    println!("    3. Paste that password below.");
    println!();
    println!("  Note: the password is stored plaintext in ~/.perry/config.toml");
    println!("  (file is in your home dir, not world-readable). macOS Keychain");
    println!("  integration is a future improvement.");
    println!();

    let password = if let Some(existing_pw) = existing.p12_password.as_ref() {
        let reuse = Confirm::new()
            .with_prompt("  Reuse the previously-saved password?")
            .default(true)
            .interact()?;
        if reuse {
            existing_pw.clone()
        } else {
            prompt_password()?
        }
    } else {
        prompt_password()?
    };

    // Verify the password actually unlocks the keystore via `keytool -list`.
    // If it fails, prompt again or bail — saving a wrong password would just
    // produce confusing errors at compile time.
    println!();
    print!("  Verifying password... ");
    use std::io::Write as _;
    std::io::stdout().flush().ok();
    let keytool_ok = Command::new("keytool")
        .arg("-list")
        .arg("-keystore")
        .arg(&p12_path)
        .arg("-storetype")
        .arg("PKCS12")
        .arg("-storepass")
        .arg(&password)
        .output()
        .map(|out| {
            out.status.success() && String::from_utf8_lossy(&out.stdout).contains("Your keystore")
        })
        .unwrap_or(false);
    if !keytool_ok {
        println!("{}", style("FAILED").red().bold());
        println!();
        bail!(
            "  Password didn't unlock {}. Re-run the wizard once you have the\n  \
             correct password (regenerate via DevEco GUI per the instructions above).",
            p12_path.display()
        );
    }
    println!("{}", style("OK").green());

    // ---- Step 3: bundle name ----
    println!();
    println!("  {} Bundle name", style("Step 3/3 —").cyan().bold());
    println!();

    let bundle_to_save = if let Some(ref auto_bn) = bundle_name {
        println!("  HarmonyOS provisioning profiles are bound to a specific bundleName.");
        println!("  Profile is bound to: {}", style(auto_bn).cyan().bold());
        println!();
        let use_auto = Confirm::new()
            .with_prompt("  Use this bundle name?")
            .default(true)
            .interact()?;
        if use_auto {
            auto_bn.clone()
        } else {
            Input::<String>::new()
                .with_prompt("  Bundle name")
                .interact_text()?
        }
    } else {
        println!(
            "  Couldn't auto-extract bundleName from {} — please enter it",
            profile_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        );
        println!("  manually. (You can find it in DevEco's app.json5 under `app.bundleName`.)");
        println!();
        Input::<String>::new()
            .with_prompt("  Bundle name")
            .interact_text()?
    };

    // Most users have DevEco's auto-generated cert which uses the alias `debugKey`.
    // Allow override for users who imported their own keystore.
    let key_alias = existing
        .key_alias
        .clone()
        .unwrap_or_else(|| "debugKey".to_string());

    // ---- Persist ----
    saved.harmonyos = Some(HarmonyosSavedConfig {
        p12_path: Some(p12_path.to_string_lossy().to_string()),
        p12_password: Some(password),
        profile_path: Some(profile_path.to_string_lossy().to_string()),
        cert_path: Some(cert_path.to_string_lossy().to_string()),
        bundle_name: Some(bundle_to_save.clone()),
        key_alias: Some(key_alias),
    });

    println!();
    println!("  {} HarmonyOS setup complete.", style("✓").green().bold());
    println!();
    println!("  Saved to {}", config_path().display());
    println!("  Try it now:");
    println!();
    println!(
        "    {}",
        style("perry compile your-app.ts --target harmonyos -o /tmp/your-app.so").dim()
    );
    println!("    {}", style("hdc install /tmp/your-app.hap").dim());
    println!();

    Ok(())
}

pub fn prompt_password() -> Result<String> {
    Password::new()
        .with_prompt("  p12 password")
        .with_confirmation("  Confirm password", "  Passwords don't match — try again.")
        .interact()
        .map_err(Into::into)
}
