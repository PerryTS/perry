use super::*;

/// Prompt for target platform selection.
pub(super) fn prompt_target(default: Option<&str>) -> String {
    let options = &["macOS", "iOS", "visionOS", "tvOS", "Android", "Linux"];
    let default_idx = match default {
        Some("ios") => 1,
        Some("visionos") => 2,
        Some("tvos") => 3,
        Some("android") => 4,
        Some("linux") => 5,
        _ => 0,
    };
    let selection = Select::new()
        .with_prompt("Target platform")
        .items(options)
        .default(default_idx)
        .interact()
        .unwrap_or(0);
    match selection {
        1 => "ios".into(),
        2 => "visionos".into(),
        3 => "tvos".into(),
        4 => "android".into(),
        5 => "linux".into(),
        _ => "macos".into(),
    }
}

/// Resolve a credential value using priority: CLI flag → env var → saved config → interactive prompt.
/// Returns None only if the field is optional and the user skips it.
pub(super) fn resolve_credential(
    cli_value: Option<&str>,
    env_var: &str,
    saved_value: Option<&str>,
    prompt_label: &str,
    required: bool,
    interactive: bool,
) -> Option<String> {
    // 1. CLI flag
    if let Some(v) = cli_value {
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    // 2. Environment variable
    if let Ok(v) = std::env::var(env_var) {
        if !v.is_empty() {
            return Some(v);
        }
    }
    // 3. Saved config
    if let Some(v) = saved_value {
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    // 4. Interactive prompt
    if interactive {
        let val = prompt_input(prompt_label, saved_value);
        if val.is_some() {
            return val;
        }
        if required {
            // Re-prompt once if required
            return prompt_input(&format!("{prompt_label} (required)"), None);
        }
    }
    None
}

/// Auto-detect a signing identity from the macOS Keychain and export it as .p12.
/// Returns (base64_p12_data, password) or None if not on macOS, no identity found, or user declines.
pub(super) fn auto_export_p12_from_keychain(
    configured_identity: Option<&str>,
    interactive: bool,
) -> Option<(String, String)> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    if !interactive {
        return None;
    }

    // List available codesigning identities
    let output = std::process::Command::new("security")
        .args(["find-identity", "-v", "-p", "codesigning"])
        .output()
        .ok()?;
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

    if identities.is_empty() {
        return None;
    }

    // Match against configured identity or let user pick
    let selected = if let Some(configured) = configured_identity {
        identities
            .iter()
            .find(|(_, name)| name == configured)
            .or_else(|| {
                identities
                    .iter()
                    .find(|(_, name)| name.contains(configured))
            })
            .cloned()
    } else {
        None
    };

    let selected = if let Some(s) = selected {
        s
    } else {
        let labels: Vec<&str> = identities.iter().map(|(_, n)| n.as_str()).collect();
        let selection = Select::new()
            .with_prompt("  Select signing identity from Keychain")
            .items(&labels)
            .default(0)
            .interact()
            .ok()?;
        identities[selection].clone()
    };

    println!();
    println!("  Found identity: {}", style(&selected.1).bold());
    let consent = Confirm::new()
        .with_prompt("  Export this certificate from Keychain? (macOS will ask for access)")
        .default(true)
        .interact()
        .unwrap_or(false);
    if !consent {
        return None;
    }

    // Generate a random password for the temp .p12 using system time as entropy
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let password: String = (0..24u64)
        .map(|i| {
            let v = ((seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(i as u128 * 1442695040888963407))
                >> 16) as u8
                % 62;
            match v {
                0..=9 => (b'0' + v) as char,
                10..=35 => (b'a' + v - 10) as char,
                _ => (b'A' + v - 36) as char,
            }
        })
        .collect();

    // Export to temp .p12
    let temp_path = std::env::temp_dir().join("perry-cert-export.p12");
    let export_result = std::process::Command::new("security")
        .args([
            "export",
            "-k",
            &format!(
                "{}/Library/Keychains/login.keychain-db",
                std::env::var("HOME").unwrap_or_default()
            ),
            "-t",
            "identities",
            "-f",
            "pkcs12",
            "-P",
            &password,
            "-o",
            &temp_path.to_string_lossy(),
        ])
        .output();

    match export_result {
        Ok(out) if out.status.success() => {}
        _ => {
            println!("  {} Could not export from Keychain.", style("!").yellow());
            return None;
        }
    }

    // Read, base64-encode, clean up
    let data = std::fs::read(&temp_path).ok()?;
    let _ = std::fs::remove_file(&temp_path);

    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&data);

    println!("  {} Certificate exported successfully", style("✓").green());
    Some((b64, password))
}

/// Resolve a file path credential: CLI → env → saved config → interactive prompt.
/// Returns the path string (not validated here).
pub(super) fn resolve_path_credential(
    cli_value: Option<&Path>,
    env_var: &str,
    saved_value: Option<&str>,
    prompt_label: &str,
    interactive: bool,
) -> Option<String> {
    if let Some(v) = cli_value {
        return Some(v.to_string_lossy().to_string());
    }
    if let Ok(v) = std::env::var(env_var) {
        if !v.is_empty() {
            return Some(v);
        }
    }
    if let Some(v) = saved_value {
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    if interactive {
        return prompt_input(prompt_label, saved_value);
    }
    None
}

/// Validate that required distribution credentials are present before starting a build.
///
/// Called immediately after credential resolution, before the tarball is created,
/// so users get a clear error message without waiting for a full build to complete.
pub(super) fn validate_credentials_for_distribute(
    is_android: bool,
    android_distribute: Option<&str>,
    google_play_json: Option<&str>,
    is_ios: bool,
    ios_distribute: Option<&str>,
    apple_key_id: Option<&str>,
    apple_issuer_id: Option<&str>,
    p8_key_content: Option<&str>,
    is_macos: bool,
    macos_distribute: Option<&str>,
) -> Result<()> {
    // Android + playstore
    if is_android {
        let distribute = android_distribute.unwrap_or("");
        if distribute == "playstore" || distribute.starts_with("playstore:") {
            if google_play_json.is_none() {
                bail!(
                    "android.distribute = \"playstore\" requires a Google Play service account JSON key.\n\
                     Run `perry setup android`, pass --google-play-key <path>, or set PERRY_GOOGLE_PLAY_KEY_PATH.\n\
                     To build without uploading, remove distribute = \"playstore\" from perry.toml."
                );
            }
            // Validate track if explicitly specified
            if let Some(track) = distribute.strip_prefix("playstore:") {
                if !matches!(track, "internal" | "alpha" | "beta" | "production") {
                    bail!(
                        "Invalid Play Store track \"{track}\". Valid values: internal, alpha, beta, production.\n\
                         Example: distribute = \"playstore:beta\""
                    );
                }
            }
        }
    }

    // iOS + appstore/testflight
    if is_ios {
        let distribute = ios_distribute.unwrap_or("");
        if distribute == "appstore" || distribute == "testflight" {
            let mut missing = Vec::new();
            if apple_key_id.is_none() {
                missing.push("Key ID (--apple-key-id / PERRY_APPLE_KEY_ID)");
            }
            if apple_issuer_id.is_none() {
                missing.push("Issuer ID (--apple-issuer-id / PERRY_APPLE_ISSUER_ID)");
            }
            if p8_key_content.is_none() {
                missing.push(".p8 key (--apple-p8-key / PERRY_APPLE_P8_KEY)");
            }
            if !missing.is_empty() {
                bail!(
                    "ios.distribute = \"{distribute}\" requires App Store Connect API credentials.\n\
                     Missing: {}\n\
                     Run `perry setup ios` or pass the missing flags.",
                    missing.join(", ")
                );
            }
        }
    }

    // macOS + appstore/notarize/both
    if is_macos {
        let distribute = macos_distribute.unwrap_or("");
        if matches!(distribute, "appstore" | "testflight" | "notarize" | "both") {
            let mut missing = Vec::new();
            if apple_key_id.is_none() {
                missing.push("Key ID (--apple-key-id / PERRY_APPLE_KEY_ID)");
            }
            if apple_issuer_id.is_none() {
                missing.push("Issuer ID (--apple-issuer-id / PERRY_APPLE_ISSUER_ID)");
            }
            if p8_key_content.is_none() {
                missing.push(".p8 key (--apple-p8-key / PERRY_APPLE_P8_KEY)");
            }
            if !missing.is_empty() {
                let purpose = match distribute {
                    "notarize" => "notarization",
                    "both" => "App Store upload and notarization",
                    "appstore" | "testflight" => "App Store Connect upload",
                    _ => "distribution",
                };
                bail!(
                    "macos.distribute = \"{distribute}\" requires App Store Connect API credentials for {purpose}.\n\
                     Missing: {}\n\
                     Run `perry setup macos` or pass the missing flags.",
                    missing.join(", ")
                );
            }
        }
    }

    Ok(())
}
