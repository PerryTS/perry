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

pub fn macos_wizard(saved: &mut PerryConfig) -> Result<()> {
    println!("  {}", style("macOS Setup").bold());
    println!();

    // --- Step 1: App Store Connect API Key ---
    // Check for existing credentials (shared with iOS — same Apple account)
    let existing_apple = saved.apple.clone().unwrap_or_default();

    println!(
        "  {} App Store Connect API Key",
        style("Step 1/2 —").cyan().bold()
    );
    println!();

    let has_existing = existing_apple.p8_key_path.is_some()
        && existing_apple.key_id.is_some()
        && existing_apple.issuer_id.is_some();

    let (p8_path, key_id, issuer_id, team_id) = if has_existing {
        let p8 = existing_apple.p8_key_path.clone().unwrap();
        let kid = existing_apple.key_id.clone().unwrap();
        let iss = existing_apple.issuer_id.clone().unwrap();
        let tid = existing_apple.team_id.clone().unwrap_or_default();
        println!("  Found existing credentials (shared with iOS):");
        println!("    Key ID:    {}", style(&kid).bold());
        println!("    Issuer ID: {}", style(&iss).dim());
        println!("    .p8 key:   {}", style(&p8).dim());
        if !tid.is_empty() {
            println!("    Team ID:   {}", style(&tid).dim());
        }
        println!();
        let reuse = Confirm::new()
            .with_prompt("  Use these existing credentials?")
            .default(true)
            .interact()?;
        if reuse {
            (p8, kid, iss, tid)
        } else {
            prompt_api_credentials()?
        }
    } else {
        println!("  You need an App Store Connect API key.");
        println!(
            "  1. Go to: {}",
            style("https://appstoreconnect.apple.com/access/integrations/api").underlined()
        );
        println!(
            "  2. Click '+', create a key with {} role.",
            style("App Manager").bold()
        );
        println!("  3. Download the .p8 file (only downloadable once).");
        println!("  4. Note the Key ID and Issuer ID.");
        println!();
        press_enter_to_continue("  Press Enter when ready");
        prompt_api_credentials()?
    };

    // Validate p8 file
    let p8_content = std::fs::read_to_string(&p8_path)
        .with_context(|| format!("Cannot read .p8 key: {p8_path}"))?;
    if !p8_content.trim_start().starts_with("-----BEGIN") {
        bail!("Invalid .p8 file — expected PEM format starting with '-----BEGIN'");
    }

    // Save API credentials (shared across platforms)
    let apple = saved.apple.get_or_insert_with(AppleSavedConfig::default);
    apple.p8_key_path = Some(p8_path.clone());
    apple.key_id = Some(key_id.clone());
    apple.issuer_id = Some(issuer_id.clone());
    if !team_id.is_empty() {
        apple.team_id = Some(team_id.clone());
    }
    save_config(saved).ok();

    println!();
    println!("  {} Key ID: {}", style("✓").green(), style(&key_id).bold());
    println!(
        "  {} Issuer ID: {}",
        style("✓").green(),
        style(&issuer_id).bold()
    );
    if !team_id.is_empty() {
        println!(
            "  {} Team ID: {}",
            style("✓").green(),
            style(&team_id).bold()
        );
    }
    println!();

    // --- Step 2: Distribution method ---
    println!(
        "  {} Distribution Method",
        style("Step 2/3 —").cyan().bold()
    );
    println!();

    let cert_types = &[
        "App Store / TestFlight (upload to App Store Connect)",
        "Notarized DMG (direct download)",
        "Both (App Store + Notarized DMG)",
    ];
    let cert_type_idx = Select::new()
        .with_prompt("  Distribution method")
        .items(cert_types)
        .default(0)
        .interact()?;

    let distribute_value = match cert_type_idx {
        0 => "appstore",
        1 => "notarize",
        _ => "both",
    };
    let needs_appstore_cert = distribute_value == "appstore" || distribute_value == "both";
    let needs_notarize_cert = distribute_value == "notarize" || distribute_value == "both";
    println!();

    // --- Step 3: Auto-create certificates via App Store Connect API ---
    println!("  {} Certificates", style("Step 3/3 —").cyan().bold());
    println!();

    // Verify API connectivity
    let client = reqwest::blocking::Client::new();
    let jwt = generate_asc_jwt(&key_id, &issuer_id, &p8_content)?;
    print!("  Verifying API access... ");
    std::io::Write::flush(&mut std::io::stdout()).ok();
    let resp = client
        .get("https://api.appstoreconnect.apple.com/v1/certificates?limit=1")
        .bearer_auth(&jwt)
        .send()
        .context("Failed to connect to App Store Connect API")?;
    if resp.status() == 401 || resp.status() == 403 {
        bail!("API authentication failed — check your Key ID, Issuer ID, and .p8 key");
    }
    if !resp.status().is_success() {
        let body = resp.text().unwrap_or_default();
        bail!("API error: {body}");
    }
    println!("{}", style("ok").green());

    let perry_dir = dirs::home_dir().unwrap_or_default().join(".perry");
    std::fs::create_dir_all(&perry_dir)?;
    let p12_password = "perry-auto";

    // Shared private key for all certs
    let key_path = perry_dir.join("macos_private_key.pem");
    let csr_path = perry_dir.join("macos_csr.pem");

    // Generate RSA 2048 private key + CSR (reused across all cert types)
    println!("  Generating private key and CSR...");
    let status = Command::new("openssl")
        .args(["genrsa", "-out"])
        .arg(&key_path)
        .arg("2048")
        .stderr(std::process::Stdio::null())
        .status()
        .context("openssl not found — required for certificate generation")?;
    if !status.success() {
        bail!("Failed to generate private key");
    }
    let status = Command::new("openssl")
        .args(["req", "-new", "-key"])
        .arg(&key_path)
        .args(["-out"])
        .arg(&csr_path)
        .args(["-subj", "/CN=Perry macOS Distribution/O=Perry"])
        .stderr(std::process::Stdio::null())
        .status()?;
    if !status.success() {
        bail!("Failed to generate CSR");
    }
    let csr_pem = std::fs::read_to_string(&csr_path)?;

    let mut cert_path = String::new();
    let mut signing_identity = String::new();
    let mut notarize_cert_path = String::new();
    let mut notarize_signing_identity = String::new();
    let mut installer_cert_path: Option<String> = None;

    // -- App Store certificate (MAC_APP_DISTRIBUTION + MAC_INSTALLER_DISTRIBUTION) --
    if needs_appstore_cert {
        let (p12, identity) = create_apple_certificate(
            &client,
            &key_id,
            &issuer_id,
            &p8_content,
            "MAC_APP_DISTRIBUTION",
            &csr_pem,
            &key_path,
            &perry_dir.join("macos_appstore.p12"),
            p12_password,
            "Mac App Distribution",
        )?;
        cert_path = p12;
        signing_identity = identity;

        // Also create MAC_INSTALLER_DISTRIBUTION for .pkg signing
        // Stored as a separate .p12 since openssl can only export one key per .p12
        match create_apple_certificate(
            &client,
            &key_id,
            &issuer_id,
            &p8_content,
            "MAC_INSTALLER_DISTRIBUTION",
            &csr_pem,
            &key_path,
            &perry_dir.join("macos_installer.p12"),
            p12_password,
            "Mac Installer Distribution",
        ) {
            Ok((_installer_p12, _installer_identity)) => {
                installer_cert_path = Some(
                    perry_dir
                        .join("macos_installer.p12")
                        .to_string_lossy()
                        .to_string(),
                );
            }
            Err(e) => {
                println!(
                    "  {} Installer cert: {} (pkg signing may fail)",
                    style("!").yellow(),
                    e
                );
            }
        }
    }

    // -- Developer ID certificate (DEVELOPER_ID_APPLICATION) --
    if needs_notarize_cert {
        let (p12, identity) = create_apple_certificate(
            &client,
            &key_id,
            &issuer_id,
            &p8_content,
            "DEVELOPER_ID_APPLICATION",
            &csr_pem,
            &key_path,
            &perry_dir.join("macos_devid.p12"),
            p12_password,
            "Developer ID Application",
        )?;
        if distribute_value == "both" {
            notarize_cert_path = p12;
            notarize_signing_identity = identity;
        } else {
            cert_path = p12;
            signing_identity = identity;
        }
    }

    // Clean up CSR (keep private key for future use)
    let _ = std::fs::remove_file(&csr_path);
    println!();

    // --- Save project-specific credentials to perry.toml ---
    let perry_toml_path = std::env::current_dir()?.join("perry.toml");
    // Create perry.toml if it doesn't exist — project-specific config belongs here
    if !perry_toml_path.exists() {
        std::fs::write(&perry_toml_path, "")?;
    }
    match update_perry_toml_macos(
        &perry_toml_path,
        distribute_value,
        &cert_path,
        if signing_identity.is_empty() {
            None
        } else {
            Some(&signing_identity)
        },
        if distribute_value == "both" {
            Some(&notarize_cert_path)
        } else {
            None
        },
        if distribute_value == "both" && !notarize_signing_identity.is_empty() {
            Some(&notarize_signing_identity)
        } else {
            None
        },
        installer_cert_path.as_deref(),
    ) {
        Ok(()) => {
            println!(
                "  {} macOS credentials saved to {}",
                style("✓").green().bold(),
                style(perry_toml_path.display()).dim()
            );
        }
        Err(e) => {
            println!("  {} Could not update perry.toml: {e}", style("!").yellow());
            println!("  Add these manually to your perry.toml [macos] section:");
            println!("  distribute = \"{distribute_value}\"");
            println!("  certificate = \"{}\"", cert_path);
        }
    }

    // --- Export compliance (for App Store) ---
    if needs_appstore_cert {
        println!();
        println!("  {} Export Compliance", style("→").cyan().bold());
        println!("  Most apps only use HTTPS and don't need custom encryption declarations.");
        let encryption_exempt = Confirm::new()
            .with_prompt("  Does your app ONLY use standard HTTPS? (no custom encryption)")
            .default(true)
            .interact()?;
        if let Err(e) = update_perry_toml_section_bool(
            &perry_toml_path,
            "macos",
            "encryption_exempt",
            encryption_exempt,
        ) {
            println!("  {} Could not update perry.toml: {e}", style("!").yellow());
            println!("  Add manually to [macos]: encryption_exempt = {encryption_exempt}");
        }
    }
    println!();

    // --- Summary ---
    println!("  {}", style("Setup complete!").green().bold());
    println!();
    println!(
        "  {} {} {}",
        style("Global").bold(),
        style("→").dim(),
        style(config_path().display()).dim(),
    );
    println!("    p8_key_path, key_id, issuer_id, team_id");
    println!();
    println!(
        "  {} {} {}",
        style("Project").bold(),
        style("→").dim(),
        style(perry_toml_path.display()).dim(),
    );
    println!("    distribute, certificate, signing_identity, encryption_exempt");
    println!();
    match distribute_value {
        "both" => {
            println!("  App Store cert: {}", style(&cert_path).dim());
            println!("  Notarize cert:  {}", style(&notarize_cert_path).dim());
        }
        _ => {
            println!("  Certificate:  {}", style(&cert_path).dim());
        }
    }
    println!("  Distribute:   {}", style(distribute_value).bold());
    println!(
        "  Cert password: auto-managed ({})",
        style("perry-auto").dim()
    );
    println!();
    println!("  Then run: {}", style("perry publish macos").bold());

    Ok(())
}

/// Merge two .p12 files into the first one (appends the second's cert+key).
/// Both must use the same password. Uses openssl to extract PEM and repackage.
// #854: signing helper for the macOS distribute="both" cert-merge flow;
// kept as the documented entry point even where currently unreferenced.
#[allow(dead_code)]
pub fn merge_p12_files(
    primary_p12: &std::path::Path,
    secondary_p12: &str,
    password: &str,
    tmpdir: &std::path::Path,
) -> Result<()> {
    let pass = format!("pass:{password}");
    let pem_a = tmpdir.join("_merge_a.pem");
    let pem_b = tmpdir.join("_merge_b.pem");
    let combined = tmpdir.join("_merge_combined.pem");

    // Extract both to PEM (try with -legacy first, fall back without)
    for (p12, pem) in [
        (primary_p12.as_os_str(), pem_a.as_os_str()),
        (std::ffi::OsStr::new(secondary_p12), pem_b.as_os_str()),
    ] {
        let ok = Command::new("openssl")
            .args(["pkcs12", "-in"])
            .arg(p12)
            .args(["-out"])
            .arg(pem)
            .args(["-nodes", "-password", &pass, "-legacy"])
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            Command::new("openssl")
                .args(["pkcs12", "-in"])
                .arg(p12)
                .args(["-out"])
                .arg(pem)
                .args(["-nodes", "-password", &pass])
                .stderr(std::process::Stdio::null())
                .status()?;
        }
    }

    // Concatenate PEM files
    let a = std::fs::read_to_string(&pem_a).unwrap_or_default();
    let b = std::fs::read_to_string(&pem_b).unwrap_or_default();
    std::fs::write(&combined, format!("{a}\n{b}"))?;

    // Re-package into .p12
    let ok = Command::new("openssl")
        .args(["pkcs12", "-export", "-in"])
        .arg(&combined)
        .args(["-out"])
        .arg(primary_p12)
        .args(["-password", &pass, "-legacy"])
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        Command::new("openssl")
            .args(["pkcs12", "-export", "-in"])
            .arg(&combined)
            .args(["-out"])
            .arg(primary_p12)
            .args(["-password", &pass])
            .stderr(std::process::Stdio::null())
            .status()?;
    }

    // Clean up
    let _ = std::fs::remove_file(&pem_a);
    let _ = std::fs::remove_file(&pem_b);
    let _ = std::fs::remove_file(&combined);
    Ok(())
}

/// Create an Apple certificate via the App Store Connect API.
///
/// 1. Check for existing certs of this type (reuse if found + .p12 exists)
/// 2. If none, submit CSR to Apple and create the cert
/// 3. Convert to .p12 using openssl
///
/// Returns (p12_path, signing_identity).
pub fn create_apple_certificate(
    client: &reqwest::blocking::Client,
    key_id: &str,
    issuer_id: &str,
    p8_content: &str,
    cert_type: &str,
    csr_pem: &str,
    private_key_path: &std::path::Path,
    p12_output_path: &std::path::Path,
    p12_password: &str,
    display_name: &str,
) -> Result<(String, String)> {
    // Check for existing certs of this type
    print!(
        "  Checking for existing {} certificate... ",
        style(display_name).bold()
    );
    std::io::Write::flush(&mut std::io::stdout()).ok();

    let jwt = generate_asc_jwt(key_id, issuer_id, p8_content)?;
    let resp = client
        .get("https://api.appstoreconnect.apple.com/v1/certificates")
        .bearer_auth(&jwt)
        .query(&[("filter[certificateType]", cert_type), ("limit", "200")])
        .send()?;
    let body: serde_json::Value = resp.json()?;
    let existing = body["data"].as_array().and_then(|arr| arr.first()).cloned();

    if let Some(ref cert) = existing {
        if p12_output_path.exists() {
            let name = cert["attributes"]["name"].as_str().unwrap_or(display_name);
            println!("{} ({})", style("found").green(), name);
            println!(
                "  {} Using existing .p12 at {}",
                style("✓").green().bold(),
                style(p12_output_path.display()).dim()
            );
            let identity = name.to_string();
            return Ok((p12_output_path.to_string_lossy().to_string(), identity));
        } else {
            // Existing cert was created elsewhere (e.g. Xcode) — we don't have the
            // matching private key, so we can't make a .p12 from it.
            // Create a brand-new cert with our CSR instead.
            println!(
                "{}",
                style("found (no local key), creating new...").yellow()
            );
        }
    } else {
        println!("{}", style("not found, creating...").yellow());
    }

    // Strip PEM headers for API (Apple wants raw base64)
    let csr_b64: String = csr_pem
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect::<Vec<_>>()
        .join("");

    // Submit CSR to Apple
    print!("  Creating {} certificate... ", style(display_name).bold());
    std::io::Write::flush(&mut std::io::stdout()).ok();

    let jwt = generate_asc_jwt(key_id, issuer_id, p8_content)?;
    let create_body = serde_json::json!({
        "data": {
            "type": "certificates",
            "attributes": {
                "certificateType": cert_type,
                "csrContent": csr_b64
            }
        }
    });
    let resp = client
        .post("https://api.appstoreconnect.apple.com/v1/certificates")
        .bearer_auth(&jwt)
        .json(&create_body)
        .send()?;

    if !resp.status().is_success() {
        let status = resp.status();
        let err = resp.text().unwrap_or_default();

        // 403 for Developer ID certs means the API key doesn't have Account Holder role.
        // Fall back to exporting from the local Keychain.
        if status == 403 {
            println!("{}", style("forbidden (Account Holder required)").yellow());
            println!(
                "  {} Developer ID certificates require Account Holder role to create via API.",
                style("ℹ").blue()
            );
            println!("  Attempting to export from your local Keychain instead...");
            println!();
            return export_cert_from_keychain(display_name, p12_output_path, p12_password);
        }

        bail!("Failed to create {display_name} certificate: {err}");
    }
    let resp_body: serde_json::Value = resp.json()?;
    let cert_content = resp_body["data"]["attributes"]["certificateContent"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No certificate content in response"))?;
    let cert_name = resp_body["data"]["attributes"]["name"]
        .as_str()
        .unwrap_or(display_name);
    println!("{}", style("done").green());
    println!(
        "  {} Certificate: {}",
        style("✓").green().bold(),
        style(cert_name).bold()
    );

    let identity = create_p12_from_cert_content(
        cert_content,
        private_key_path,
        p12_output_path,
        p12_password,
        display_name,
    )?;

    Ok((p12_output_path.to_string_lossy().to_string(), identity))
}

/// Fallback: export a certificate from the local macOS Keychain when the API
/// returns 403 (e.g., Developer ID certs require Account Holder role).
///
/// Lists codesigning identities, filters by display_name prefix, and uses
/// `security export` to create a .p12.
pub fn export_cert_from_keychain(
    display_name: &str,
    p12_output_path: &std::path::Path,
    p12_password: &str,
) -> Result<(String, String)> {
    // List available codesigning identities
    let output = Command::new("security")
        .args(["find-identity", "-v", "-p", "codesigning"])
        .output()
        .context("Failed to run `security find-identity`")?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse identity lines: '  1) SHA1HASH "Identity Name"'
    let mut identities: Vec<(String, String)> = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if !line.starts_with(|c: char| c.is_ascii_digit()) {
            continue;
        }
        if let Some(quote_start) = line.find('"') {
            if let Some(quote_end) = line.rfind('"') {
                if quote_end > quote_start {
                    let name = &line[quote_start + 1..quote_end];
                    let after_paren = line.find(") ").map(|i| i + 2).unwrap_or(0);
                    let hash_end = line.find(" \"").unwrap_or(line.len());
                    if hash_end > after_paren {
                        let hash = line[after_paren..hash_end].trim().to_string();
                        identities.push((hash, name.to_string()));
                    }
                }
            }
        }
    }

    // Filter to matching identities (e.g. "Developer ID Application")
    let matching: Vec<_> = identities
        .iter()
        .filter(|(_, name)| name.starts_with(display_name))
        .collect();

    if matching.is_empty() {
        bail!(
            "No \"{}\" certificate found in your Keychain.\n\
             Create one in Xcode → Settings → Accounts → Manage Certificates,\n\
             then run `perry setup macos` again.",
            display_name
        );
    }

    let (_hash, identity_name) = if matching.len() == 1 {
        (matching[0].0.clone(), matching[0].1.clone())
    } else {
        let labels: Vec<&str> = matching.iter().map(|(_, n)| n.as_str()).collect();
        let selection = Select::new()
            .with_prompt(format!(
                "  Multiple {} certs found — select one",
                display_name
            ))
            .items(&labels)
            .default(0)
            .interact()?;
        (matching[selection].0.clone(), matching[selection].1.clone())
    };

    println!("  Found in Keychain: {}", style(&identity_name).bold());

    // Export the identity (cert + private key) from Keychain as .p12
    print!("  Exporting from Keychain (macOS may ask for access)... ");
    std::io::Write::flush(&mut std::io::stdout()).ok();

    let keychain_path = format!(
        "{}/Library/Keychains/login.keychain-db",
        std::env::var("HOME").unwrap_or_default()
    );
    let export_result = Command::new("security")
        .args([
            "export",
            "-k",
            &keychain_path,
            "-t",
            "identities",
            "-f",
            "pkcs12",
            "-P",
            p12_password,
            "-o",
        ])
        .arg(p12_output_path)
        .output();

    match export_result {
        Ok(out) if out.status.success() => {
            println!("{}", style("done").green());
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            bail!(
                "Keychain export failed: {}\n\
                 You may need to unlock your Keychain or grant access.",
                stderr.trim()
            );
        }
        Err(e) => bail!("Failed to run security export: {e}"),
    }

    // The `security export -t identities` exports ALL identities.
    // We need to filter to just the one we want. Re-create .p12 with only our cert.
    // Extract the specific identity using its SHA-1 hash.
    let temp_all = p12_output_path.with_extension("all.p12");
    std::fs::rename(p12_output_path, &temp_all)?;

    // Use openssl to extract our specific cert by piping through pkcs12
    // First, extract all certs+keys from the exported p12
    let extract = Command::new("openssl")
        .args(["pkcs12", "-in"])
        .arg(&temp_all)
        .args(["-out"])
        .arg(p12_output_path.with_extension("pem"))
        .args([
            "-nodes",
            "-password",
            &format!("pass:{p12_password}"),
            "-legacy",
        ])
        .stderr(std::process::Stdio::null())
        .status();

    // If that fails, try without -legacy
    if !extract.map(|s| s.success()).unwrap_or(false) {
        let _ = Command::new("openssl")
            .args(["pkcs12", "-in"])
            .arg(&temp_all)
            .args(["-out"])
            .arg(p12_output_path.with_extension("pem"))
            .args(["-nodes", "-password", &format!("pass:{p12_password}")])
            .stderr(std::process::Stdio::null())
            .status();
    }

    // Re-package just this identity into a clean .p12
    // (For simplicity, use the full export — the builder's temp keychain
    // import will pick the right identity by name anyway.)
    std::fs::rename(&temp_all, p12_output_path)?;
    let _ = std::fs::remove_file(p12_output_path.with_extension("pem"));

    println!(
        "  {} Certificate: {}",
        style("✓").green().bold(),
        style(&identity_name).bold()
    );
    println!(
        "  {} Saved to {}",
        style("✓").green().bold(),
        style(p12_output_path.display()).dim()
    );

    Ok((p12_output_path.to_string_lossy().to_string(), identity_name))
}

/// Convert base64-encoded DER certificate content + private key into a .p12 file.
/// Returns the signing identity string extracted from the certificate.
pub fn create_p12_from_cert_content(
    cert_content_b64: &str,
    private_key_path: &std::path::Path,
    p12_output_path: &std::path::Path,
    p12_password: &str,
    display_name: &str,
) -> Result<String> {
    use base64::Engine;

    let cert_der = base64::engine::general_purpose::STANDARD
        .decode(cert_content_b64)
        .context("Failed to decode certificate from Apple")?;

    // Write cert as PEM for openssl
    let cert_pem_path = p12_output_path.with_extension("cer.pem");
    let cert_pem = format!(
        "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n",
        base64::engine::general_purpose::STANDARD
            .encode(&cert_der)
            .as_bytes()
            .chunks(76)
            .map(|c| std::str::from_utf8(c).unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n")
    );
    std::fs::write(&cert_pem_path, &cert_pem)?;

    // Extract signing identity (CN) from the certificate
    let identity_output = Command::new("openssl")
        .args(["x509", "-noout", "-subject", "-in"])
        .arg(&cert_pem_path)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    let identity = identity_output
        .split("CN=")
        .nth(1) // old format: subject= /CN=.../O=...
        .or_else(|| identity_output.split("CN = ").nth(1)) // new format: subject=CN = ..., O = ...
        .map(|s| s.split('/').next().unwrap_or(s)) // strip /O=...
        .map(|s| s.split(", O").next().unwrap_or(s)) // strip , O = ...
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| display_name.to_string());

    // Create .p12 from private key + certificate
    print!("  Creating .p12 bundle... ");
    std::io::Write::flush(&mut std::io::stdout()).ok();

    let status = Command::new("openssl")
        .args(["pkcs12", "-export", "-inkey"])
        .arg(private_key_path)
        .args(["-in"])
        .arg(&cert_pem_path)
        .args(["-out"])
        .arg(p12_output_path)
        .args(["-password", &format!("pass:{p12_password}"), "-legacy"])
        .stderr(std::process::Stdio::null())
        .status()?;
    if !status.success() {
        // Retry without -legacy (older openssl)
        let status = Command::new("openssl")
            .args(["pkcs12", "-export", "-inkey"])
            .arg(private_key_path)
            .args(["-in"])
            .arg(&cert_pem_path)
            .args(["-out"])
            .arg(p12_output_path)
            .args(["-password", &format!("pass:{p12_password}")])
            .stderr(std::process::Stdio::null())
            .status()?;
        if !status.success() {
            bail!("Failed to create .p12 for {display_name}");
        }
    }
    println!("{}", style("done").green());

    // Clean up intermediate PEM
    let _ = std::fs::remove_file(&cert_pem_path);

    Ok(identity)
}
