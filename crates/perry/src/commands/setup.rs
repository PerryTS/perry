//! `perry setup` — guided credential setup wizard for App Store / Google Play distribution
//! (and toolchain setup for the "lightweight" Windows target — LLVM + xwin'd SDK)

use anyhow::{bail, Context, Result};
use clap::Args;
use console::style;
use dialoguer::{Confirm, Input, Password, Select};
use std::path::PathBuf;
use std::process::Command;

use super::publish::{
    AndroidSavedConfig, AppleSavedConfig, HarmonyosSavedConfig, PerryConfig,
    config_path, is_interactive, load_config, save_config,
};

#[derive(Args, Debug)]
pub struct SetupArgs {
    /// Platform to configure: android, ios, visionos, macos, tvos, watchos, windows, harmonyos
    pub platform: Option<String>,

    /// (windows only) Accept the Microsoft Visual Studio Build Tools redistributable
    /// license required to download CRT + Windows SDK via xwin. Equivalent to
    /// answering "yes" at the interactive prompt; enables non-interactive / CI use.
    #[arg(long)]
    pub accept_license: bool,
}

pub fn run(args: SetupArgs) -> Result<()> {
    // Credential wizards always need interactive (secret prompts); the windows
    // toolchain wizard accepts --accept-license for CI.
    let is_windows_platform = args.platform.as_deref() == Some("windows");
    let needs_interactive = !(is_windows_platform && args.accept_license);
    if needs_interactive && !is_interactive() {
        bail!("`perry setup` requires an interactive terminal (pass --accept-license for `windows` to skip)");
    }

    println!();
    println!("  {} Perry Setup", style("▶").cyan().bold());
    println!();

    let platform = match args.platform.as_deref() {
        Some(p) => p.to_string(),
        None => {
            let options = &["Android", "iOS", "visionOS", "macOS", "tvOS", "watchOS", "Windows (toolchain)", "HarmonyOS"];
            let selection = Select::new()
                .with_prompt("  Which platform to configure?")
                .items(options)
                .default(0)
                .interact()?;
            match selection {
                0 => "android".to_string(),
                1 => "ios".to_string(),
                2 => "visionos".to_string(),
                3 => "macos".to_string(),
                4 => "tvos".to_string(),
                5 => "watchos".to_string(),
                6 => "windows".to_string(),
                _ => "harmonyos".to_string(),
            }
        }
    };

    // Windows is a toolchain setup (not a credential wizard) — doesn't touch PerryConfig.
    if platform == "windows" {
        return windows_wizard(args.accept_license);
    }

    let mut saved = load_config();

    match platform.as_str() {
        "android" => android_wizard(&mut saved)?,
        "ios" => ios_wizard(&mut saved)?,
        "visionos" => visionos_wizard(&mut saved)?,
        "macos" => macos_wizard(&mut saved)?,
        "tvos" => tvos_wizard(&mut saved)?,
        "watchos" => watchos_wizard(&mut saved)?,
        "harmonyos" => harmonyos_wizard(&mut saved)?,
        other => bail!("Unknown platform '{other}'. Use: android, ios, visionos, macos, tvos, watchos, windows, harmonyos"),
    }

    save_config(&saved)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Windows toolchain wizard (lightweight path — LLVM + xwin'd SDK, no MSVC)
// ---------------------------------------------------------------------------

/// Locate `xwin.exe`:
///   1. Bundled next to `perry.exe` (the release zip ships it there)
///   2. `~/.cargo/bin/xwin.exe` (for dev installs via `cargo install xwin`)
///   3. PATH lookup
fn find_xwin_exe() -> Option<PathBuf> {
    if let Ok(perry) = std::env::current_exe() {
        if let Some(dir) = perry.parent() {
            let bundled = dir.join(if cfg!(windows) { "xwin.exe" } else { "xwin" });
            if bundled.exists() {
                return Some(bundled);
            }
        }
    }
    if let Some(home) = dirs::home_dir() {
        let cargo_bin = home
            .join(".cargo")
            .join("bin")
            .join(if cfg!(windows) { "xwin.exe" } else { "xwin" });
        if cargo_bin.exists() {
            return Some(cargo_bin);
        }
    }
    let which_cmd = if cfg!(windows) { "where" } else { "which" };
    if let Ok(out) = Command::new(which_cmd).arg("xwin").output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if let Some(first) = s.lines().next() {
                let p = PathBuf::from(first);
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }
    None
}

pub(crate) fn windows_wizard(accept_license: bool) -> Result<()> {
    use std::time::Instant;

    println!("  {}", style("Windows Toolchain Setup").bold());
    println!();
    println!("  This downloads the Microsoft CRT + Windows SDK libraries (via xwin)");
    println!("  so Perry can link Windows executables without Visual Studio Build Tools.");
    println!();

    // 1. Verify LLVM is present (provides clang for codegen + lld-link for linking).
    match perry_codegen::linker::find_clang() {
        Some(p) => println!("  {} LLVM found: {}", style("✓").green(), p.display()),
        None => bail!(
            "LLVM not found. Install it first, then rerun:\n  \
             winget install LLVM.LLVM    (or: choco install llvm / scoop install llvm)"
        ),
    }

    // 2. Locate xwin.
    let xwin = find_xwin_exe().ok_or_else(|| anyhow::anyhow!(
        "xwin.exe not found. The Perry Windows release zip bundles it alongside \
         perry.exe. If you installed Perry from source (cargo install), install xwin \
         separately:\n  \
         cargo install xwin --locked --version 0.9.0"
    ))?;
    println!("  {} xwin found: {}", style("✓").green(), xwin.display());
    println!();

    // 3. Microsoft license acceptance. xwin's own URL (src/main.rs:269 in xwin 0.9.0).
    let license_url = "https://go.microsoft.com/fwlink/?LinkId=2086102";
    if !accept_license {
        println!("  {} Microsoft Visual Studio Build Tools License", style("⚠").yellow().bold());
        println!();
        println!("  The Microsoft CRT + Windows SDK libraries are redistributable under");
        println!("  the Microsoft Software License Terms. By proceeding you accept:");
        println!();
        println!("    {}", style(license_url).underlined().blue());
        println!();
        let accepted = Confirm::new()
            .with_prompt("  Do you accept the license?")
            .default(false)
            .interact()?;
        if !accepted {
            bail!("License not accepted — aborting.");
        }
    } else {
        println!("  {} License accepted via --accept-license", style("ℹ").cyan());
    }

    // 4. Output directory — %LOCALAPPDATA%\perry\windows-sdk (matches what
    //    find_perry_windows_sdk() in compile.rs probes at link time).
    let output_dir = dirs::data_local_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not resolve %LOCALAPPDATA%"))?
        .join("perry")
        .join("windows-sdk");

    println!();
    println!("  Output: {}", output_dir.display());
    println!("  Expect ~700 MB download / ~1.5 GB unpacked. Takes 2–4 minutes on a");
    println!("  typical connection. Partial downloads are resumable — safe to re-run.");
    println!();

    std::fs::create_dir_all(&output_dir)
        .with_context(|| format!("Failed to create {}", output_dir.display()))?;

    // 5. Run xwin. --disable-symlinks avoids noisy case-sensitivity symlinks on
    //    NTFS (xwin adds them on case-sensitive filesystems for windows.h vs
    //    Windows.h; Windows' NTFS is case-insensitive by default so they're a
    //    no-op). --accept-license since we already prompted.
    // xwin arg order: top-level flags (--accept-license, --arch) come BEFORE
    // the subcommand; splat-level flags (--output, --disable-symlinks) come AFTER.
    let start = Instant::now();
    let mut cmd = Command::new(&xwin);
    cmd.arg("--accept-license")
        .arg("--arch").arg("x86_64")
        .arg("splat")
        .arg("--disable-symlinks")
        .arg("--output").arg(&output_dir);

    let status = cmd.status().with_context(|| format!("Failed to invoke {}", xwin.display()))?;
    if !status.success() {
        bail!(
            "xwin splat failed (status {}). The partial download at {} can be retried \
             safely — re-run `perry setup windows`.",
            status,
            output_dir.display()
        );
    }

    let elapsed = start.elapsed();
    println!();
    println!("  {} Windows SDK ready at {}", style("✓").green().bold(), output_dir.display());
    println!("    ({:.1}s)", elapsed.as_secs_f64());
    println!();
    println!("  Try it:  {}", style("perry compile hello.ts && ./hello.exe").bold());
    println!();
    println!("  Run `perry doctor` to verify the full toolchain.");
    Ok(())
}

// ---------------------------------------------------------------------------
// Android wizard
// ---------------------------------------------------------------------------

pub(crate) fn android_wizard(saved: &mut PerryConfig) -> Result<()> {
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

        let path = Input::<String>::new()
            .with_prompt("  Output path (e.g. ~/release-key.keystore)")
            .interact_text()?;
        let path = expand_tilde(&path);
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
        println!("  {} Keystore created at {}", style("✓").green(), style(&path).bold());
        (path, alias)
    };

    let android = saved.android.get_or_insert_with(AndroidSavedConfig::default);
    android.keystore_path = Some(keystore_path.clone());
    android.key_alias = Some(key_alias.clone());

    println!("  {} Keystore: {}", style("✓").green(), style(&keystore_path).bold());
    println!("  {} Key alias: {}", style("✓").green(), style(&key_alias).bold());
    println!();

    // --- Step 2: Google Play Service Account ---
    println!("  {} Google Play Service Account", style("Step 2/2 —").cyan().bold());
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

    let android = saved.android.get_or_insert_with(AndroidSavedConfig::default);
    android.google_play_key_path = Some(json_path);

    // Update project perry.toml with distribute = "playstore"
    let perry_toml_path = std::env::current_dir()?.join("perry.toml");
    // Create perry.toml if it doesn't exist — project-specific config belongs here
    if !perry_toml_path.exists() {
        std::fs::write(&perry_toml_path, "")?;
    }
    let gp_key = saved.android.as_ref().and_then(|a| a.google_play_key_path.as_deref());
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
    println!("  {} {} {}",
        style("Global").bold(),
        style("→").dim(),
        style(config_path().display()).dim(),
    );
    println!("    keystore_path, key_alias, google_play_key_path");
    println!();
    println!("  {} {} {}",
        style("Project").bold(),
        style("→").dim(),
        style(perry_toml_path.display()).dim(),
    );
    println!("    keystore, key_alias, google_play_key, distribute");
    println!();
    println!("  Tip: to target a specific track, use:");
    println!("  distribute = \"playstore:beta\"  {} :internal, :alpha, :beta, :production", style("#").dim());
    println!();
    println!("  Then run: {}", style("perry publish android").bold());

    Ok(())
}

// ---------------------------------------------------------------------------
// iOS wizard
// ---------------------------------------------------------------------------

pub(crate) fn ios_wizard(saved: &mut PerryConfig) -> Result<()> {
    println!("  {}", style("iOS Setup").bold());
    println!("  Automates: app creation, certificate, bundle ID, and provisioning profile via App Store Connect API");
    println!();

    // --- Step 1: App Store Connect API Key ---
    // Check for existing credentials first
    let existing_apple = saved.apple.clone().unwrap_or_default();

    println!("  {} App Store Connect API Key", style("Step 1 —").cyan().bold());
    println!();

    let has_existing = existing_apple.p8_key_path.is_some()
        && existing_apple.key_id.is_some()
        && existing_apple.issuer_id.is_some();

    let (p8_path, key_id, issuer_id, team_id) = if has_existing {
        let p8 = existing_apple.p8_key_path.clone().unwrap();
        let kid = existing_apple.key_id.clone().unwrap();
        let iss = existing_apple.issuer_id.clone().unwrap();
        let tid = existing_apple.team_id.clone().unwrap_or_default();
        println!("  Found existing credentials:");
        println!("    Key ID:    {}", style(&kid).bold());
        println!("    Issuer ID: {}", style(&iss).dim());
        println!("    .p8 key:   {}", style(&p8).dim());
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
        println!("  1. Go to: {}", style("https://appstoreconnect.apple.com/access/integrations/api").underlined());
        println!("  2. Click '+', create a key with {} role.", style("App Manager").bold());
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
        bail!("Invalid .p8 file — expected PEM format");
    }

    // Save API credentials immediately
    let apple = saved.apple.get_or_insert_with(AppleSavedConfig::default);
    apple.p8_key_path = Some(p8_path.clone());
    apple.key_id = Some(key_id.clone());
    apple.issuer_id = Some(issuer_id.clone());
    apple.team_id = Some(team_id.clone());
    save_config(saved).ok();

    println!("  {} API credentials configured", style("✓").green().bold());
    println!();

    // Generate JWT for API calls
    let jwt = generate_asc_jwt(&key_id, &issuer_id, &p8_content)?;

    // Verify API connectivity
    print!("  Verifying API access... ");
    std::io::Write::flush(&mut std::io::stdout()).ok();
    let client = reqwest::blocking::Client::new();
    let resp = client.get("https://api.appstoreconnect.apple.com/v1/certificates?limit=1")
        .bearer_auth(&jwt)
        .send()
        .context("Failed to connect to App Store Connect API")?;
    if resp.status() == 401 || resp.status() == 403 {
        bail!("API authentication failed — check your Key ID, Issuer ID, and .p8 key file");
    }
    if !resp.status().is_success() {
        let body = resp.text().unwrap_or_default();
        bail!("API error: {body}");
    }
    println!("{}", style("ok").green());
    println!();

    // --- Step 2: Read bundle_id from perry.toml ---
    let perry_toml_path = std::env::current_dir()?.join("perry.toml");
    let bundle_id = if perry_toml_path.exists() {
        let content = std::fs::read_to_string(&perry_toml_path)?;
        let parsed: toml::Value = toml::from_str(&content)?;
        parsed.get("ios").and_then(|i| i.get("bundle_id")).and_then(|v| v.as_str())
            .or_else(|| parsed.get("app").and_then(|a| a.get("bundle_id")).and_then(|v| v.as_str()))
            .or_else(|| parsed.get("project").and_then(|p| p.get("bundle_id")).and_then(|v| v.as_str()))
            .map(|s| s.to_string())
    } else {
        None
    };

    let bundle_id = if let Some(bid) = bundle_id {
        println!("  Found bundle ID in perry.toml: {}", style(&bid).bold());
        let use_it = Confirm::new()
            .with_prompt("  Use this bundle ID?")
            .default(true)
            .interact()?;
        if use_it { bid } else {
            Input::<String>::new()
                .with_prompt("  Bundle ID (e.g. com.company.app)")
                .interact_text()?
        }
    } else {
        Input::<String>::new()
            .with_prompt("  Bundle ID (e.g. com.company.app)")
            .interact_text()?
    };
    println!();

    // --- Step 2: Register Bundle ID (App ID) if needed ---
    println!("  {} Registering App ID", style("Step 2 —").cyan().bold());
    print!("  Checking if {} exists... ", style(&bundle_id).bold());
    std::io::Write::flush(&mut std::io::stdout()).ok();

    let jwt = generate_asc_jwt(&key_id, &issuer_id, &p8_content)?;
    let resp = client.get("https://api.appstoreconnect.apple.com/v1/bundleIds")
        .bearer_auth(&jwt)
        .query(&[("filter[identifier]", &bundle_id), ("limit", &"1".to_string())])
        .send()?;
    let body: serde_json::Value = resp.json()?;
    let existing_bundle_ids = body["data"].as_array();
    let bundle_id_resource_id = if let Some(ids) = existing_bundle_ids {
        if ids.is_empty() {
            println!("{}", style("not found, creating...").yellow());
            // Register new bundle ID
            let jwt = generate_asc_jwt(&key_id, &issuer_id, &p8_content)?;
            let app_name = bundle_id.split('.').last().unwrap_or("app");
            let create_body = serde_json::json!({
                "data": {
                    "type": "bundleIds",
                    "attributes": {
                        "identifier": bundle_id,
                        "name": format!("Perry - {}", app_name),
                        "platform": "IOS"
                    }
                }
            });
            let resp = client.post("https://api.appstoreconnect.apple.com/v1/bundleIds")
                .bearer_auth(&jwt)
                .json(&create_body)
                .send()?;
            if !resp.status().is_success() {
                let err = resp.text().unwrap_or_default();
                bail!("Failed to register Bundle ID: {err}");
            }
            let resp_body: serde_json::Value = resp.json()?;
            let rid = resp_body["data"]["id"].as_str()
                .ok_or_else(|| anyhow::anyhow!("No ID in bundle registration response"))?
                .to_string();
            println!("  {} Registered: {}", style("✓").green().bold(), style(&bundle_id).bold());
            rid
        } else {
            println!("{}", style("exists").green());
            ids[0]["id"].as_str().unwrap_or("").to_string()
        }
    } else {
        bail!("Unexpected API response when checking bundle IDs");
    };
    println!();

    // --- Step 3: Create App in App Store Connect if needed ---
    println!("  {} App Store Connect App", style("Step 3 —").cyan().bold());
    print!("  Checking if app exists for {}... ", style(&bundle_id).bold());
    std::io::Write::flush(&mut std::io::stdout()).ok();

    let jwt = generate_asc_jwt(&key_id, &issuer_id, &p8_content)?;
    let resp = client.get("https://api.appstoreconnect.apple.com/v1/apps")
        .bearer_auth(&jwt)
        .query(&[("filter[bundleId]", bundle_id.as_str()), ("limit", "1")])
        .send()?;
    let body: serde_json::Value = resp.json()?;
    let existing_apps = body["data"].as_array();
    if let Some(apps) = existing_apps {
        if apps.is_empty() {
            println!("{}", style("not found, creating...").yellow());

            // Read app name from perry.toml or prompt
            let app_name = if perry_toml_path.exists() {
                let content = std::fs::read_to_string(&perry_toml_path)?;
                let parsed: toml::Value = toml::from_str(&content)?;
                parsed.get("app").and_then(|a| a.get("name")).and_then(|v| v.as_str())
                    .or_else(|| parsed.get("project").and_then(|p| p.get("name")).and_then(|v| v.as_str()))
                    .map(|s| s.to_string())
            } else {
                None
            };

            let app_name = if let Some(name) = app_name {
                println!("  App name from perry.toml: {}", style(&name).bold());
                let use_it = Confirm::new()
                    .with_prompt("  Use this name?")
                    .default(true)
                    .interact()?;
                if use_it { name } else {
                    Input::<String>::new()
                        .with_prompt("  App name (as shown on App Store)")
                        .interact_text()?
                }
            } else {
                Input::<String>::new()
                    .with_prompt("  App name (as shown on App Store)")
                    .interact_text()?
            };

            let sku = bundle_id.replace('.', "-");
            let create_body = serde_json::json!({
                "data": {
                    "type": "apps",
                    "attributes": {
                        "name": app_name,
                        "primaryLocale": "en-US",
                        "sku": sku,
                        "bundleId": bundle_id
                    },
                    "relationships": {
                        "bundleId": {
                            "data": {
                                "type": "bundleIds",
                                "id": bundle_id_resource_id
                            }
                        }
                    }
                }
            });

            let jwt = generate_asc_jwt(&key_id, &issuer_id, &p8_content)?;
            let resp = client.post("https://api.appstoreconnect.apple.com/v1/apps")
                .bearer_auth(&jwt)
                .json(&create_body)
                .send()?;
            if !resp.status().is_success() {
                let err = resp.text().unwrap_or_default();
                // Don't fail hard — app creation is optional, user can create manually
                println!("  {} Could not create app: {}", style("!").yellow(), err);
                println!("  You may need to create the app manually in App Store Connect.");
            } else {
                println!("  {} App \"{}\" created in App Store Connect", style("✓").green().bold(), style(&app_name).bold());
            }
        } else {
            let name = apps[0]["attributes"]["name"].as_str().unwrap_or("unknown");
            println!("{} ({})", style("exists").green(), style(name).bold());
        }
    } else {
        println!("{}", style("could not check").yellow());
    }
    println!();

    // --- Step 4: Create or find Distribution Certificate ---
    println!("  {} Distribution Certificate", style("Step 4 —").cyan().bold());
    print!("  Checking for existing distribution certificates... ");
    std::io::Write::flush(&mut std::io::stdout()).ok();

    let jwt = generate_asc_jwt(&key_id, &issuer_id, &p8_content)?;
    let resp = client.get("https://api.appstoreconnect.apple.com/v1/certificates")
        .bearer_auth(&jwt)
        .query(&[("filter[certificateType]", "DISTRIBUTION"), ("limit", "200")])
        .send()?;
    let body: serde_json::Value = resp.json()?;
    let certs = body["data"].as_array();

    let perry_dir = dirs::home_dir().unwrap_or_default().join(".perry");
    std::fs::create_dir_all(&perry_dir)?;
    let p12_path = perry_dir.join("distribution.p12");
    let p12_password = "perry-auto";

    // Check if we already have a valid .p12 with matching cert
    // Collect ALL valid distribution cert IDs — profile will include all of them
    let all_cert_ids: Vec<String> = if let Some(cert_list) = certs {
        let valid: Vec<String> = cert_list.iter()
            .filter(|c| c["attributes"]["certificateType"].as_str() == Some("DISTRIBUTION"))
            .filter_map(|c| c["id"].as_str().map(|s| s.to_string()))
            .collect();
        if valid.is_empty() {
            println!("{}", style("none found").yellow());
        } else {
            println!("{} found", style(format!("{}", valid.len())).green());
        }
        valid
    } else {
        println!("{}", style("error reading").red());
        vec![]
    };

    let existing_cert_id = if !all_cert_ids.is_empty() && p12_path.exists() {
        println!("  Found existing .p12 at {}", style(p12_path.display()).dim());
        let keep = Confirm::new()
            .with_prompt("  Keep existing certificate?")
            .default(true)
            .interact()?;
        if keep {
            Some(all_cert_ids[0].clone()) // placeholder — profile will use all certs
        } else {
            None
        }
    } else if !all_cert_ids.is_empty() {
        Some(all_cert_ids[0].clone())
    } else {
        None
    };

    let mut created_signing_identity: Option<String> = None;

    // If reusing existing cert, auto-detect signing identity from Keychain
    // so we can save it to perry.toml (otherwise `perry publish` will prompt for it)
    if existing_cert_id.is_some() {
        // Check if perry.toml already has a signing_identity
        let existing_identity = if perry_toml_path.exists() {
            let content = std::fs::read_to_string(&perry_toml_path).unwrap_or_default();
            let parsed: toml::Value = toml::from_str(&content).unwrap_or(toml::Value::Table(toml::Table::new()));
            parsed.get("ios")
                .and_then(|i| i.get("signing_identity"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        } else {
            None
        };
        if let Some(id) = existing_identity {
            println!("  Signing identity from perry.toml: {}", style(&id).bold());
            created_signing_identity = Some(id);
        } else {
            // Try to detect from Keychain
            let output = Command::new("security")
                .args(["find-identity", "-v", "-p", "codesigning"])
                .output();
            if let Ok(output) = output {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let mut identities: Vec<String> = Vec::new();
                for line in stdout.lines() {
                    let line = line.trim();
                    if !line.starts_with(|c: char| c.is_ascii_digit()) { continue; }
                    if let Some(quote_start) = line.find('"') {
                        if let Some(quote_end) = line.rfind('"') {
                            if quote_end > quote_start {
                                let name = line[quote_start + 1..quote_end].to_string();
                                if name.contains("Distribution") || name.contains("Developer ID") {
                                    identities.push(name);
                                }
                            }
                        }
                    }
                }
                if identities.len() == 1 {
                    println!("  Detected signing identity: {}", style(&identities[0]).bold());
                    created_signing_identity = Some(identities[0].clone());
                } else if identities.len() > 1 {
                    // Filter for "Apple Distribution" for iOS
                    let dist: Vec<&String> = identities.iter().filter(|n| n.starts_with("Apple Distribution")).collect();
                    if dist.len() == 1 {
                        println!("  Detected signing identity: {}", style(dist[0]).bold());
                        created_signing_identity = Some(dist[0].clone());
                    } else {
                        let labels: Vec<&str> = identities.iter().map(|s| s.as_str()).collect();
                        let selection = Select::new()
                            .with_prompt("  Select signing identity from Keychain")
                            .items(&labels)
                            .default(0)
                            .interact()?;
                        created_signing_identity = Some(identities[selection].clone());
                    }
                }
            }
        }
    }

    let _cert_resource_id = if let Some(id) = existing_cert_id {
        id
    } else {
        // Generate a new private key + CSR, submit to Apple, get cert back, make .p12
        println!("  Generating private key and certificate signing request...");
        let key_path = perry_dir.join("dist_private_key.pem");
        let csr_path = perry_dir.join("dist_csr.pem");

        // Generate RSA 2048 private key
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

        // Generate CSR
        let status = Command::new("openssl")
            .args(["req", "-new", "-key"])
            .arg(&key_path)
            .args(["-out"])
            .arg(&csr_path)
            .args(["-subj", "/CN=Perry Distribution/O=Perry"])
            .stderr(std::process::Stdio::null())
            .status()?;
        if !status.success() {
            bail!("Failed to generate CSR");
        }

        // Read CSR as DER (base64)
        let csr_pem = std::fs::read_to_string(&csr_path)?;
        let csr_b64: String = csr_pem.lines()
            .filter(|l| !l.starts_with("-----"))
            .collect::<Vec<_>>()
            .join("");

        // Submit CSR to Apple
        print!("  Submitting certificate request to Apple... ");
        std::io::Write::flush(&mut std::io::stdout()).ok();

        let jwt = generate_asc_jwt(&key_id, &issuer_id, &p8_content)?;
        let create_body = serde_json::json!({
            "data": {
                "type": "certificates",
                "attributes": {
                    "certificateType": "DISTRIBUTION",
                    "csrContent": csr_b64
                }
            }
        });
        let resp = client.post("https://api.appstoreconnect.apple.com/v1/certificates")
            .bearer_auth(&jwt)
            .json(&create_body)
            .send()?;
        if !resp.status().is_success() {
            let err = resp.text().unwrap_or_default();
            bail!("Failed to create certificate: {err}");
        }
        let resp_body: serde_json::Value = resp.json()?;
        let cert_content_b64 = resp_body["data"]["attributes"]["certificateContent"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No certificate content in response"))?;
        let cert_id = resp_body["data"]["id"].as_str().unwrap_or("").to_string();
        let cert_name = resp_body["data"]["attributes"]["name"].as_str().unwrap_or("Unknown");
        println!("{}", style("done").green());
        println!("  {} Certificate: {}", style("✓").green().bold(), style(cert_name).bold());

        // Decode cert and write as PEM
        use base64::Engine;
        let cert_der = base64::engine::general_purpose::STANDARD.decode(cert_content_b64)
            .context("Failed to decode certificate from Apple")?;
        let cert_pem_path = perry_dir.join("distribution.cer.pem");
        let cert_pem = format!(
            "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n",
            base64::engine::general_purpose::STANDARD.encode(&cert_der)
                .as_bytes()
                .chunks(76)
                .map(|c| std::str::from_utf8(c).unwrap_or(""))
                .collect::<Vec<_>>()
                .join("\n")
        );
        std::fs::write(&cert_pem_path, &cert_pem)?;

        // Create .p12 from private key + certificate
        print!("  Creating .p12 bundle... ");
        std::io::Write::flush(&mut std::io::stdout()).ok();

        let status = Command::new("openssl")
            .args(["pkcs12", "-export",
                   "-inkey"])
            .arg(&key_path)
            .args(["-in"])
            .arg(&cert_pem_path)
            .args(["-out"])
            .arg(&p12_path)
            .args(["-password", &format!("pass:{p12_password}"),
                   "-legacy"]) // macOS openssl compatibility
            .stderr(std::process::Stdio::null())
            .status()?;
        if !status.success() {
            // Try without -legacy flag (older openssl)
            let status = Command::new("openssl")
                .args(["pkcs12", "-export",
                       "-inkey"])
                .arg(&key_path)
                .args(["-in"])
                .arg(&cert_pem_path)
                .args(["-out"])
                .arg(&p12_path)
                .args(["-password", &format!("pass:{p12_password}")])
                .stderr(std::process::Stdio::null())
                .status()?;
            if !status.success() {
                bail!("Failed to create .p12 certificate bundle");
            }
        }
        println!("{}", style("done").green());

        // Derive signing identity from cert (will be saved to perry.toml, not global config)
        let identity = format!("Apple Distribution: {} ({})",
            cert_name.strip_prefix("Apple Distribution: ").unwrap_or(cert_name),
            &team_id);
        println!("  {} Identity: {}", style("✓").green().bold(), style(&identity).bold());
        created_signing_identity = Some(identity);

        // Clean up intermediate files (keep the private key for potential re-use)
        let _ = std::fs::remove_file(&csr_path);
        let _ = std::fs::remove_file(&cert_pem_path);

        cert_id
    };

    save_config(saved).ok();
    println!();

    // --- Step 5: Create Provisioning Profile ---
    println!("  {} Provisioning Profile", style("Step 5 —").cyan().bold());
    print!("  Creating provisioning profile for {}... ", style(&bundle_id).bold());
    std::io::Write::flush(&mut std::io::stdout()).ok();

    let jwt = generate_asc_jwt(&key_id, &issuer_id, &p8_content)?;

    // First check if one already exists
    let resp = client.get("https://api.appstoreconnect.apple.com/v1/profiles")
        .bearer_auth(&jwt)
        .query(&[("filter[profileType]", "IOS_APP_STORE"), ("include", "bundleId"), ("limit", "200")])
        .send()?;
    let body: serde_json::Value = resp.json()?;
    let existing_profile = body["data"].as_array()
        .and_then(|profiles| {
            profiles.iter().find(|p| {
                // Check if this profile's bundle ID matches ours
                let bid_id = p["relationships"]["bundleId"]["data"]["id"].as_str().unwrap_or("");
                bid_id == bundle_id_resource_id
            })
        });

    let profile_b64 = if let Some(profile) = existing_profile {
        // Delete existing profile and recreate — it may reference an old certificate
        let profile_id = profile["id"].as_str().unwrap_or("");
        if !profile_id.is_empty() {
            print!("{}, replacing... ", style("found existing").yellow());
            std::io::Write::flush(&mut std::io::stdout()).ok();
            let jwt = generate_asc_jwt(&key_id, &issuer_id, &p8_content)?;
            let _ = client.delete(format!("https://api.appstoreconnect.apple.com/v1/profiles/{profile_id}"))
                .bearer_auth(&jwt)
                .send();
        }
        // Fall through to create new profile below
        "".to_string()
    } else {
        "".to_string()
    };
    let profile_b64 = if profile_b64.is_empty() {
        // Create new profile
        let create_body = serde_json::json!({
            "data": {
                "type": "profiles",
                "attributes": {
                    "name": format!("Perry - {}", bundle_id),
                    "profileType": "IOS_APP_STORE"
                },
                "relationships": {
                    "bundleId": {
                        "data": {
                            "type": "bundleIds",
                            "id": bundle_id_resource_id
                        }
                    },
                    "certificates": {
                        "data": all_cert_ids.iter().map(|id| {
                            serde_json::json!({"type": "certificates", "id": id})
                        }).collect::<Vec<_>>()
                    }
                }
            }
        });
        let jwt = generate_asc_jwt(&key_id, &issuer_id, &p8_content)?;
        let resp = client.post("https://api.appstoreconnect.apple.com/v1/profiles")
            .bearer_auth(&jwt)
            .json(&create_body)
            .send()?;
        if !resp.status().is_success() {
            let err = resp.text().unwrap_or_default();
            bail!("Failed to create provisioning profile: {err}");
        }
        let resp_body: serde_json::Value = resp.json()?;
        println!("{}", style("created").green());
        resp_body["data"]["attributes"]["profileContent"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No profile content in response"))?
            .to_string()
    } else {
        profile_b64
    };

    // Decode and save the provisioning profile
    use base64::Engine;
    let profile_data = base64::engine::general_purpose::STANDARD.decode(&profile_b64)
        .context("Failed to decode provisioning profile")?;
    let profile_filename = format!("{}.mobileprovision", bundle_id.replace('.', "_"));
    let profile_path = perry_dir.join(profile_filename);
    std::fs::write(&profile_path, &profile_data)?;

    println!("  {} Profile saved to {}", style("✓").green().bold(), style(profile_path.display()).dim());
    println!();

    // --- Save project-specific credentials to perry.toml ---
    let p12_str = p12_path.to_string_lossy().to_string();
    let profile_str = profile_path.to_string_lossy().to_string();

    // Create perry.toml if it doesn't exist — project-specific config belongs here
    if !perry_toml_path.exists() {
        std::fs::write(&perry_toml_path, "")?;
    }
    match update_perry_toml_ios(
        &perry_toml_path,
        &p12_str,
        &profile_str,
        created_signing_identity.as_deref(),
        &bundle_id,
    ) {
        Ok(()) => {
            println!("  {} Project credentials saved to {}", style("✓").green().bold(),
                style(perry_toml_path.display()).dim());
        }
        Err(e) => {
            println!("  {} Could not update perry.toml: {e}", style("!").yellow());
            println!("  Add these manually to your perry.toml [ios] section:");
            println!("  certificate = \"{}\"", p12_str);
            println!("  provisioning_profile = \"{}\"", profile_str);
        }
    }
    // --- Export compliance ---
    println!("  {} Export Compliance", style("→").cyan().bold());
    println!("  Most apps only use HTTPS and don't need custom encryption declarations.");
    let encryption_exempt = Confirm::new()
        .with_prompt("  Does your app ONLY use standard HTTPS? (no custom encryption)")
        .default(true)
        .interact()?;
    if let Err(e) = update_perry_toml_encryption_exempt(&perry_toml_path, encryption_exempt) {
        println!("  {} Could not update perry.toml: {e}", style("!").yellow());
        println!("  Add manually to [ios]: encryption_exempt = {encryption_exempt}");
    }
    println!();

    // --- Summary ---
    println!("  {}", style("Setup complete!").green().bold());
    println!();
    println!("  {} {} {}",
        style("Global").bold(),
        style("→").dim(),
        style(config_path().display()).dim(),
    );
    println!("    p8_key_path, key_id, issuer_id, team_id");
    println!();
    println!("  {} {} {}",
        style("Project").bold(),
        style("→").dim(),
        style(perry_toml_path.display()).dim(),
    );
    println!("    bundle_id, certificate, provisioning_profile, signing_identity, encryption_exempt");
    println!();
    println!("  Certificate:  {}", style(p12_path.display()).dim());
    println!("  Profile:      {}", style(profile_path.display()).dim());
    println!("  Cert password: {}", style(p12_password).bold());
    println!();
    println!("  Set the password in your environment:");
    println!("  export PERRY_APPLE_CERTIFICATE_PASSWORD={p12_password}");
    println!();
    println!("  Then run: {}", style("perry publish ios").bold());

    Ok(())
}

/// Generate an App Store Connect API JWT token (ES256, 20-minute expiry)
fn generate_asc_jwt(key_id: &str, issuer_id: &str, p8_content: &str) -> Result<String> {
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

    let token = encode(&header, &claims, &encoding_key)
        .context("Failed to generate JWT")?;

    Ok(token)
}

/// Prompt for App Store Connect API credentials
fn prompt_api_credentials() -> Result<(String, String, String, String)> {
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

pub(crate) fn macos_wizard(saved: &mut PerryConfig) -> Result<()> {
    println!("  {}", style("macOS Setup").bold());
    println!();

    // --- Step 1: App Store Connect API Key ---
    // Check for existing credentials (shared with iOS — same Apple account)
    let existing_apple = saved.apple.clone().unwrap_or_default();

    println!("  {} App Store Connect API Key", style("Step 1/2 —").cyan().bold());
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
        println!("  1. Go to: {}", style("https://appstoreconnect.apple.com/access/integrations/api").underlined());
        println!("  2. Click '+', create a key with {} role.", style("App Manager").bold());
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
    println!("  {} Issuer ID: {}", style("✓").green(), style(&issuer_id).bold());
    if !team_id.is_empty() {
        println!("  {} Team ID: {}", style("✓").green(), style(&team_id).bold());
    }
    println!();

    // --- Step 2: Distribution method ---
    println!("  {} Distribution Method", style("Step 2/3 —").cyan().bold());
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
    let resp = client.get("https://api.appstoreconnect.apple.com/v1/certificates?limit=1")
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
            &client, &key_id, &issuer_id, &p8_content,
            "MAC_APP_DISTRIBUTION", &csr_pem, &key_path,
            &perry_dir.join("macos_appstore.p12"), p12_password,
            "Mac App Distribution",
        )?;
        cert_path = p12;
        signing_identity = identity;

        // Also create MAC_INSTALLER_DISTRIBUTION for .pkg signing
        // Stored as a separate .p12 since openssl can only export one key per .p12
        match create_apple_certificate(
            &client, &key_id, &issuer_id, &p8_content,
            "MAC_INSTALLER_DISTRIBUTION", &csr_pem, &key_path,
            &perry_dir.join("macos_installer.p12"), p12_password,
            "Mac Installer Distribution",
        ) {
            Ok((_installer_p12, _installer_identity)) => {
                installer_cert_path = Some(perry_dir.join("macos_installer.p12").to_string_lossy().to_string());
            }
            Err(e) => {
                println!("  {} Installer cert: {} (pkg signing may fail)", style("!").yellow(), e);
            }
        }
    }

    // -- Developer ID certificate (DEVELOPER_ID_APPLICATION) --
    if needs_notarize_cert {
        let (p12, identity) = create_apple_certificate(
            &client, &key_id, &issuer_id, &p8_content,
            "DEVELOPER_ID_APPLICATION", &csr_pem, &key_path,
            &perry_dir.join("macos_devid.p12"), p12_password,
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
        if signing_identity.is_empty() { None } else { Some(&signing_identity) },
        if distribute_value == "both" { Some(&notarize_cert_path) } else { None },
        if distribute_value == "both" && !notarize_signing_identity.is_empty() {
            Some(&notarize_signing_identity)
        } else {
            None
        },
        installer_cert_path.as_deref(),
    ) {
        Ok(()) => {
            println!("  {} macOS credentials saved to {}", style("✓").green().bold(),
                style(perry_toml_path.display()).dim());
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
        if let Err(e) = update_perry_toml_section_bool(&perry_toml_path, "macos", "encryption_exempt", encryption_exempt) {
            println!("  {} Could not update perry.toml: {e}", style("!").yellow());
            println!("  Add manually to [macos]: encryption_exempt = {encryption_exempt}");
        }
    }
    println!();

    // --- Summary ---
    println!("  {}", style("Setup complete!").green().bold());
    println!();
    println!("  {} {} {}",
        style("Global").bold(),
        style("→").dim(),
        style(config_path().display()).dim(),
    );
    println!("    p8_key_path, key_id, issuer_id, team_id");
    println!();
    println!("  {} {} {}",
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
    println!("  Cert password: auto-managed ({})", style("perry-auto").dim());
    println!();
    println!("  Then run: {}", style("perry publish macos").bold());

    Ok(())
}

/// Merge two .p12 files into the first one (appends the second's cert+key).
/// Both must use the same password. Uses openssl to extract PEM and repackage.
fn merge_p12_files(
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
    for (p12, pem) in [(primary_p12.as_os_str(), pem_a.as_os_str()), (std::ffi::OsStr::new(secondary_p12), pem_b.as_os_str())] {
        let ok = Command::new("openssl")
            .args(["pkcs12", "-in"]).arg(p12)
            .args(["-out"]).arg(pem)
            .args(["-nodes", "-password", &pass, "-legacy"])
            .stderr(std::process::Stdio::null())
            .status().map(|s| s.success()).unwrap_or(false);
        if !ok {
            Command::new("openssl")
                .args(["pkcs12", "-in"]).arg(p12)
                .args(["-out"]).arg(pem)
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
        .args(["pkcs12", "-export", "-in"]).arg(&combined)
        .args(["-out"]).arg(primary_p12)
        .args(["-password", &pass, "-legacy"])
        .stderr(std::process::Stdio::null())
        .status().map(|s| s.success()).unwrap_or(false);
    if !ok {
        Command::new("openssl")
            .args(["pkcs12", "-export", "-in"]).arg(&combined)
            .args(["-out"]).arg(primary_p12)
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
fn create_apple_certificate(
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
    print!("  Checking for existing {} certificate... ", style(display_name).bold());
    std::io::Write::flush(&mut std::io::stdout()).ok();

    let jwt = generate_asc_jwt(key_id, issuer_id, p8_content)?;
    let resp = client.get("https://api.appstoreconnect.apple.com/v1/certificates")
        .bearer_auth(&jwt)
        .query(&[("filter[certificateType]", cert_type), ("limit", "200")])
        .send()?;
    let body: serde_json::Value = resp.json()?;
    let existing = body["data"].as_array()
        .and_then(|arr| arr.first())
        .cloned();

    if let Some(ref cert) = existing {
        if p12_output_path.exists() {
            let name = cert["attributes"]["name"].as_str().unwrap_or(display_name);
            println!("{} ({})", style("found").green(), name);
            println!("  {} Using existing .p12 at {}", style("✓").green().bold(),
                style(p12_output_path.display()).dim());
            let identity = name.to_string();
            return Ok((p12_output_path.to_string_lossy().to_string(), identity));
        } else {
            // Existing cert was created elsewhere (e.g. Xcode) — we don't have the
            // matching private key, so we can't make a .p12 from it.
            // Create a brand-new cert with our CSR instead.
            println!("{}", style("found (no local key), creating new...").yellow());
        }
    } else {
        println!("{}", style("not found, creating...").yellow());
    }

    // Strip PEM headers for API (Apple wants raw base64)
    let csr_b64: String = csr_pem.lines()
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
    let resp = client.post("https://api.appstoreconnect.apple.com/v1/certificates")
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
            println!("  {} Developer ID certificates require Account Holder role to create via API.", style("ℹ").blue());
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
    println!("  {} Certificate: {}", style("✓").green().bold(), style(cert_name).bold());

    let identity = create_p12_from_cert_content(
        cert_content, private_key_path, p12_output_path, p12_password, display_name,
    )?;

    Ok((p12_output_path.to_string_lossy().to_string(), identity))
}

/// Fallback: export a certificate from the local macOS Keychain when the API
/// returns 403 (e.g., Developer ID certs require Account Holder role).
///
/// Lists codesigning identities, filters by display_name prefix, and uses
/// `security export` to create a .p12.
fn export_cert_from_keychain(
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
    let matching: Vec<_> = identities.iter()
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
            .with_prompt(format!("  Multiple {} certs found — select one", display_name))
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
            "export", "-k", &keychain_path,
            "-t", "identities",
            "-f", "pkcs12",
            "-P", p12_password,
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
        .args(["-nodes", "-password", &format!("pass:{p12_password}"), "-legacy"])
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

    println!("  {} Certificate: {}", style("✓").green().bold(), style(&identity_name).bold());
    println!("  {} Saved to {}", style("✓").green().bold(), style(p12_output_path.display()).dim());

    Ok((p12_output_path.to_string_lossy().to_string(), identity_name))
}

/// Convert base64-encoded DER certificate content + private key into a .p12 file.
/// Returns the signing identity string extracted from the certificate.
fn create_p12_from_cert_content(
    cert_content_b64: &str,
    private_key_path: &std::path::Path,
    p12_output_path: &std::path::Path,
    p12_password: &str,
    display_name: &str,
) -> Result<String> {
    use base64::Engine;

    let cert_der = base64::engine::general_purpose::STANDARD.decode(cert_content_b64)
        .context("Failed to decode certificate from Apple")?;

    // Write cert as PEM for openssl
    let cert_pem_path = p12_output_path.with_extension("cer.pem");
    let cert_pem = format!(
        "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n",
        base64::engine::general_purpose::STANDARD.encode(&cert_der)
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
        .split("CN=").nth(1)  // old format: subject= /CN=.../O=...
        .or_else(|| identity_output.split("CN = ").nth(1))  // new format: subject=CN = ..., O = ...
        .map(|s| s.split('/').next().unwrap_or(s))  // strip /O=...
        .map(|s| s.split(", O").next().unwrap_or(s))  // strip , O = ...
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

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Expand leading `~/` to the user's home directory.
fn expand_tilde(path: &str) -> String {
    if path.starts_with("~/") {
        dirs::home_dir()
            .map(|h| h.join(&path[2..]).to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string())
    } else {
        path.to_string()
    }
}

/// Prompt for a file path, validate it exists and has the expected extension.
fn prompt_file_path(prompt: &str, expected_ext: &str) -> Result<String> {
    let path = Input::<String>::new()
        .with_prompt(prompt)
        .interact_text()?;
    let path = expand_tilde(&path);
    if !std::path::Path::new(&path).exists() {
        bail!("File not found: {path}");
    }
    if !path.ends_with(expected_ext) {
        bail!("Expected a {expected_ext} file, got: {path}");
    }
    Ok(path)
}

/// Display a "Press Enter to continue" prompt.
fn press_enter_to_continue(prompt: &str) {
    let _ = Input::<String>::new()
        .with_prompt(prompt)
        .allow_empty(true)
        .interact_text();
}

/// Update perry.toml [ios] section with project-specific signing credentials.
fn update_perry_toml_ios(
    perry_toml_path: &std::path::Path,
    certificate: &str,
    provisioning_profile: &str,
    signing_identity: Option<&str>,
    bundle_id: &str,
) -> Result<()> {
    let content = std::fs::read_to_string(perry_toml_path)?;
    let mut doc = content.parse::<toml::Table>()
        .context("Failed to parse perry.toml")?;

    let ios = doc.entry("ios")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[ios] in perry.toml is not a table"))?;

    ios.insert("bundle_id".into(), toml::Value::String(bundle_id.into()));
    ios.insert("certificate".into(), toml::Value::String(certificate.into()));
    ios.insert("provisioning_profile".into(), toml::Value::String(provisioning_profile.into()));
    if let Some(identity) = signing_identity {
        ios.insert("signing_identity".into(), toml::Value::String(identity.into()));
    }
    if !ios.contains_key("distribute") {
        ios.insert("distribute".into(), toml::Value::String("testflight".into()));
    }

    // Ensure [project] has version and build_number — required for App Store uploads.
    // build_number is auto-incremented by `perry publish` on each upload.
    let project = doc.entry("project")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[project] in perry.toml is not a table"))?;
    if !project.contains_key("version") {
        project.insert("version".into(), toml::Value::String("1.0.0".into()));
    }
    if !project.contains_key("build_number") {
        project.insert("build_number".into(), toml::Value::Integer(0));
    }

    let new_content = toml::to_string_pretty(&doc)
        .context("Failed to serialize perry.toml")?;
    std::fs::write(perry_toml_path, new_content)?;
    Ok(())
}

/// Update perry.toml [ios] section with encryption_exempt flag.
fn update_perry_toml_encryption_exempt(
    perry_toml_path: &std::path::Path,
    encryption_exempt: bool,
) -> Result<()> {
    update_perry_toml_section_bool(perry_toml_path, "ios", "encryption_exempt", encryption_exempt)
}

/// Update a boolean field in a named section of perry.toml.
fn update_perry_toml_section_bool(
    perry_toml_path: &std::path::Path,
    section: &str,
    key: &str,
    value: bool,
) -> Result<()> {
    let content = std::fs::read_to_string(perry_toml_path)?;
    let mut doc = content.parse::<toml::Table>()
        .context("Failed to parse perry.toml")?;

    let table = doc.entry(section)
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[{section}] in perry.toml is not a table"))?;

    table.insert(key.into(), toml::Value::Boolean(value));

    let new_content = toml::to_string_pretty(&doc)
        .context("Failed to serialize perry.toml")?;
    std::fs::write(perry_toml_path, new_content)?;
    Ok(())
}

/// Update perry.toml [android] section with keystore and distribute settings.
fn update_perry_toml_android(
    perry_toml_path: &std::path::Path,
    keystore_path: &str,
    key_alias: &str,
    google_play_key: Option<&str>,
) -> Result<()> {
    let content = std::fs::read_to_string(perry_toml_path)?;
    let mut doc = content.parse::<toml::Table>()
        .context("Failed to parse perry.toml")?;

    let android = doc.entry("android")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[android] in perry.toml is not a table"))?;

    android.insert("keystore".into(), toml::Value::String(keystore_path.into()));
    android.insert("key_alias".into(), toml::Value::String(key_alias.into()));
    if let Some(key) = google_play_key {
        android.insert("google_play_key".into(), toml::Value::String(key.into()));
    }
    if !android.contains_key("distribute") {
        android.insert("distribute".into(), toml::Value::String("playstore".into()));
    }

    // Ensure [project] has version and build_number — required for Play Store uploads.
    // build_number is auto-incremented by `perry publish` on each upload.
    let project = doc.entry("project")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[project] in perry.toml is not a table"))?;
    if !project.contains_key("version") {
        project.insert("version".into(), toml::Value::String("1.0.0".into()));
    }
    if !project.contains_key("build_number") {
        project.insert("build_number".into(), toml::Value::Integer(0));
    }

    let new_content = toml::to_string_pretty(&doc)
        .context("Failed to serialize perry.toml")?;
    std::fs::write(perry_toml_path, new_content)?;
    Ok(())
}

/// Update perry.toml [macos] section with project-specific signing credentials.
fn update_perry_toml_macos(
    perry_toml_path: &std::path::Path,
    distribute: &str,
    certificate: &str,
    signing_identity: Option<&str>,
    notarize_certificate: Option<&str>,
    notarize_signing_identity: Option<&str>,
    installer_certificate: Option<&str>,
) -> Result<()> {
    let content = std::fs::read_to_string(perry_toml_path)?;
    let mut doc = content.parse::<toml::Table>()
        .context("Failed to parse perry.toml")?;

    let macos = doc.entry("macos")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[macos] in perry.toml is not a table"))?;

    macos.insert("distribute".into(), toml::Value::String(distribute.into()));
    macos.insert("certificate".into(), toml::Value::String(certificate.into()));
    if let Some(identity) = signing_identity {
        macos.insert("signing_identity".into(), toml::Value::String(identity.into()));
    }
    if let Some(notarize_cert) = notarize_certificate {
        macos.insert("notarize_certificate".into(), toml::Value::String(notarize_cert.into()));
    }
    if let Some(notarize_identity) = notarize_signing_identity {
        macos.insert("notarize_signing_identity".into(), toml::Value::String(notarize_identity.into()));
    }
    if let Some(installer_cert) = installer_certificate {
        macos.insert("installer_certificate".into(), toml::Value::String(installer_cert.into()));
    }

    let new_content = toml::to_string_pretty(&doc)
        .context("Failed to serialize perry.toml")?;
    std::fs::write(perry_toml_path, new_content)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// visionOS wizard
// ---------------------------------------------------------------------------

pub(crate) fn visionos_wizard(saved: &mut PerryConfig) -> Result<()> {
    println!("  {}", style("visionOS Setup").bold());
    println!();

    let existing_apple = saved.apple.clone().unwrap_or_default();

    println!("  {} App Store Connect API Key", style("Step 1/2 —").cyan().bold());
    println!();

    let has_existing = existing_apple.p8_key_path.is_some()
        && existing_apple.key_id.is_some()
        && existing_apple.issuer_id.is_some();

    let (p8_path, key_id, issuer_id, team_id) = if has_existing {
        let p8 = existing_apple.p8_key_path.clone().unwrap();
        let kid = existing_apple.key_id.clone().unwrap();
        let iss = existing_apple.issuer_id.clone().unwrap();
        let tid = existing_apple.team_id.clone().unwrap_or_default();
        println!("  Found existing credentials (shared with iOS/macOS):");
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
        println!("  1. Go to: {}", style("https://appstoreconnect.apple.com/access/integrations/api").underlined());
        println!("  2. Create an API key with \"App Manager\" or \"Admin\" role.");
        println!("  3. Download the .p8 file and note the Key ID and Issuer ID.");
        println!();
        prompt_api_credentials()?
    };

    saved.apple = Some(AppleSavedConfig {
        p8_key_path: Some(p8_path),
        key_id: Some(key_id),
        issuer_id: Some(issuer_id),
        team_id: if team_id.is_empty() { None } else { Some(team_id) },
        ..existing_apple
    });

    println!();
    println!("  {} Bundle ID", style("Step 2/2 —").cyan().bold());
    println!();

    let perry_toml_path = std::env::current_dir()?.join("perry.toml");
    let existing_bid = if perry_toml_path.exists() {
        let content = std::fs::read_to_string(&perry_toml_path)?;
        let parsed: toml::Table = content.parse().unwrap_or_default();
        parsed.get("visionos").and_then(|w| w.get("bundle_id")).and_then(|v| v.as_str())
            .or_else(|| parsed.get("app").and_then(|a| a.get("bundle_id")).and_then(|v| v.as_str()))
            .or_else(|| parsed.get("project").and_then(|p| p.get("bundle_id")).and_then(|v| v.as_str()))
            .or_else(|| parsed.get("ios").and_then(|p| p.get("bundle_id")).and_then(|v| v.as_str()))
            .map(|s| s.to_string())
    } else {
        None
    };

    let bundle_id: String = if let Some(ref bid) = existing_bid {
        println!("  Found existing bundle ID: {}", style(bid).bold());
        let reuse = Confirm::new()
            .with_prompt("  Use this bundle ID?")
            .default(true)
            .interact()?;
        if reuse {
            bid.clone()
        } else {
            Input::new()
                .with_prompt("  visionOS Bundle ID (e.g. com.example.myvision)")
                .interact_text()?
        }
    } else {
        Input::new()
            .with_prompt("  visionOS Bundle ID (e.g. com.example.myvision)")
            .interact_text()?
    };

    if !perry_toml_path.exists() {
        std::fs::write(&perry_toml_path, "")?;
    }
    save_visionos_bundle_id(&perry_toml_path, &bundle_id)?;

    println!();
    println!("  {} visionOS setup complete!", style("✓").green().bold());
    println!();
    println!("  Saved to:");
    println!("    Global: {}", style(config_path().display()).dim());
    println!("    Project: {}", style(perry_toml_path.display()).dim());
    println!();
    println!("  Next steps:");
    println!("    perry compile app.ts --target visionos-simulator");
    println!("    perry run visionos");
    println!();

    Ok(())
}

fn save_visionos_bundle_id(perry_toml_path: &std::path::Path, bundle_id: &str) -> Result<()> {
    let content = if perry_toml_path.exists() {
        std::fs::read_to_string(perry_toml_path)?
    } else {
        String::new()
    };
    let mut doc = content.parse::<toml::Table>()
        .unwrap_or_else(|_| toml::Table::new());

    let visionos = doc.entry("visionos")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[visionos] in perry.toml is not a table"))?;

    visionos.insert("bundle_id".into(), toml::Value::String(bundle_id.into()));

    let new_content = toml::to_string_pretty(&doc)
        .context("Failed to serialize perry.toml")?;
    std::fs::write(perry_toml_path, new_content)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// watchOS wizard
// ---------------------------------------------------------------------------

pub(crate) fn watchos_wizard(saved: &mut PerryConfig) -> Result<()> {
    println!("  {}", style("watchOS Setup").bold());
    println!();

    // --- Step 1: App Store Connect API Key ---
    // Shared with iOS — same Apple account
    let existing_apple = saved.apple.clone().unwrap_or_default();

    println!("  {} App Store Connect API Key", style("Step 1/2 —").cyan().bold());
    println!();

    let has_existing = existing_apple.p8_key_path.is_some()
        && existing_apple.key_id.is_some()
        && existing_apple.issuer_id.is_some();

    let (p8_path, key_id, issuer_id, team_id) = if has_existing {
        let p8 = existing_apple.p8_key_path.clone().unwrap();
        let kid = existing_apple.key_id.clone().unwrap();
        let iss = existing_apple.issuer_id.clone().unwrap();
        let tid = existing_apple.team_id.clone().unwrap_or_default();
        println!("  Found existing credentials (shared with iOS/macOS):");
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
        println!("  1. Go to: {}", style("https://appstoreconnect.apple.com/access/integrations/api").underlined());
        println!("  2. Create an API key with \"App Manager\" or \"Admin\" role.");
        println!("  3. Download the .p8 file and note the Key ID and Issuer ID.");
        println!();
        prompt_api_credentials()?
    };

    saved.apple = Some(AppleSavedConfig {
        p8_key_path: Some(p8_path),
        key_id: Some(key_id),
        issuer_id: Some(issuer_id),
        team_id: if team_id.is_empty() { None } else { Some(team_id) },
        ..existing_apple
    });

    // --- Step 2: Bundle ID ---
    println!();
    println!("  {} Bundle ID", style("Step 2/2 —").cyan().bold());
    println!();

    // Check perry.toml for existing bundle_id
    let perry_toml_path = std::env::current_dir()?.join("perry.toml");
    let existing_bid = if perry_toml_path.exists() {
        let content = std::fs::read_to_string(&perry_toml_path)?;
        let parsed: toml::Table = content.parse().unwrap_or_default();
        parsed.get("watchos").and_then(|w| w.get("bundle_id")).and_then(|v| v.as_str())
            .or_else(|| parsed.get("app").and_then(|a| a.get("bundle_id")).and_then(|v| v.as_str()))
            .or_else(|| parsed.get("project").and_then(|p| p.get("bundle_id")).and_then(|v| v.as_str()))
            .map(|s| s.to_string())
    } else {
        None
    };

    let bundle_id: String = if let Some(ref bid) = existing_bid {
        println!("  Found existing bundle ID: {}", style(bid).bold());
        let reuse = Confirm::new()
            .with_prompt("  Use this bundle ID?")
            .default(true)
            .interact()?;
        if reuse {
            bid.clone()
        } else {
            Input::new()
                .with_prompt("  watchOS Bundle ID (e.g. com.example.mywatch)")
                .interact_text()?
        }
    } else {
        Input::new()
            .with_prompt("  watchOS Bundle ID (e.g. com.example.mywatch)")
            .interact_text()?
    };

    // Save bundle_id to perry.toml
    if !perry_toml_path.exists() {
        std::fs::write(&perry_toml_path, "")?;
    }
    save_watchos_bundle_id(&perry_toml_path, &bundle_id)?;

    println!();
    println!("  {} watchOS setup complete!", style("✓").green().bold());
    println!();
    println!("  Saved to:");
    println!("    Global: {}", style(config_path().display()).dim());
    println!("    Project: {}", style(perry_toml_path.display()).dim());
    println!();
    println!("  Next steps:");
    println!("    perry compile app.ts --target watchos-simulator");
    println!("    perry run watchos");
    println!();

    Ok(())
}

fn save_watchos_bundle_id(perry_toml_path: &std::path::Path, bundle_id: &str) -> Result<()> {
    let content = if perry_toml_path.exists() {
        std::fs::read_to_string(perry_toml_path)?
    } else {
        String::new()
    };
    let mut doc = content.parse::<toml::Table>()
        .unwrap_or_else(|_| toml::Table::new());

    let watchos = doc.entry("watchos")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[watchos] in perry.toml is not a table"))?;

    watchos.insert("bundle_id".into(), toml::Value::String(bundle_id.into()));

    let new_content = toml::to_string_pretty(&doc)
        .context("Failed to serialize perry.toml")?;
    std::fs::write(perry_toml_path, new_content)?;
    Ok(())
}

pub(crate) fn tvos_wizard(saved: &mut PerryConfig) -> Result<()> {
    println!("  {}", style("tvOS Setup").bold());
    println!();

    // --- Step 1: App Store Connect API Key ---
    // Shared with iOS/macOS — same Apple account
    let existing_apple = saved.apple.clone().unwrap_or_default();

    println!("  {} App Store Connect API Key", style("Step 1/2 —").cyan().bold());
    println!();

    let has_existing = existing_apple.p8_key_path.is_some()
        && existing_apple.key_id.is_some()
        && existing_apple.issuer_id.is_some();

    let (p8_path, key_id, issuer_id, team_id) = if has_existing {
        let p8 = existing_apple.p8_key_path.clone().unwrap();
        let kid = existing_apple.key_id.clone().unwrap();
        let iss = existing_apple.issuer_id.clone().unwrap();
        let tid = existing_apple.team_id.clone().unwrap_or_default();
        println!("  Found existing credentials (shared with iOS/macOS):");
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
        println!("  1. Go to: {}", style("https://appstoreconnect.apple.com/access/integrations/api").underlined());
        println!("  2. Create an API key with \"App Manager\" or \"Admin\" role.");
        println!("  3. Download the .p8 file and note the Key ID and Issuer ID.");
        println!();
        prompt_api_credentials()?
    };

    saved.apple = Some(AppleSavedConfig {
        p8_key_path: Some(p8_path),
        key_id: Some(key_id),
        issuer_id: Some(issuer_id),
        team_id: if team_id.is_empty() { None } else { Some(team_id) },
        ..existing_apple
    });

    // --- Step 2: Bundle ID ---
    println!();
    println!("  {} Bundle ID", style("Step 2/2 —").cyan().bold());
    println!();

    // Check perry.toml for existing bundle_id
    let perry_toml_path = std::env::current_dir()?.join("perry.toml");
    let existing_bid = if perry_toml_path.exists() {
        let content = std::fs::read_to_string(&perry_toml_path)?;
        let parsed: toml::Table = content.parse().unwrap_or_default();
        parsed.get("tvos").and_then(|w| w.get("bundle_id")).and_then(|v| v.as_str())
            .or_else(|| parsed.get("app").and_then(|a| a.get("bundle_id")).and_then(|v| v.as_str()))
            .or_else(|| parsed.get("project").and_then(|p| p.get("bundle_id")).and_then(|v| v.as_str()))
            .map(|s| s.to_string())
    } else {
        None
    };

    let bundle_id: String = if let Some(ref bid) = existing_bid {
        println!("  Found existing bundle ID: {}", style(bid).bold());
        let reuse = Confirm::new()
            .with_prompt("  Use this bundle ID?")
            .default(true)
            .interact()?;
        if reuse {
            bid.clone()
        } else {
            Input::new()
                .with_prompt("  tvOS Bundle ID (e.g. com.example.mytv)")
                .interact_text()?
        }
    } else {
        Input::new()
            .with_prompt("  tvOS Bundle ID (e.g. com.example.mytv)")
            .interact_text()?
    };

    // Save bundle_id to perry.toml
    if !perry_toml_path.exists() {
        std::fs::write(&perry_toml_path, "")?;
    }
    save_tvos_bundle_id(&perry_toml_path, &bundle_id)?;

    println!();
    println!("  {} tvOS setup complete!", style("✓").green().bold());
    println!();
    println!("  Saved to:");
    println!("    Global: {}", style(config_path().display()).dim());
    println!("    Project: {}", style(perry_toml_path.display()).dim());
    println!();
    println!("  Next steps:");
    println!("    perry compile app.ts --target tvos-simulator");
    println!("    perry run tvos");
    println!();

    Ok(())
}

fn save_tvos_bundle_id(perry_toml_path: &std::path::Path, bundle_id: &str) -> Result<()> {
    let content = if perry_toml_path.exists() {
        std::fs::read_to_string(perry_toml_path)?
    } else {
        String::new()
    };
    let mut doc = content.parse::<toml::Table>()
        .unwrap_or_else(|_| toml::Table::new());

    let tvos = doc.entry("tvos")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[tvos] in perry.toml is not a table"))?;

    tvos.insert("bundle_id".into(), toml::Value::String(bundle_id.into()));

    let new_content = toml::to_string_pretty(&doc)
        .context("Failed to serialize perry.toml")?;
    std::fs::write(perry_toml_path, new_content)?;
    Ok(())
}

/// `perry setup harmonyos` — find DevEco Studio's auto-generated debug
/// signing materials, ask the user for the keystore password (which DevEco
/// stores AES-encrypted with a machine-bound key not externally accessible),
/// and persist everything to `~/.perry/config.toml` so subsequent
/// `perry compile --target harmonyos` invocations sign the HAP automatically.
///
/// DevEco's auto-generated debug cert lives at `~/.ohos/config/default_*.{p12,p7b,cer}`
/// and is created the first time the user clicks Run on any HarmonyOS project.
/// The `.p7b` provisioning profile is bound to a specific bundleName + the
/// emulator's UDID — Perry's compiled HAP must use the same bundleName, which
/// the wizard auto-extracts from the profile's embedded JSON.
pub(crate) fn harmonyos_wizard(saved: &mut PerryConfig) -> Result<()> {
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
    println!("  {} Locate signing materials", style("Step 1/3 —").cyan().bold());
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

    let p12_candidates: Vec<PathBuf> = entries.iter()
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
        println!("  Found {} signing materials in {}:",
            style("1").green(),
            ohos_config.display()
        );
        println!("    {}", only.file_name().unwrap_or_default().to_string_lossy());
        println!();
        only
    } else {
        println!("  Found {} signing materials in {}:",
            style(p12_candidates.len().to_string()).green(),
            ohos_config.display()
        );
        let labels: Vec<String> = p12_candidates.iter()
            .map(|p| p.file_name().unwrap_or_default().to_string_lossy().to_string())
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
    let stem = p12_path.file_stem()
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
    let bundle_name = profile_text
        .find("\"bundle-name\":\"")
        .and_then(|i| {
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
    println!("  {} p12 keystore password", style("Step 2/3 —").cyan().bold());
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
        .arg("-keystore").arg(&p12_path)
        .arg("-storetype").arg("PKCS12")
        .arg("-storepass").arg(&password)
        .output()
        .map(|out| out.status.success() && String::from_utf8_lossy(&out.stdout).contains("Your keystore"))
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
        println!("  Couldn't auto-extract bundleName from {} — please enter it",
            profile_path.file_name().unwrap_or_default().to_string_lossy());
        println!("  manually. (You can find it in DevEco's app.json5 under `app.bundleName`.)");
        println!();
        Input::<String>::new()
            .with_prompt("  Bundle name")
            .interact_text()?
    };

    // Most users have DevEco's auto-generated cert which uses the alias `debugKey`.
    // Allow override for users who imported their own keystore.
    let key_alias = existing.key_alias.clone().unwrap_or_else(|| "debugKey".to_string());

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
    println!("    {}", style("perry compile your-app.ts --target harmonyos -o /tmp/your-app.so").dim());
    println!("    {}", style("hdc install /tmp/your-app.hap").dim());
    println!();

    Ok(())
}

fn prompt_password() -> Result<String> {
    Password::new()
        .with_prompt("  p12 password")
        .with_confirmation("  Confirm password", "  Passwords don't match — try again.")
        .interact()
        .map_err(Into::into)
}
