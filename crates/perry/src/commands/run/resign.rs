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

    // Use team ID from saved config (NOT from the identity name — the parenthesized
    // part in "Apple Development: Name (XXXXX)" is a personal cert ID, not the team ID)
    let team_id = config
        .apple
        .as_ref()
        .and_then(|a| a.team_id.clone())
        .ok_or_else(|| {
            anyhow!("No Apple team ID in ~/.perry/config.toml — run `perry setup ios` first")
        })?;

    // Pick the Apple Development identity that belongs to our team. The cert ID
    // in the name (e.g. RY57F22743) is NOT the team ID, so this verifies the
    // TeamIdentifier each candidate hash produces via a test codesign.
    let (identity_hash, identity) = find_dev_identity_for_team(&team_id)?;

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
        let app_group = read_ios_app_group_from_toml();
        let push = read_ios_push_notifications_from_toml().unwrap_or(false);
        create_dev_profile_via_api(
            config,
            &bundle_id,
            &team_id,
            device_udid,
            app_group.as_deref(),
            push,
            format,
        )
        .await
        .context(
            "Could not create development provisioning profile.\n\
                 Ensure your App Store Connect API key has the right permissions,\n\
                 or use a simulator instead: perry run ios --simulator <UDID>",
        )?
    };

    // Step 2: Embed the profile and code-sign for development.
    embed_profile_and_sign(app_dir, &team_id, &bundle_id, &identity_hash, &profile_data)?;

    Ok(())
}

/// Locate the Apple Development signing identity (hash + display name) in the
/// Keychain that belongs to `team_id`. Returns distinct errors for "no dev
/// identity at all" vs. "none matching this team" so callers can guide the user.
pub fn find_dev_identity_for_team(team_id: &str) -> Result<(String, String)> {
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

    let identity_hash = find_identity_for_team(&dev_identities, team_id).ok_or_else(|| {
        anyhow!(
            "No Apple Development certificate for team {team_id} found in Keychain.\n\
             Use Xcode to set up development signing for this team."
        )
    })?;
    let name = dev_identities
        .iter()
        .find(|(h, _)| h == &identity_hash)
        .map(|(_, n)| n.clone())
        .unwrap_or_else(|| identity_hash.clone());
    Ok((identity_hash, name))
}

/// Build the entitlements plist used when code-signing a development build.
///
/// Reuses the compile-emitted `app.entitlements` (App Groups / associated-domains
/// from #1178) when present — splicing the development keys
/// (`application-identifier`, team identifier, `get-task-allow`,
/// keychain-access-groups) in before `</dict>` — so on-device entitlements
/// survive re-signing instead of being clobbered. When no `app.entitlements`
/// exists, emits a standalone development plist (the prior behaviour).
fn build_dev_entitlements_xml(app_dir: &Path, team_id: &str, bundle_id: &str) -> String {
    let app_identifier = format!("{team_id}.{bundle_id}");
    let dev_keys = format!(
        "    <key>application-identifier</key>\n    <string>{app_identifier}</string>\n    \
         <key>com.apple.developer.team-identifier</key>\n    <string>{team_id}</string>\n    \
         <key>get-task-allow</key>\n    <true/>\n    \
         <key>keychain-access-groups</key>\n    <array>\n        <string>{app_identifier}</string>\n    </array>\n"
    );

    match std::fs::read_to_string(app_dir.join("app.entitlements")) {
        Ok(existing)
            if existing.contains("</dict>") && !existing.contains("application-identifier") =>
        {
            existing.replacen("</dict>", &format!("{dev_keys}</dict>"), 1)
        }
        _ => format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n<dict>\n{dev_keys}</dict>\n</plist>\n"
        ),
    }
}

/// Embed `profile_data` as `embedded.mobileprovision` and code-sign `app_dir`
/// with the given development identity. The entitlements are produced by
/// [`build_dev_entitlements_xml`], preserving any compile-emitted App Group /
/// associated-domains entitlements.
pub fn embed_profile_and_sign(
    app_dir: &Path,
    team_id: &str,
    bundle_id: &str,
    identity_hash: &str,
    profile_data: &[u8],
) -> Result<()> {
    std::fs::write(app_dir.join("embedded.mobileprovision"), profile_data)?;

    let tmp_dir = std::env::temp_dir().join("perry_run_resign");
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(&tmp_dir)?;

    let entitlements = tmp_dir.join("entitlements.plist");
    std::fs::write(
        &entitlements,
        build_dev_entitlements_xml(app_dir, team_id, bundle_id),
    )?;

    // Remove old signature and re-sign.
    let _ = std::fs::remove_dir_all(app_dir.join("_CodeSignature"));

    let status = Command::new("codesign")
        .args(["--force", "--sign", identity_hash, "--entitlements"])
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

/// Best-effort development signing for `perry compile --target ios`.
///
/// If a development provisioning profile (e.g. from `perry setup ios
/// --development`) and a matching Apple Development identity are already
/// available locally, embed + sign the bundle and return `true`. Returns
/// `false` (without error) when the materials are missing, so a plain compile
/// on a machine that hasn't provisioned a device still produces an unsigned
/// bundle with install instructions instead of failing.
pub fn try_sign_existing_dev_profile(
    app_dir: &Path,
    config: &super::super::publish::PerryConfig,
    format: OutputFormat,
) -> Result<bool> {
    let team_id = match config.apple.as_ref().and_then(|a| a.team_id.clone()) {
        Some(t) => t,
        None => return Ok(false),
    };
    let bundle_id = read_bundle_id_from_app(app_dir).unwrap_or_else(|| "com.perry.app".to_string());
    let profile_path = match find_system_dev_profile(&bundle_id, &team_id) {
        Some(p) => p,
        None => return Ok(false),
    };
    let (identity_hash, identity) = match find_dev_identity_for_team(&team_id) {
        Ok(v) => v,
        Err(_) => return Ok(false),
    };
    let profile_data = std::fs::read(&profile_path)?;
    embed_profile_and_sign(app_dir, &team_id, &bundle_id, &identity_hash, &profile_data)?;
    if let OutputFormat::Text = format {
        println!(
            "Signed for development (team {}, {}).",
            style(&team_id).dim(),
            style(&identity).dim()
        );
    }
    Ok(true)
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
    app_group: Option<&str>,
    push_notifications: bool,
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
        let app_name = bundle_id.split('.').next_back().unwrap_or("app");
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

    // 2b. Best-effort App Groups capability enablement (#1301).
    //
    // Apple's *public* App Store Connect API can toggle the APP_GROUPS
    // capability on a bundle ID, but it cannot create the `group.*` App Group
    // identifier nor bind it to the App ID — that part has no public endpoint
    // (https://developer.apple.com/forums/thread/127917). So when an App Group
    // is declared we enable the capability and then print the one manual portal
    // step that still has to happen, instead of silently producing a profile
    // that won't validate the `application-groups` entitlement.
    if let Some(group) = app_group.filter(|g| !g.is_empty()) {
        if let OutputFormat::Text = format {
            print!("    Enabling App Groups capability...");
            std::io::Write::flush(&mut std::io::stdout()).ok();
        }
        let resp = client
            .post(format!("{base}/bundleIdCapabilities"))
            .bearer_auth(&token)
            .json(&serde_json::json!({
                "data": {
                    "type": "bundleIdCapabilities",
                    "attributes": { "capabilityType": "APP_GROUPS" },
                    "relationships": {
                        "bundleId": {
                            "data": { "type": "bundleIds", "id": bundle_id_resource_id }
                        }
                    }
                }
            }))
            .send()
            .await;
        // An already-enabled capability comes back as a 409 conflict; treat that
        // as success, and never fail profile creation over the capability toggle.
        let enabled = matches!(&resp, Ok(r) if r.status().is_success());
        if let OutputFormat::Text = format {
            println!(" {}", if enabled { "done" } else { "already enabled" });
            println!();
            println!(
                "    {} App Group {} needs a one-time manual step — the App Store",
                style("note:").yellow().bold(),
                style(group).bold()
            );
            println!("    Connect API cannot create or bind App Group identifiers. In the");
            println!("    Apple Developer portal (Certificates, Identifiers & Profiles):");
            println!("      1. Identifiers → + → App Groups → register {group}");
            println!(
                "      2. Identifiers → App ID {bundle_id} → App Groups → Edit → check {group}"
            );
            println!("    Then re-run so the profile picks up the binding.");
            println!();
        }
    }

    // 2c. Best-effort Push Notifications capability enablement (#5074).
    //
    // Unlike App Groups, PUSH_NOTIFICATIONS has no identifier to register — the
    // capability toggle on the App ID is all that's needed for the minted
    // profile to validate the `aps-environment` entitlement that
    // `inject_ios_push_entitlement` writes at compile time. An already-enabled
    // capability comes back as a 409 conflict; treat that as success and never
    // fail profile creation over the toggle.
    if push_notifications {
        if let OutputFormat::Text = format {
            print!("    Enabling Push Notifications capability...");
            std::io::Write::flush(&mut std::io::stdout()).ok();
        }
        let resp = client
            .post(format!("{base}/bundleIdCapabilities"))
            .bearer_auth(&token)
            .json(&serde_json::json!({
                "data": {
                    "type": "bundleIdCapabilities",
                    "attributes": { "capabilityType": "PUSH_NOTIFICATIONS" },
                    "relationships": {
                        "bundleId": {
                            "data": { "type": "bundleIds", "id": bundle_id_resource_id }
                        }
                    }
                }
            }))
            .send()
            .await;
        let enabled = matches!(&resp, Ok(r) if r.status().is_success());
        if let OutputFormat::Text = format {
            println!(" {}", if enabled { "done" } else { "already enabled" });
        }
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

/// Read the declared App Group from `./perry.toml`, if any.
///
/// Mirrors the `[ios] app_group` → `[app] app_group` → top-level `app_group`
/// precedence that `app_metadata.rs` uses for Apple targets. Strictly
/// best-effort — any missing file / key / parse error yields `None`, since App
/// Group enablement never gates provisioning.
pub fn read_ios_app_group_from_toml() -> Option<String> {
    let path = std::env::current_dir().ok()?.join("perry.toml");
    let content = std::fs::read_to_string(path).ok()?;
    parse_ios_app_group(&content)
}

fn parse_ios_app_group(content: &str) -> Option<String> {
    let parsed = toml::from_str::<toml::Value>(content).ok()?;
    parsed
        .get("ios")
        .and_then(|i| i.get("app_group"))
        .or_else(|| parsed.get("app").and_then(|a| a.get("app_group")))
        .or_else(|| parsed.get("app_group"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Read `[ios] push_notifications` from `./perry.toml` (#5074). Best-effort —
/// any missing file / key / parse error yields `false`, since the capability
/// toggle never gates provisioning. The matching `aps-environment` entitlement
/// is written separately by `inject_ios_push_entitlement` at compile time.
pub fn read_ios_push_notifications_from_toml() -> Option<bool> {
    let path = std::env::current_dir().ok()?.join("perry.toml");
    let content = std::fs::read_to_string(path).ok()?;
    Some(parse_ios_push_notifications(&content))
}

fn parse_ios_push_notifications(content: &str) -> bool {
    toml::from_str::<toml::Value>(content)
        .ok()
        .and_then(|parsed| {
            parsed
                .get("ios")
                .and_then(|i| i.get("push_notifications"))
                .and_then(|v| v.as_bool())
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #1301: re-signing must not drop the App Group entitlement that
    /// `perry compile --target ios` writes to `app.entitlements` (#1178).
    /// The development keys are layered on top of the existing file.
    #[test]
    fn dev_entitlements_preserve_compile_emitted_app_group() {
        let dir = std::env::temp_dir().join(format!("perry_resign_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("app.entitlements"),
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <plist version=\"1.0\">\n<dict>\n    \
             <key>com.apple.security.application-groups</key>\n    <array>\n        \
             <string>group.com.example.shared</string>\n    </array>\n</dict>\n</plist>\n",
        )
        .unwrap();

        let xml = build_dev_entitlements_xml(&dir, "ABCDE12345", "com.example.app");

        // App Group survives.
        assert!(xml.contains("com.apple.security.application-groups"));
        assert!(xml.contains("group.com.example.shared"));
        // Development keys are layered in.
        assert!(xml.contains("<key>application-identifier</key>"));
        assert!(xml.contains("ABCDE12345.com.example.app"));
        assert!(xml.contains("<key>get-task-allow</key>"));
        // Exactly one closing dict/plist (no double-wrapping).
        assert_eq!(xml.matches("</dict>").count(), 1);
        assert_eq!(xml.matches("</plist>").count(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// With no compile-emitted entitlements, a standalone development plist
    /// is produced (the prior behaviour, kept for the no-app-group case).
    #[test]
    fn dev_entitlements_standalone_without_app_entitlements() {
        let dir = std::env::temp_dir().join(format!("perry_resign_test2_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let xml = build_dev_entitlements_xml(&dir, "ABCDE12345", "com.example.app");

        assert!(xml.starts_with("<?xml"));
        assert!(xml.contains("<key>application-identifier</key>"));
        assert!(xml.contains("ABCDE12345.com.example.app"));
        assert!(!xml.contains("com.apple.security.application-groups"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #1301: App Group resolution follows the `[ios]` → `[app]` → top-level
    /// precedence so the dev-provisioning path enables the same group the
    /// compile-time entitlement injection (#1178) writes.
    #[test]
    fn parse_ios_app_group_precedence() {
        // [ios] wins over [app] and top-level.
        let toml = "app_group = \"group.top\"\n\
                    [app]\napp_group = \"group.app\"\n\
                    [ios]\napp_group = \"group.ios\"\n";
        assert_eq!(parse_ios_app_group(toml).as_deref(), Some("group.ios"));

        // [app] is the fallback when [ios] omits it.
        let toml = "[app]\napp_group = \"group.app\"\n[ios]\nbundle_id = \"com.x.y\"\n";
        assert_eq!(parse_ios_app_group(toml).as_deref(), Some("group.app"));

        // Top-level is the last resort.
        assert_eq!(
            parse_ios_app_group("app_group = \"group.top\"\n").as_deref(),
            Some("group.top")
        );

        // No app_group anywhere, empty string, and invalid toml all yield None.
        assert_eq!(parse_ios_app_group("[ios]\nbundle_id = \"a\"\n"), None);
        assert_eq!(parse_ios_app_group("[ios]\napp_group = \"\"\n"), None);
        assert_eq!(parse_ios_app_group("not = valid = toml"), None);
    }

    /// #5074: re-signing must not drop the `aps-environment` entitlement that
    /// `perry compile --target ios` writes to `app.entitlements` — otherwise
    /// `registerForRemoteNotifications` fails on the dev-signed bundle.
    #[test]
    fn dev_entitlements_preserve_compile_emitted_push() {
        let dir = std::env::temp_dir().join(format!("perry_resign_push_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("app.entitlements"),
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <plist version=\"1.0\">\n<dict>\n    \
             <key>aps-environment</key>\n    <string>development</string>\n</dict>\n</plist>\n",
        )
        .unwrap();

        let xml = build_dev_entitlements_xml(&dir, "ABCDE12345", "com.example.app");

        // Push entitlement survives, development keys are layered in.
        assert!(xml.contains("<key>aps-environment</key>"));
        assert!(xml.contains("<string>development</string>"));
        assert!(xml.contains("<key>application-identifier</key>"));
        assert!(xml.contains("ABCDE12345.com.example.app"));
        assert_eq!(xml.matches("</dict>").count(), 1);
        assert_eq!(xml.matches("</plist>").count(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #5074: `[ios] push_notifications` drives the best-effort
    /// PUSH_NOTIFICATIONS capability toggle during dev provisioning.
    #[test]
    fn parse_ios_push_notifications_opt_in() {
        assert!(parse_ios_push_notifications(
            "[ios]\npush_notifications = true\n"
        ));
        assert!(!parse_ios_push_notifications(
            "[ios]\npush_notifications = false\n"
        ));
        assert!(!parse_ios_push_notifications("[ios]\nbundle_id = \"a\"\n"));
        assert!(!parse_ios_push_notifications("not = valid = toml"));
    }
}
