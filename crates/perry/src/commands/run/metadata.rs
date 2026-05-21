//! Project-metadata + iOS signing-credential helpers for `perry run`.

use super::*;

/// Find project root by walking up from a directory
pub fn find_project_root(start: &Path) -> PathBuf {
    let mut dir = start.to_path_buf();
    for _ in 0..10 {
        if dir.join("package.json").exists() || dir.join("perry.toml").exists() {
            return dir;
        }
        if let Some(parent) = dir.parent() {
            dir = parent.to_path_buf();
        } else {
            break;
        }
    }
    start.to_path_buf()
}

/// Read app name and bundle ID from package.json
pub fn read_app_metadata(project_root: &Path, input: &Path) -> (String, String) {
    // Check perry.toml first (has [ios].bundle_id, [project].name)
    let toml_path = project_root.join("perry.toml");
    let toml_config = std::fs::read_to_string(&toml_path)
        .ok()
        .and_then(|s| toml::from_str::<toml::Value>(&s).ok());

    let toml_name = toml_config
        .as_ref()
        .and_then(|t| t.get("project"))
        .and_then(|p| p.get("name"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let toml_bundle_id = toml_config
        .as_ref()
        .and_then(|t| {
            // Check [visionos].bundle_id, [ios].bundle_id, [macos].bundle_id, [app].bundle_id, [project].bundle_id, then top-level
            t.get("visionos")
                .and_then(|i| i.get("bundle_id"))
                .or_else(|| t.get("ios").and_then(|i| i.get("bundle_id")))
                .or_else(|| t.get("macos").and_then(|m| m.get("bundle_id")))
                .or_else(|| t.get("app").and_then(|a| a.get("bundle_id")))
                .or_else(|| t.get("project").and_then(|p| p.get("bundle_id")))
                .or_else(|| t.get("bundle_id"))
        })
        .and_then(|v| v.as_str())
        .map(|s| {
            // #999: this string flows into codesign argv when `perry run`
            // re-signs the iOS bundle. Validate before letting it through.
            let label = format!("perry.toml bundle_id at {}", toml_path.display());
            super::super::sanitize::validate_bundle_id_or_exit(s, &label)
        });

    // Then check package.json
    let pkg_path = project_root.join("package.json");
    let pkg = std::fs::read_to_string(&pkg_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());

    let pkg_name = pkg
        .as_ref()
        .and_then(|p| p.get("name"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let pkg_bundle_id = pkg
        .as_ref()
        .and_then(|p| {
            p.get("bundleId")
                .or_else(|| p.get("perry").and_then(|pp| pp.get("bundleId")))
        })
        .and_then(|v| v.as_str())
        .map(|s| {
            // #999: validate before letting an explicit package.json
            // bundle ID reach codesign argv.
            let label = format!("package.json `bundleId` at {}", pkg_path.display());
            super::super::sanitize::validate_bundle_id_or_exit(s, &label)
        });

    let raw_name = toml_name.or(pkg_name).unwrap_or_else(|| {
        input
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("app")
            .to_string()
    });
    // Issue #500 (generalises #467): the package-derived name flows
    // into argv as `-o <name>` to the linker and as a bundle-ID stem
    // into codesign. Route through the shared sanitizer so every
    // hostile-character class (response-file `@`, flag-like `-`,
    // shell metacharacters, control bytes, path traversal, bidi
    // marks, …) is scrubbed at a single, fuzz-tested choke point.
    let name = super::super::sanitize::sanitize_for_linker_argv(&raw_name);

    let bundle_id = toml_bundle_id
        .or(pkg_bundle_id)
        .unwrap_or_else(|| format!("com.perry.{}", name));

    (name, bundle_id)
}

/// Read iOS-specific config from perry.toml
pub fn read_perry_toml_ios(project_root: &Path) -> Option<toml::Value> {
    let toml_path = project_root.join("perry.toml");
    let content = std::fs::read_to_string(&toml_path).ok()?;
    let config: toml::Value = content.parse().ok()?;
    config.get("ios").cloned()
}

/// Build signing credentials for physical iOS device builds.
/// Priority: perry.toml [ios] → ~/.perry/config.toml → Keychain auto-detect
pub fn build_device_credentials(
    config: &super::super::publish::PerryConfig,
    bundle_id: &str,
    ios_toml: Option<&toml::Value>,
) -> Result<serde_json::Value> {
    use base64::Engine;

    let apple = config.apple.as_ref();

    // Signing identity: perry.toml [ios].signing_identity → Keychain auto-detect
    let signing_identity = ios_toml
        .and_then(|t| t.get("signing_identity"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(detect_signing_identity);

    // Team ID from global config
    let team_id = apple.and_then(|a| a.team_id.clone());
    let key_id = apple.and_then(|a| a.key_id.clone());
    let issuer_id = apple.and_then(|a| a.issuer_id.clone());

    // .p8 key from global config
    let p8_key = apple
        .and_then(|a| a.p8_key_path.as_ref())
        .and_then(|p| std::fs::read_to_string(p).ok());

    // Certificate: perry.toml [ios].certificate path → Keychain auto-export
    let (cert_b64, cert_password) = {
        let toml_cert_path = ios_toml
            .and_then(|t| t.get("certificate"))
            .and_then(|v| v.as_str());

        if let Some(cert_path) = toml_cert_path {
            let path = Path::new(cert_path);
            if path.exists() {
                let data = std::fs::read(path).ok();
                let b64 = data.map(|d| base64::engine::general_purpose::STANDARD.encode(&d));
                // Password: check env, then use "perry-auto" for ~/.perry/ certs
                let password = std::env::var("PERRY_APPLE_CERTIFICATE_PASSWORD")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .or_else(|| {
                        if cert_path.contains("/.perry/") {
                            Some("perry-auto".to_string())
                        } else {
                            None
                        }
                    });
                (b64, password)
            } else {
                auto_export_p12(signing_identity.as_deref())
            }
        } else {
            auto_export_p12(signing_identity.as_deref())
        }
    };

    // Provisioning profile: for dev builds, don't send the distribution profile —
    // the hub should generate/find a development profile when ios_distribute = "development".
    // Only check for profiles that are explicitly development profiles.
    let profile_b64 = find_development_provisioning_profile(bundle_id);

    if signing_identity.is_none() {
        bail!(
            "No code signing identity found for device builds.\n\
             Run `perry setup ios` first, or use `perry run ios --simulator <UDID>` for unsigned builds."
        );
    }

    Ok(serde_json::json!({
        "apple_team_id": team_id,
        "apple_signing_identity": signing_identity,
        "apple_key_id": key_id,
        "apple_issuer_id": issuer_id,
        "apple_p8_key": p8_key,
        "provisioning_profile_base64": profile_b64,
        "apple_certificate_p12_base64": cert_b64,
        "apple_certificate_password": cert_password,
    }))
}

/// Detect first available Apple Distribution / Developer signing identity
pub fn detect_signing_identity() -> Option<String> {
    let output = Command::new("security")
        .args(["find-identity", "-v", "-p", "codesigning"])
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Prefer "Apple Distribution" for device, then "iPhone Distribution", then first available
    let mut identities: Vec<String> = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if let Some(q1) = line.find('"') {
            if let Some(q2) = line.rfind('"') {
                if q2 > q1 {
                    identities.push(line[q1 + 1..q2].to_string());
                }
            }
        }
    }

    identities
        .iter()
        .find(|n| n.starts_with("Apple Distribution"))
        .or_else(|| {
            identities
                .iter()
                .find(|n| n.starts_with("iPhone Distribution"))
        })
        .or_else(|| identities.first())
        .cloned()
}

/// Auto-export a .p12 from Keychain for the given identity
pub fn auto_export_p12(identity: Option<&str>) -> (Option<String>, Option<String>) {
    let identity = match identity {
        Some(id) => id,
        None => return (None, None),
    };
    use base64::Engine;

    let password = "perry-run-auto";
    let tmp_path = std::env::temp_dir().join("perry_run_auto.p12");

    // Find the identity hash
    let output = match Command::new("security")
        .args(["find-identity", "-v", "-p", "codesigning"])
        .output()
    {
        Ok(o) => o,
        Err(_) => return (None, None),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut hash = None;
    for line in stdout.lines() {
        if line.contains(identity) {
            let trimmed = line.trim();
            let after_paren = trimmed.find(") ").map(|i| i + 2).unwrap_or(0);
            let hash_end = trimmed.find(" \"").unwrap_or(trimmed.len());
            if hash_end > after_paren {
                hash = Some(trimmed[after_paren..hash_end].trim().to_string());
                break;
            }
        }
    }

    let _hash = match hash {
        Some(h) => h,
        None => return (None, None),
    };

    // Export .p12
    let status = Command::new("security")
        .args([
            "export",
            "-k",
            "login.keychain-db",
            "-t",
            "identities",
            "-f",
            "pkcs12",
            "-P",
            password,
            "-o",
        ])
        .arg(&tmp_path)
        .status();

    if status.map(|s| s.success()).unwrap_or(false) {
        if let Ok(data) = std::fs::read(&tmp_path) {
            let _ = std::fs::remove_file(&tmp_path);
            let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
            return (Some(b64), Some(password.to_string()));
        }
    }
    let _ = std::fs::remove_file(&tmp_path);
    (None, None)
}

/// Check if a provisioning profile is a development profile (has get-task-allow = true)
pub fn is_development_profile(path: &Path) -> bool {
    let output = Command::new("security")
        .args(["cms", "-D", "-i"])
        .arg(path)
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            // Development profiles have get-task-allow = true
            // Also check for ProvisionedDevices (dev profiles list specific devices)
            stdout.contains("<key>ProvisionedDevices</key>")
                || stdout.contains("<key>get-task-allow</key>\n\t\t<true/>")
        }
        _ => false,
    }
}

/// Find a development provisioning profile (not distribution) for device builds
pub fn find_development_provisioning_profile(bundle_id: &str) -> Option<String> {
    use base64::Engine;

    let perry_dir = dirs::home_dir()?.join(".perry");
    if !perry_dir.exists() {
        return None;
    }

    // Check all .mobileprovision files, prefer ones matching the bundle_id
    let underscored = bundle_id.replace('.', "_");
    let mut candidates: Vec<PathBuf> = Vec::new();

    // Prioritized candidates
    let primary = perry_dir.join(format!("{underscored}.mobileprovision"));
    if primary.exists() {
        candidates.push(primary);
    }
    let fallback = perry_dir.join("perry.mobileprovision");
    if fallback.exists() {
        candidates.push(fallback);
    }

    // All other .mobileprovision files
    if let Ok(entries) = std::fs::read_dir(&perry_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("mobileprovision")
                && !candidates.contains(&path)
            {
                candidates.push(path);
            }
        }
    }

    // Return first development profile found
    for path in &candidates {
        if is_development_profile(path) {
            if let Ok(data) = std::fs::read(path) {
                return Some(base64::engine::general_purpose::STANDARD.encode(&data));
            }
        }
    }

    // No development profile found — return None, hub will handle it
    None
}
