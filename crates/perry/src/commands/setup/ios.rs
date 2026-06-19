use anyhow::{anyhow, bail, Context, Result};
use console::style;
use dialoguer::{Confirm, Input, Select};
use std::process::Command;

use super::super::publish::{
    config_path, is_interactive, save_config, AppleSavedConfig, PerryConfig,
};

use super::*;

pub fn ios_wizard(saved: &mut PerryConfig) -> Result<()> {
    println!("  {}", style("iOS Setup").bold());
    println!("  Automates: app creation, certificate, bundle ID, and provisioning profile via App Store Connect API");
    println!();

    // --- Step 1: App Store Connect API Key ---
    // Check for existing credentials first
    let existing_apple = saved.apple.clone().unwrap_or_default();

    println!(
        "  {} App Store Connect API Key",
        style("Step 1 —").cyan().bold()
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
    let resp = client
        .get("https://api.appstoreconnect.apple.com/v1/certificates?limit=1")
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
        parsed
            .get("ios")
            .and_then(|i| i.get("bundle_id"))
            .and_then(|v| v.as_str())
            .or_else(|| {
                parsed
                    .get("app")
                    .and_then(|a| a.get("bundle_id"))
                    .and_then(|v| v.as_str())
            })
            .or_else(|| {
                parsed
                    .get("project")
                    .and_then(|p| p.get("bundle_id"))
                    .and_then(|v| v.as_str())
            })
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
        if use_it {
            bid
        } else {
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
    let resp = client
        .get("https://api.appstoreconnect.apple.com/v1/bundleIds")
        .bearer_auth(&jwt)
        .query(&[
            ("filter[identifier]", &bundle_id),
            ("limit", &"1".to_string()),
        ])
        .send()?;
    let body: serde_json::Value = resp.json()?;
    let existing_bundle_ids = body["data"].as_array();
    let bundle_id_resource_id = if let Some(ids) = existing_bundle_ids {
        if ids.is_empty() {
            println!("{}", style("not found, creating...").yellow());
            // Register new bundle ID
            let jwt = generate_asc_jwt(&key_id, &issuer_id, &p8_content)?;
            let app_name = bundle_id.split('.').next_back().unwrap_or("app");
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
            let resp = client
                .post("https://api.appstoreconnect.apple.com/v1/bundleIds")
                .bearer_auth(&jwt)
                .json(&create_body)
                .send()?;
            if !resp.status().is_success() {
                let err = resp.text().unwrap_or_default();
                bail!("Failed to register Bundle ID: {err}");
            }
            let resp_body: serde_json::Value = resp.json()?;
            let rid = resp_body["data"]["id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("No ID in bundle registration response"))?
                .to_string();
            println!(
                "  {} Registered: {}",
                style("✓").green().bold(),
                style(&bundle_id).bold()
            );
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
    println!(
        "  {} App Store Connect App",
        style("Step 3 —").cyan().bold()
    );
    print!(
        "  Checking if app exists for {}... ",
        style(&bundle_id).bold()
    );
    std::io::Write::flush(&mut std::io::stdout()).ok();

    let jwt = generate_asc_jwt(&key_id, &issuer_id, &p8_content)?;
    let resp = client
        .get("https://api.appstoreconnect.apple.com/v1/apps")
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
                parsed
                    .get("app")
                    .and_then(|a| a.get("name"))
                    .and_then(|v| v.as_str())
                    .or_else(|| {
                        parsed
                            .get("project")
                            .and_then(|p| p.get("name"))
                            .and_then(|v| v.as_str())
                    })
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
                if use_it {
                    name
                } else {
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
            let resp = client
                .post("https://api.appstoreconnect.apple.com/v1/apps")
                .bearer_auth(&jwt)
                .json(&create_body)
                .send()?;
            if !resp.status().is_success() {
                let err = resp.text().unwrap_or_default();
                // Don't fail hard — app creation is optional, user can create manually
                println!("  {} Could not create app: {}", style("!").yellow(), err);
                println!("  You may need to create the app manually in App Store Connect.");
            } else {
                println!(
                    "  {} App \"{}\" created in App Store Connect",
                    style("✓").green().bold(),
                    style(&app_name).bold()
                );
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
    println!(
        "  {} Distribution Certificate",
        style("Step 4 —").cyan().bold()
    );
    print!("  Checking for existing distribution certificates... ");
    std::io::Write::flush(&mut std::io::stdout()).ok();

    let jwt = generate_asc_jwt(&key_id, &issuer_id, &p8_content)?;
    let resp = client
        .get("https://api.appstoreconnect.apple.com/v1/certificates")
        .bearer_auth(&jwt)
        .query(&[
            ("filter[certificateType]", "DISTRIBUTION"),
            ("limit", "200"),
        ])
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
        let valid: Vec<String> = cert_list
            .iter()
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
        println!(
            "  Found existing .p12 at {}",
            style(p12_path.display()).dim()
        );
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
            let parsed: toml::Value =
                toml::from_str(&content).unwrap_or(toml::Value::Table(toml::Table::new()));
            parsed
                .get("ios")
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
                    if !line.starts_with(|c: char| c.is_ascii_digit()) {
                        continue;
                    }
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
                    println!(
                        "  Detected signing identity: {}",
                        style(&identities[0]).bold()
                    );
                    created_signing_identity = Some(identities[0].clone());
                } else if identities.len() > 1 {
                    // Filter for "Apple Distribution" for iOS
                    let dist: Vec<&String> = identities
                        .iter()
                        .filter(|n| n.starts_with("Apple Distribution"))
                        .collect();
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
        let csr_b64: String = csr_pem
            .lines()
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
        let resp = client
            .post("https://api.appstoreconnect.apple.com/v1/certificates")
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
        let cert_name = resp_body["data"]["attributes"]["name"]
            .as_str()
            .unwrap_or("Unknown");
        println!("{}", style("done").green());
        println!(
            "  {} Certificate: {}",
            style("✓").green().bold(),
            style(cert_name).bold()
        );

        // Decode cert and write as PEM
        use base64::Engine;
        let cert_der = base64::engine::general_purpose::STANDARD
            .decode(cert_content_b64)
            .context("Failed to decode certificate from Apple")?;
        let cert_pem_path = perry_dir.join("distribution.cer.pem");
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

        // Create .p12 from private key + certificate
        print!("  Creating .p12 bundle... ");
        std::io::Write::flush(&mut std::io::stdout()).ok();

        let status = Command::new("openssl")
            .args(["pkcs12", "-export", "-inkey"])
            .arg(&key_path)
            .args(["-in"])
            .arg(&cert_pem_path)
            .args(["-out"])
            .arg(&p12_path)
            .args(["-password", &format!("pass:{p12_password}"), "-legacy"]) // macOS openssl compatibility
            .stderr(std::process::Stdio::null())
            .status()?;
        if !status.success() {
            // Try without -legacy flag (older openssl)
            let status = Command::new("openssl")
                .args(["pkcs12", "-export", "-inkey"])
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
        let identity = format!(
            "Apple Distribution: {} ({})",
            cert_name
                .strip_prefix("Apple Distribution: ")
                .unwrap_or(cert_name),
            &team_id
        );
        println!(
            "  {} Identity: {}",
            style("✓").green().bold(),
            style(&identity).bold()
        );
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
    print!(
        "  Creating provisioning profile for {}... ",
        style(&bundle_id).bold()
    );
    std::io::Write::flush(&mut std::io::stdout()).ok();

    let jwt = generate_asc_jwt(&key_id, &issuer_id, &p8_content)?;

    // First check if one already exists
    let resp = client
        .get("https://api.appstoreconnect.apple.com/v1/profiles")
        .bearer_auth(&jwt)
        .query(&[
            ("filter[profileType]", "IOS_APP_STORE"),
            ("include", "bundleId"),
            ("limit", "200"),
        ])
        .send()?;
    let body: serde_json::Value = resp.json()?;
    let existing_profile = body["data"].as_array().and_then(|profiles| {
        profiles.iter().find(|p| {
            // Check if this profile's bundle ID matches ours
            let bid_id = p["relationships"]["bundleId"]["data"]["id"]
                .as_str()
                .unwrap_or("");
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
            let _ = client
                .delete(format!(
                    "https://api.appstoreconnect.apple.com/v1/profiles/{profile_id}"
                ))
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
        let resp = client
            .post("https://api.appstoreconnect.apple.com/v1/profiles")
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
    let profile_data = base64::engine::general_purpose::STANDARD
        .decode(&profile_b64)
        .context("Failed to decode provisioning profile")?;
    let profile_filename = format!("{}.mobileprovision", bundle_id.replace('.', "_"));
    let profile_path = perry_dir.join(profile_filename);
    std::fs::write(&profile_path, &profile_data)?;

    println!(
        "  {} Profile saved to {}",
        style("✓").green().bold(),
        style(profile_path.display()).dim()
    );
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
            println!(
                "  {} Project credentials saved to {}",
                style("✓").green().bold(),
                style(perry_toml_path.display()).dim()
            );
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
    println!(
        "    bundle_id, certificate, provisioning_profile, signing_identity, encryption_exempt"
    );
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

/// `perry setup ios --development` — provision the connected device for
/// on-device development (issue #1301).
///
/// Reuses the App Store Connect credentials stored by a prior `perry setup ios`
/// to register the connected device's UDID and mint an "iOS App Development"
/// provisioning profile for the project's bundle ID, saved next to the
/// distribution profile at `~/.perry/<bundle>_dev.mobileprovision`. The existing
/// `perry run --target ios` / `perry compile --target ios` paths auto-discover
/// that profile and dev-sign the bundle, so no perry.toml distribution fields
/// are clobbered here.
pub fn ios_development_setup(saved: &PerryConfig) -> Result<()> {
    println!("  {}", style("iOS Development Provisioning").bold());
    println!("  Registers your connected device and creates an iOS App Development profile");
    println!("  via App Store Connect (reuses the credentials from `perry setup ios`).");
    println!();

    // --- Require stored App Store Connect credentials ---
    let apple = saved.apple.as_ref().ok_or_else(|| {
        anyhow!(
            "No Apple credentials found in ~/.perry/config.toml.\n\
             Run `perry setup ios` first to store your App Store Connect API key."
        )
    })?;
    let team_id = apple
        .team_id
        .clone()
        .ok_or_else(|| anyhow!("Missing apple.team_id — run `perry setup ios` first."))?;
    if apple.key_id.is_none() || apple.issuer_id.is_none() || apple.p8_key_path.is_none() {
        bail!(
            "Incomplete App Store Connect credentials in ~/.perry/config.toml.\n\
             Run `perry setup ios` to (re)configure key_id, issuer_id, and the .p8 key."
        );
    }

    // --- Resolve the project bundle ID from perry.toml ---
    let perry_toml_path = std::env::current_dir()?.join("perry.toml");
    let toml_bundle_id = if perry_toml_path.exists() {
        let content = std::fs::read_to_string(&perry_toml_path)?;
        toml::from_str::<toml::Value>(&content)
            .ok()
            .and_then(|parsed| {
                parsed
                    .get("ios")
                    .and_then(|i| i.get("bundle_id"))
                    .or_else(|| parsed.get("app").and_then(|a| a.get("bundle_id")))
                    .or_else(|| parsed.get("project").and_then(|p| p.get("bundle_id")))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
    } else {
        None
    };

    let bundle_id = match toml_bundle_id {
        Some(bid) => {
            println!("  Bundle ID: {}", style(&bid).bold());
            bid
        }
        None if is_interactive() => Input::<String>::new()
            .with_prompt("  Bundle ID (e.g. com.company.app)")
            .interact_text()?,
        None => bail!(
            "No bundle ID found in perry.toml ([ios]/[app]/[project].bundle_id).\n\
             Add one or run this command interactively."
        ),
    };
    println!();

    // --- Detect / pick the connected device ---
    let devices = crate::commands::run::detect_ios_devices()?;
    if devices.is_empty() {
        bail!(
            "No connected iOS device found.\n\
             Connect your iPhone/iPad via USB (and trust this computer), then retry.\n\
             Verify with: xcrun devicectl list devices"
        );
    }
    let udid = if devices.len() == 1 {
        println!(
            "  Device: {} ({})",
            style(&devices[0].name).bold(),
            style(&devices[0].udid).dim()
        );
        devices[0].udid.clone()
    } else {
        crate::commands::run::pick_device(&devices, "device to provision")?
    };
    println!();

    // --- Register the device + mint the development profile via the ASC API ---
    // A declared `[ios] app_group` is enabled (best-effort) on the bundle ID and
    // the remaining manual portal step is surfaced inside the API call (#1301).
    let app_group = crate::commands::run::read_ios_app_group_from_toml();
    let push = crate::commands::run::read_ios_push_notifications_from_toml().unwrap_or(false);
    let rt = tokio::runtime::Runtime::new()?;
    let profile_data = rt.block_on(crate::commands::run::create_dev_profile_via_api(
        saved,
        &bundle_id,
        &team_id,
        &udid,
        app_group.as_deref(),
        push,
        crate::OutputFormat::Text,
    ))?;

    let save_path = dirs::home_dir().map(|h| {
        h.join(".perry").join(format!(
            "{}_dev.mobileprovision",
            bundle_id.replace('.', "_")
        ))
    });

    println!();
    println!(
        "  {} Development profile ready ({} bytes).",
        style("✓").green().bold(),
        profile_data.len()
    );
    if let Some(p) = &save_path {
        println!("  Saved to {}", style(p.display()).dim());
    }
    println!();
    println!("  Build, sign, install, and launch on your device with:");
    println!("    {}", style("perry run --target ios").bold());

    Ok(())
}
