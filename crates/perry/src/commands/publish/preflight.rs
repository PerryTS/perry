use super::*;

/// Run the security audit step before building. Extracted from `run_async`
/// (line ~549) so the runner stays under the file-size cap.
pub(super) async fn run_security_audit_step(
    args: &PublishArgs,
    project_dir: &Path,
    config: &PerryToml,
    format: OutputFormat,
) -> Result<()> {
    if !args.skip_audit {
        if let OutputFormat::Text = format {
            eprintln!("\n  {} Running security audit...", style("→").cyan());
        }

        // Resolve audit settings from CLI flags → perry.toml [audit] → defaults
        let audit_fail_on = if args.audit_fail_on != "C" {
            args.audit_fail_on.clone()
        } else {
            config
                .audit
                .as_ref()
                .and_then(|a| a.fail_on.clone())
                .unwrap_or_else(|| "C".to_string())
        };
        let audit_severity = config
            .audit
            .as_ref()
            .and_then(|a| a.severity.clone())
            .unwrap_or_else(|| "all".to_string());
        let audit_ignore = config
            .audit
            .as_ref()
            .and_then(|a| a.ignore.as_ref().map(|v| v.join(",")))
            .unwrap_or_default();
        let verify_url = if args.verify_url != "https://verify.perryts.com" {
            args.verify_url.clone()
        } else {
            config
                .verify
                .as_ref()
                .and_then(|v| v.url.clone())
                .unwrap_or_else(|| "https://verify.perryts.com".to_string())
        };

        // Infer app_type from target
        let app_type = match args.platform {
            Some(Platform::Ios)
            | Some(Platform::Visionos)
            | Some(Platform::Android)
            | Some(Platform::Macos)
            | Some(Platform::Tvos)
            | Some(Platform::Web)
            | Some(Platform::Windows) => "gui",
            _ => "server",
        };

        match crate::commands::audit::run_audit_check(
            project_dir,
            &verify_url,
            app_type,
            &audit_severity,
            &audit_ignore,
            &audit_fail_on,
            false,
            format,
        )
        .await
        {
            Ok(_) => {}
            Err(e) => {
                bail!("{}\n  Use {} to bypass.", e, style("--skip-audit").yellow());
            }
        }
    }
    Ok(())
}

/// Pre-flight validation for iOS App Store / TestFlight — detect common rejection reasons.
/// Extracted from `run_async` (line ~1558) so the runner stays under the file-size cap.
#[allow(clippy::too_many_arguments)]
pub(super) fn ios_preflight_validation(
    ios_distribute: Option<&str>,
    provisioning_profile_path: Option<&str>,
    bundle_id: &str,
    apple_team_id: Option<&str>,
    icon: Option<&str>,
    project_dir: &Path,
    version: &str,
    build_number: u64,
    apple_certificate_p12_b64: Option<&str>,
    ios_encryption_exempt: Option<bool>,
) -> Result<()> {
    let distribute = ios_distribute.unwrap_or("");
    if distribute == "appstore" || distribute == "testflight" {
        let mut warnings: Vec<String> = Vec::new();
        let mut errors: Vec<String> = Vec::new();

        // 1. Validate provisioning profile bundle ID matches project bundle_id
        if let Some(profile_path) = provisioning_profile_path {
            let profile_data = fs::read(profile_path)
                .with_context(|| format!("Failed to read provisioning profile: {profile_path}"))?;
            let data_str = String::from_utf8_lossy(&profile_data);
            if let (Some(xml_start), Some(xml_end)) =
                (data_str.find("<?xml"), data_str.find("</plist>"))
            {
                let plist_xml = &data_str[xml_start..xml_end + "</plist>".len()];

                // Extract application-identifier from Entitlements
                if let Some(app_id_pos) = plist_xml.find("<key>application-identifier</key>") {
                    let after_key =
                        &plist_xml[app_id_pos + "<key>application-identifier</key>".len()..];
                    if let Some(s_start) = after_key.find("<string>") {
                        if let Some(s_end) = after_key.find("</string>") {
                            let app_identifier = &after_key[s_start + "<string>".len()..s_end];
                            // application-identifier is "TEAMID.bundle.id" — strip team prefix
                            let profile_bundle_id = if let Some(dot_pos) = app_identifier.find('.')
                            {
                                &app_identifier[dot_pos + 1..]
                            } else {
                                app_identifier
                            };

                            if profile_bundle_id != bundle_id && profile_bundle_id != "*" {
                                errors.push(format!(
                                    "Provisioning profile bundle ID mismatch:\n\
                                     \x20\x20  Profile: {} (from {})\n\
                                     \x20\x20  Project: {} (from perry.toml)\n\
                                     \x20\x20  Fix: Create a new provisioning profile for \"{}\" at developer.apple.com",
                                    app_identifier, profile_path, bundle_id, bundle_id
                                ));
                            }
                        }
                    }
                }

                // Check if profile has expired
                if let Some(exp_pos) = plist_xml.find("<key>ExpirationDate</key>") {
                    let after_key = &plist_xml[exp_pos + "<key>ExpirationDate</key>".len()..];
                    if let Some(d_start) = after_key.find("<date>") {
                        if let Some(d_end) = after_key.find("</date>") {
                            let expiry_str = &after_key[d_start + "<date>".len()..d_end];
                            // ISO 8601 dates sort lexicographically; compare with rough "now"
                            let now = {
                                let d = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs();
                                // Convert epoch seconds to YYYY-MM-DD (approx, good enough for comparison)
                                let days = d / 86400;
                                let years = 1970 + days / 365;
                                let remaining_days = days % 365;
                                let month = remaining_days / 30 + 1;
                                let day = remaining_days % 30 + 1;
                                format!("{:04}-{:02}-{:02}", years, month, day)
                            };
                            // Only compare date portion (first 10 chars)
                            let expiry_date = if expiry_str.len() >= 10 {
                                &expiry_str[..10]
                            } else {
                                expiry_str
                            };
                            let now_date = &now[..10];
                            if expiry_date < now_date {
                                errors.push(format!(
                                    "Provisioning profile expired on {}.\n\
                                     \x20\x20  Download a fresh profile from developer.apple.com",
                                    expiry_date
                                ));
                            }
                        }
                    }
                }

                // Extract and validate team ID matches
                if let Some(expected_team) = apple_team_id {
                    if let Some(team_pos) = plist_xml.find("<key>TeamIdentifier</key>") {
                        let after_key = &plist_xml[team_pos + "<key>TeamIdentifier</key>".len()..];
                        if let Some(s_start) = after_key.find("<string>") {
                            if let Some(s_end) = after_key.find("</string>") {
                                let profile_team = &after_key[s_start + "<string>".len()..s_end];
                                if profile_team != expected_team {
                                    errors.push(format!(
                                        "Provisioning profile team ID mismatch:\n\
                                         \x20\x20  Profile: {}\n\
                                         \x20\x20  Config:  {}\n\
                                         \x20\x20  Ensure the profile was created under the correct team",
                                        profile_team, expected_team
                                    ));
                                }
                            }
                        }
                    }
                }
            } else {
                warnings.push("Could not parse provisioning profile to validate bundle ID.".into());
            }
        } else {
            errors.push(
                "No provisioning profile specified. iOS App Store / TestFlight requires one.\n\
                 \x20\x20  Add provisioning_profile to [ios] in perry.toml or pass --provisioning-profile\n\
                 \x20\x20  Run `perry setup ios` to configure automatically"
                .into()
            );
        }

        // 2. Check for app icon (required for App Store)
        if icon.is_none() {
            errors.push(
                "No app icon configured. App Store requires a 1024×1024 icon.\n\
                 \x20\x20  Add to [project] in perry.toml:\n\
                 \x20\x20  icons = { source = \"assets/icon.png\" }"
                    .into(),
            );
        } else if let Some(icon_path) = icon {
            let full_icon_path = project_dir.join(icon_path);
            if !full_icon_path.exists() {
                errors.push(format!(
                    "App icon not found: {}\n\
                     \x20\x20  Ensure the icon file exists at the specified path",
                    icon_path
                ));
            }
        }

        // 3. Validate version string (must be MAJOR.MINOR or MAJOR.MINOR.PATCH)
        let version_parts: Vec<&str> = version.split('.').collect();
        if version_parts.len() < 2
            || version_parts.len() > 3
            || !version_parts.iter().all(|p| p.parse::<u32>().is_ok())
        {
            warnings.push(format!(
                "Version \"{}\" may not be valid for App Store.\n\
                 \x20\x20  Use: MAJOR.MINOR or MAJOR.MINOR.PATCH (e.g., 1.2.0)",
                version
            ));
        }

        // 4. Validate build number is positive
        if build_number == 0 {
            errors.push("Build number must be positive for App Store submission.".into());
        }

        // 5. Check signing certificate is provided
        if apple_certificate_p12_b64.is_none() {
            errors.push(
                "No distribution certificate (.p12) provided. Required for App Store signing.\n\
                 \x20\x20  Add certificate to [ios] in perry.toml or pass --certificate\n\
                 \x20\x20  Run `perry setup ios` to configure automatically"
                    .into(),
            );
        }

        // 6. Warn if encryption_exempt is not set (causes manual prompt in App Store Connect)
        if ios_encryption_exempt.is_none() {
            warnings.push(
                "encryption_exempt not set in [ios] of perry.toml.\n\
                 \x20\x20  Without it, App Store Connect will prompt about export compliance on every upload.\n\
                 \x20\x20  If your app only uses HTTPS (no custom encryption), add:\n\
                 \x20\x20  encryption_exempt = true"
                .into()
            );
        }

        // Print warnings and errors
        if !warnings.is_empty() || !errors.is_empty() {
            println!();
            println!("  {} Pre-flight check results:", style("→").cyan().bold());
            for w in &warnings {
                println!("  {} {}", style("⚠").yellow().bold(), w);
            }
            for e in &errors {
                println!("  {} {}", style("✗").red().bold(), e);
            }
            println!();
        }

        if !errors.is_empty() {
            bail!(
                "Pre-flight validation failed with {} error(s). Fix the issues above before publishing.",
                errors.len()
            );
        }
    }
    Ok(())
}

/// Pre-flight validation for macOS App Store / Both distribution.
/// Extracted from `run_async` (line ~1757) so the runner stays under the file-size cap.
#[allow(clippy::too_many_arguments)]
pub(super) fn macos_preflight_validation(
    macos_distribute: Option<&str>,
    icon: Option<&str>,
    project_dir: &Path,
    version: &str,
    build_number: u64,
    apple_certificate_p12_b64: Option<&str>,
    notarize_cert_b64: Option<&str>,
    macos_encryption_exempt: Option<bool>,
) -> Result<()> {
    let distribute = macos_distribute.unwrap_or("");
    if matches!(distribute, "appstore" | "both") {
        let mut warnings: Vec<String> = Vec::new();
        let mut errors: Vec<String> = Vec::new();

        // 1. Check for app icon (required for App Store)
        if icon.is_none() {
            errors.push(
                "No app icon configured. App Store requires a 1024×1024 icon.\n\
                 \x20\x20  Add to [project] in perry.toml:\n\
                 \x20\x20  icons = { source = \"assets/icon.png\" }"
                    .into(),
            );
        } else if let Some(icon_path) = icon {
            let full_icon_path = project_dir.join(icon_path);
            if !full_icon_path.exists() {
                errors.push(format!(
                    "App icon not found: {}\n\
                     \x20\x20  Ensure the icon file exists at the specified path",
                    icon_path
                ));
            }
        }

        // 2. Validate version string
        let version_parts: Vec<&str> = version.split('.').collect();
        if version_parts.len() < 2
            || version_parts.len() > 3
            || !version_parts.iter().all(|p| p.parse::<u32>().is_ok())
        {
            warnings.push(format!(
                "Version \"{}\" may not be valid for App Store.\n\
                 \x20\x20  Use: MAJOR.MINOR or MAJOR.MINOR.PATCH (e.g., 1.2.0)",
                version
            ));
        }

        // 3. Validate build number is positive
        if build_number == 0 {
            errors.push("Build number must be positive for App Store submission.".into());
        }

        // 4. Check signing certificate
        if apple_certificate_p12_b64.is_none() {
            errors.push(
                "No distribution certificate (.p12) provided. Required for App Store signing.\n\
                 \x20\x20  Add certificate to [macos] in perry.toml or pass --certificate\n\
                 \x20\x20  Run `perry setup macos` to configure automatically"
                    .into(),
            );
        }

        // 5. For "both": check notarize certificate
        if distribute == "both" && notarize_cert_b64.is_none() {
            errors.push(
                "distribute = \"both\" requires a separate Developer ID certificate for notarization.\n\
                 \x20\x20  Add notarize_certificate to [macos] in perry.toml\n\
                 \x20\x20  Run `perry setup macos` and select \"Both\" to configure"
                .into()
            );
        }

        // 6. Warn if encryption_exempt is not set
        if macos_encryption_exempt.is_none() {
            warnings.push(
                "encryption_exempt not set in [macos] of perry.toml.\n\
                 \x20\x20  Without it, App Store Connect will prompt about export compliance on every upload.\n\
                 \x20\x20  If your app only uses HTTPS (no custom encryption), add:\n\
                 \x20\x20  encryption_exempt = true"
                .into()
            );
        }

        // Print warnings and errors
        if !warnings.is_empty() || !errors.is_empty() {
            println!();
            println!("  {} Pre-flight check results:", style("→").cyan().bold());
            for w in &warnings {
                println!("  {} {}", style("⚠").yellow().bold(), w);
            }
            for e in &errors {
                println!("  {} {}", style("✗").red().bold(), e);
            }
            println!();
        }

        if !errors.is_empty() {
            bail!(
                "Pre-flight validation failed with {} error(s). Fix the issues above before publishing.",
                errors.len()
            );
        }
    }
    Ok(())
}
