//! iOS development re-signing (local + App Store Connect provisioning).

use super::*;

/// Re-sign an .app bundle for development device installs.
///
/// Searches for an existing dev provisioning profile, or creates one via
/// the App Store Connect API (registers device, creates App ID + profile).
/// Then re-signs with a local Apple Development identity.
pub async fn resign_for_development(
    app_dir: &Path,
    config: &super::super::publish::PerryConfig,
    device_udid: &str,
    format: OutputFormat,
) -> Result<()> {
    // Read bundle ID from Info.plist
    let bundle_id = read_bundle_id_from_app(app_dir).unwrap_or_else(|| "com.perry.app".to_string());

    // Find all development signing identities (we'll pick the right one after
    // determining the provisioning profile, since the profile must contain the
    // certificate matching the signing identity)
    let output = Command::new("security")
        .args(["find-identity", "-v", "-p", "codesigning"])
        .output()
        .context("Failed to query Keychain for signing identities")?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    let dev_identities: Vec<(String, String)> = stdout // (hash, name)
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let q1 = line.find('"')?;
            let q2 = line.rfind('"')?;
            if q2 <= q1 {
                return None;
            }
            let name = line[q1 + 1..q2].to_string();
            if !name.starts_with("Apple Development") && !name.starts_with("iPhone Developer") {
                return None;
            }
            let after_paren = line.find(") ").map(|i| i + 2).unwrap_or(0);
            let hash_end = line.find(" \"").unwrap_or(line.len());
            if hash_end <= after_paren {
                return None;
            }
            let hash = line[after_paren..hash_end].trim().to_string();
            Some((hash, name))
        })
        .collect();

    if dev_identities.is_empty() {
        bail!(
            "No Apple Development signing identity found in Keychain.\n\
             Use Xcode to set up your development signing, or use a simulator instead."
        );
    }

    // Use team ID from saved config (NOT from the identity name — the parenthesized
    // part in "Apple Development: Name (XXXXX)" is a personal cert ID, not the team ID)
    let team_id = config
        .apple
        .as_ref()
        .and_then(|a| a.team_id.clone())
        .ok_or_else(|| {
            anyhow!("No Apple team ID in ~/.perry/config.toml — run `perry setup ios` first")
        })?;

    // Pick the identity that belongs to our team by checking TeamIdentifier
    // via a test codesign. The cert ID in the name (e.g. RY57F22743) is NOT
    // the team ID — we must verify which hash produces the right TeamIdentifier.
    let identity_hash = find_identity_for_team(&dev_identities, &team_id).ok_or_else(|| {
        anyhow!(
            "No Apple Development certificate for team {team_id} found in Keychain.\n\
             Use Xcode to set up development signing for this team."
        )
    })?;
    let identity = dev_identities
        .iter()
        .find(|(h, _)| h == &identity_hash)
        .map(|(_, n)| n.clone())
        .unwrap_or_else(|| identity_hash.clone());

    if let OutputFormat::Text = format {
        println!(
            "Re-signing for development (team {}, {})...",
            style(&team_id).dim(),
            style(&identity).dim()
        );
    }

    // Step 1: Find or create a development provisioning profile
    let profile_data = if let Some(path) = find_system_dev_profile(&bundle_id, &team_id) {
        if let OutputFormat::Text = format {
            println!(
                "  Using existing dev profile: {}",
                style(path.display()).dim()
            );
        }
        std::fs::read(&path)?
    } else {
        // Create via App Store Connect API
        if let OutputFormat::Text = format {
            println!("  Creating development provisioning profile via App Store Connect...");
        }
        create_dev_profile_via_api(config, &bundle_id, &team_id, device_udid, format)
            .await
            .context(
                "Could not create development provisioning profile.\n\
                 Ensure your App Store Connect API key has the right permissions,\n\
                 or use a simulator instead: perry run ios --simulator <UDID>",
            )?
    };

    // Embed the dev profile
    std::fs::write(app_dir.join("embedded.mobileprovision"), &profile_data)?;

    // identity was already selected by team ID matching above

    // Step 2: Build entitlements
    let tmp_dir = std::env::temp_dir().join("perry_run_resign");
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(&tmp_dir)?;

    let app_identifier = format!("{team_id}.{bundle_id}");
    let entitlements = tmp_dir.join("entitlements.plist");
    std::fs::write(
        &entitlements,
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>application-identifier</key>
    <string>{app_identifier}</string>
    <key>com.apple.developer.team-identifier</key>
    <string>{team_id}</string>
    <key>get-task-allow</key>
    <true/>
    <key>keychain-access-groups</key>
    <array>
        <string>{app_identifier}</string>
    </array>
</dict>
</plist>
"#,
        ),
    )?;

    // Step 3: Remove old signature and re-sign
    let _ = std::fs::remove_dir_all(app_dir.join("_CodeSignature"));

    let status = Command::new("codesign")
        .args(["--force", "--sign", &identity_hash, "--entitlements"])
        .arg(&entitlements)
        .arg("--generate-entitlement-der")
        .arg(app_dir)
        .status()
        .context("Failed to run codesign")?;

    let _ = std::fs::remove_dir_all(&tmp_dir);

    if !status.success() {
        bail!("codesign failed — check that your development certificate is valid");
    }

    Ok(())
}

/// Find the signing identity hash that belongs to the given team ID.
/// Signs a temp file with each identity and checks the resulting TeamIdentifier.
pub fn find_identity_for_team(identities: &[(String, String)], team_id: &str) -> Option<String> {
    let tmp = std::env::temp_dir().join("perry_team_check");
    let _ = std::fs::write(&tmp, b"x");

    for (hash, _name) in identities {
        let sign = Command::new("codesign")
            .args(["--force", "--sign", hash])
            .arg(&tmp)
            .output();
        if sign.map(|o| o.status.success()).unwrap_or(false) {
            let verify = Command::new("codesign").args(["-dvv"]).arg(&tmp).output();
            if let Ok(v) = verify {
                let stderr = String::from_utf8_lossy(&v.stderr);
                if let Some(line) = stderr.lines().find(|l| l.starts_with("TeamIdentifier=")) {
                    if line.trim_start_matches("TeamIdentifier=") == team_id {
                        let _ = std::fs::remove_file(&tmp);
                        return Some(hash.clone());
                    }
                }
            }
        }
    }
    let _ = std::fs::remove_file(&tmp);
    None
}

pub fn find_system_dev_profile(bundle_id: &str, team_id: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let profile_dirs = [
        home.join("Library/MobileDevice/Provisioning Profiles"),
        home.join(".perry"),
    ];

    for dir in &profile_dirs {
        if !dir.exists() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("mobileprovision") {
                    continue;
                }
                if let Ok(output) = Command::new("security")
                    .args(["cms", "-D", "-i"])
                    .arg(&path)
                    .output()
                {
                    if output.status.success() {
                        let c = String::from_utf8_lossy(&output.stdout);
                        let is_dev = c.contains("<key>ProvisionedDevices</key>")
                            || c.contains("<key>get-task-allow</key>\n\t\t<true/>");
                        let matches = (c.contains(bundle_id)
                            || c.contains(&format!("{team_id}.*")))
                            && c.contains(team_id);
                        if is_dev && matches {
                            return Some(path);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Create a development provisioning profile via App Store Connect API.
///
/// Steps: generate JWT → register device → find/create App ID → find dev cert →
/// create profile → download profile content
pub async fn create_dev_profile_via_api(
    config: &super::super::publish::PerryConfig,
    bundle_id: &str,
    _team_id: &str,
    device_udid: &str,
    format: OutputFormat,
) -> Result<Vec<u8>> {
    let apple = config.apple.as_ref().ok_or_else(|| {
        anyhow!("No Apple credentials in ~/.perry/config.toml — run `perry setup ios` first")
    })?;

    let key_id = apple
        .key_id
        .as_deref()
        .ok_or_else(|| anyhow!("Missing apple.key_id in config"))?;
    let issuer_id = apple
        .issuer_id
        .as_deref()
        .ok_or_else(|| anyhow!("Missing apple.issuer_id in config"))?;
    let p8_path = apple
        .p8_key_path
        .as_deref()
        .ok_or_else(|| anyhow!("Missing apple.p8_key_path in config"))?;
    let p8_key = std::fs::read_to_string(p8_path)
        .with_context(|| format!("Failed to read .p8 key from {p8_path}"))?;

    // Generate JWT for App Store Connect API
    let token = generate_asc_jwt(key_id, issuer_id, &p8_key)?;

    let client = reqwest::Client::new();
    let base = "https://api.appstoreconnect.apple.com/v1";

    // 1. Register the device (ignore error if already registered)
    if let OutputFormat::Text = format {
        print!("    Registering device...");
        std::io::Write::flush(&mut std::io::stdout()).ok();
    }
    let device_name = format!(
        "Perry Dev Device {}",
        &device_udid[..8.min(device_udid.len())]
    );
    let _ = client
        .post(format!("{base}/devices"))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "data": {
                "type": "devices",
                "attributes": {
                    "name": device_name,
                    "platform": "IOS",
                    "udid": device_udid
                }
            }
        }))
        .send()
        .await;
    if let OutputFormat::Text = format {
        println!(" done");
    }

    // 2. Find or create App ID (bundleId)
    if let OutputFormat::Text = format {
        print!("    Resolving App ID...");
        std::io::Write::flush(&mut std::io::stdout()).ok();
    }
    let resp = client
        .get(format!("{base}/bundleIds"))
        .bearer_auth(&token)
        .query(&[("filter[identifier]", bundle_id)])
        .send()
        .await
        .context("Failed to query bundleIds")?;
    let body: serde_json::Value = resp.json().await?;

    let bundle_id_resource_id = if let Some(first) = body["data"].as_array().and_then(|a| a.first())
    {
        first["id"].as_str().unwrap_or("").to_string()
    } else {
        // Create App ID
        let app_name = bundle_id.split('.').last().unwrap_or("app");
        let resp = client
            .post(format!("{base}/bundleIds"))
            .bearer_auth(&token)
            .json(&serde_json::json!({
                "data": {
                    "type": "bundleIds",
                    "attributes": {
                        "identifier": bundle_id,
                        "name": format!("Perry {app_name}"),
                        "platform": "IOS"
                    }
                }
            }))
            .send()
            .await
            .context("Failed to create bundleId")?;
        let body: serde_json::Value = resp.json().await?;
        body["data"]["id"].as_str().unwrap_or("").to_string()
    };
    if bundle_id_resource_id.is_empty() {
        bail!("Could not resolve App ID for {bundle_id}");
    }
    if let OutputFormat::Text = format {
        println!(" done");
    }

    // 3. Find a development certificate
    if let OutputFormat::Text = format {
        print!("    Finding development certificate...");
        std::io::Write::flush(&mut std::io::stdout()).ok();
    }
    let resp = client
        .get(format!("{base}/certificates"))
        .bearer_auth(&token)
        .query(&[("filter[certificateType]", "IOS_DEVELOPMENT,DEVELOPMENT")])
        .send()
        .await
        .context("Failed to query certificates")?;
    let body: serde_json::Value = resp.json().await?;

    let cert_ids: Vec<String> = body["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|c| c["id"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    if cert_ids.is_empty() {
        bail!("No iOS development certificates found in your Apple Developer account");
    }
    if let OutputFormat::Text = format {
        println!(" done ({})", cert_ids.len());
    }

    // 4. Get all registered device IDs
    let resp = client
        .get(format!("{base}/devices"))
        .bearer_auth(&token)
        .query(&[("filter[platform]", "IOS"), ("limit", "200")])
        .send()
        .await
        .context("Failed to query devices")?;
    let body: serde_json::Value = resp.json().await?;
    let device_ids: Vec<String> = body["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|d| d["id"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    // 5. Create the provisioning profile
    if let OutputFormat::Text = format {
        print!("    Creating development profile...");
        std::io::Write::flush(&mut std::io::stdout()).ok();
    }

    let cert_relationships: Vec<serde_json::Value> = cert_ids
        .iter()
        .map(|id| serde_json::json!({"type": "certificates", "id": id}))
        .collect();
    let device_relationships: Vec<serde_json::Value> = device_ids
        .iter()
        .map(|id| serde_json::json!({"type": "devices", "id": id}))
        .collect();

    let profile_name = format!("Perry Dev - {bundle_id}");
    let resp = client
        .post(format!("{base}/profiles"))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "data": {
                "type": "profiles",
                "attributes": {
                    "name": profile_name,
                    "profileType": "IOS_APP_DEVELOPMENT"
                },
                "relationships": {
                    "bundleId": {
                        "data": {"type": "bundleIds", "id": bundle_id_resource_id}
                    },
                    "certificates": {
                        "data": cert_relationships
                    },
                    "devices": {
                        "data": device_relationships
                    }
                }
            }
        }))
        .send()
        .await
        .context("Failed to create provisioning profile")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("Failed to create profile (HTTP {status}): {body}");
    }

    let body: serde_json::Value = resp.json().await?;

    // The profile content is base64-encoded in attributes.profileContent
    let profile_b64 = body["data"]["attributes"]["profileContent"]
        .as_str()
        .ok_or_else(|| anyhow!("No profileContent in API response"))?;

    use base64::Engine;
    let profile_data = base64::engine::general_purpose::STANDARD
        .decode(profile_b64)
        .context("Failed to decode profile content")?;

    if let OutputFormat::Text = format {
        println!(" done");
    }

    // Save for future use
    if let Some(home) = dirs::home_dir() {
        let save_path = home.join(".perry").join(format!(
            "{}_dev.mobileprovision",
            bundle_id.replace('.', "_")
        ));
        let _ = std::fs::write(&save_path, &profile_data);
    }

    Ok(profile_data)
}

/// Generate a JWT for App Store Connect API authentication
pub fn generate_asc_jwt(key_id: &str, issuer_id: &str, p8_key: &str) -> Result<String> {
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();

    #[derive(serde::Serialize)]
    struct Claims {
        iss: String,
        iat: u64,
        exp: u64,
        aud: String,
    }

    let claims = Claims {
        iss: issuer_id.to_string(),
        iat: now,
        exp: now + 1200, // 20 minutes
        aud: "appstoreconnect-v1".to_string(),
    };

    let mut header = Header::new(Algorithm::ES256);
    header.kid = Some(key_id.to_string());
    header.typ = Some("JWT".to_string());

    let key = EncodingKey::from_ec_pem(p8_key.as_bytes()).context("Failed to parse .p8 key")?;

    encode(&header, &claims, &key).context("Failed to generate JWT")
}

/// Read CFBundleIdentifier from an .app's Info.plist
pub fn read_bundle_id_from_app(app_dir: &Path) -> Option<String> {
    let output = Command::new("/usr/libexec/PlistBuddy")
        .args(["-c", "Print :CFBundleIdentifier"])
        .arg(app_dir.join("Info.plist"))
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}
