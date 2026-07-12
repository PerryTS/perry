//! Publish command - build, sign, package and distribute via perry-ship build server

use anyhow::{bail, Context, Result};
use clap::Args;
use console::style;
use dialoguer::{Confirm, Input, Select};
use flate2::write::GzEncoder;
use flate2::Compression;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::multipart;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tokio_tungstenite::tungstenite::Message;
use url::Url;
use walkdir::WalkDir;

use crate::{OutputFormat, Platform};

mod args;
mod config_types;
mod credentials;
mod preflight;
mod resolve;
mod saved_config;
mod server_api;
mod tarball;

#[cfg(test)]
mod tests;

// Re-exports — explicit names only (no globs).
pub use args::PublishArgs;
#[cfg(test)]
pub(crate) use saved_config::IosSavedConfig; // consumed only by tests
pub(crate) use saved_config::{
    check_beta_consent, config_path, is_interactive, load_config, prompt_input, report_beta_error,
    save_config, AndroidSavedConfig, AppleSavedConfig, HarmonyosSavedConfig, PerryConfig,
};
pub(crate) use tarball::create_project_tarball_with_filters;

// Sibling-only items used by run_async and tests.
use config_types::{resolve_build_march, PerryToml};
use credentials::{
    auto_export_p12_from_keychain, prompt_target, resolve_credential, resolve_path_credential,
    validate_credentials_for_distribute,
};
use preflight::{ios_preflight_validation, macos_preflight_validation, run_security_audit_step};
use resolve::{resolve_bundle_id, resolve_entry};
use server_api::{
    BuildManifest, BuildResponse, CredentialsPayload, RegisterResponse, ServerMessage,
};

pub fn run(args: PublishArgs, format: OutputFormat, use_color: bool, _verbose: u8) -> Result<()> {
    if !check_beta_consent("publish") {
        bail!("Aborted.");
    }

    let target_hint = match args.platform {
        Some(Platform::Ios) => Some("ios"),
        Some(Platform::Visionos) => Some("visionos"),
        Some(Platform::Android) | Some(Platform::Wearos) => Some("android"),
        Some(Platform::Linux) => Some("linux"),
        Some(Platform::Windows) => Some("windows"),
        Some(Platform::Web) => Some("web"),
        _ => Some("macos"),
    };

    let rt = tokio::runtime::Runtime::new()?;
    let result = rt.block_on(run_async(args, format, use_color));

    if let Err(ref e) = result {
        report_beta_error("publish", &format!("{e:#}"), target_hint);
    }

    result
}

async fn run_async(args: PublishArgs, format: OutputFormat, _use_color: bool) -> Result<()> {
    let project_dir = args.project.canonicalize().unwrap_or(args.project.clone());

    // Load .env file from project directory (if present) so users can set
    // PERRY_ANDROID_KEYSTORE_PASSWORD, PERRY_LICENSE_KEY, etc. without
    // polluting their shell profile.
    let env_path = project_dir.join(".env");
    if env_path.exists() {
        let _ = dotenvy::from_path(&env_path);
    }

    // Load saved config
    let mut saved = load_config();
    let interactive = is_interactive() && matches!(format, OutputFormat::Text);

    // Read perry.toml
    let perry_toml_path = project_dir.join("perry.toml");
    let mut config: PerryToml = if perry_toml_path.exists() {
        let content = fs::read_to_string(&perry_toml_path).context("Failed to read perry.toml")?;
        toml::from_str(&content).context("Failed to parse perry.toml")?
    } else {
        bail!(
            "No perry.toml found in {}. Run 'perry init' first.",
            project_dir.display()
        );
    };

    // --- Integration: Security Audit ---
    run_security_audit_step(&args, &project_dir, &config, format).await?;

    // Resolve app info (always from perry.toml)
    let app_name = config
        .app
        .as_ref()
        .and_then(|a| a.name.clone())
        .or_else(|| config.project.as_ref().and_then(|p| p.name.clone()))
        .unwrap_or_else(|| "app".into());

    let toml_version = config
        .app
        .as_ref()
        .and_then(|a| a.version.clone())
        .or_else(|| config.project.as_ref().and_then(|p| p.version.clone()))
        .unwrap_or_else(|| "1.0.0".into());

    // build_number is the monotonically increasing integer used as CFBundleVersion (iOS)
    // and versionCode (Android). Auto-incremented on each publish.
    let toml_build_number = config
        .app
        .as_ref()
        .and_then(|a| a.build_number)
        .or_else(|| config.project.as_ref().and_then(|p| p.build_number))
        .unwrap_or(0);

    if let OutputFormat::Text = format {
        println!();
        println!(
            "  {} Perry Publish v{}",
            style("▶").cyan().bold(),
            env!("CARGO_PKG_VERSION")
        );
        println!();
        println!("  App:       {}", style(&app_name).bold());
    }

    // --- Resolve target platform ---
    let target_name = if let Some(p) = args.platform {
        match p {
            Platform::Macos => "macos".to_string(),
            Platform::Ios => "ios".to_string(),
            Platform::Visionos => "visionos".to_string(),
            Platform::Watchos => "watchos".to_string(),
            Platform::Tvos => "tvos".to_string(),
            Platform::Android => "android".to_string(),
            // Wear OS ships through Google Play exactly like an Android app.
            Platform::Wearos => "android".to_string(),
            Platform::Linux => "linux".to_string(),
            Platform::Windows => "windows".to_string(),
            Platform::Web => "web".to_string(),
        }
    } else if let Some(ref t) = saved.default_target {
        // Have a saved default — use it (user can change via prompt below)
        t.clone()
    } else if interactive {
        prompt_target(saved.default_target.as_deref())
    } else {
        bail!("No target specified. Use: perry publish <macos|ios|visionos|tvos|watchos|android|linux|windows|web>");
    };

    let target_display = match target_name.as_str() {
        "ios" => "iOS",
        "visionos" => "visionOS",
        "tvos" => "tvOS",
        "watchos" => "watchOS",
        "android" => "Android",
        "linux" => "Linux",
        "windows" => "Windows",
        "web" => "Web",
        _ => "macOS",
    };
    let is_ios = target_name == "ios";
    let is_visionos = target_name == "visionos";
    let is_tvos = target_name == "tvos";
    let is_watchos = target_name == "watchos";
    let is_android = target_name == "android";
    let is_linux = target_name == "linux";

    // --- Resolve Linux libc (#4826) ---
    // `--libc` CLI flag wins over the `[linux] libc` perry.toml setting.
    // Normalize + validate here so the worker only ever receives `glibc`
    // (or absent) / `musl`. Non-Linux targets ignore it.
    let linux_libc: Option<String> = if is_linux {
        let raw = args
            .libc
            .clone()
            .or_else(|| config.linux.as_ref().and_then(|l| l.libc.clone()));
        match raw.as_deref().map(|s| s.trim().to_ascii_lowercase()) {
            None => None,
            Some(ref s) if s == "glibc" || s == "gnu" || s.is_empty() => Some("glibc".to_string()),
            Some(ref s) if s == "musl" => Some("musl".to_string()),
            Some(other) => bail!(
                "Invalid [linux] libc / --libc value '{other}'. \
                 Supported: glibc (default) or musl."
            ),
        }
    } else {
        None
    };

    // --- Resolve CPU baseline (#6125) — see resolve_build_march ---
    let build_march = resolve_build_march(args.march.as_deref(), config.build.as_ref(), is_linux);
    if is_linux {
        if let (OutputFormat::Text, Some(march)) = (&format, &build_march) {
            println!("  CPU:       {} baseline", style(march).bold());
        }
    }

    // --- Resolve server URL ---
    let server_url = args
        .server
        .clone()
        .or_else(|| saved.server.clone())
        .or_else(|| config.publish.as_ref().and_then(|p| p.server.clone()))
        .unwrap_or_else(|| "https://hub.perryts.com".into());

    // --- Resolve entry point ---
    let entry = resolve_entry(
        &config,
        is_ios,
        is_visionos,
        is_tvos,
        is_watchos,
        is_android,
    );

    // --- Resolve version (allow override) ---
    let version = if interactive {
        let v = prompt_input(
            &format!("  Version [{}]", toml_version),
            Some(&toml_version),
        );
        v.unwrap_or(toml_version.clone())
    } else {
        toml_version.clone()
    };

    // Update perry.toml if version changed
    if version != toml_version {
        if let Ok(content) = fs::read_to_string(&perry_toml_path) {
            let updated = content.replace(
                &format!("version = \"{}\"", toml_version),
                &format!("version = \"{}\"", version),
            );
            if updated != content {
                fs::write(&perry_toml_path, &updated).ok();
            }
        }
    }

    // Extract macos distribute early — needed for build_number auto-increment decision.
    // --notarize flag overrides to "notarize" (DMG only, no App Store upload).
    let macos_distribute = if args.notarize {
        Some("notarize".to_string())
    } else {
        config.macos.as_ref().and_then(|m| m.distribute.clone())
    };

    // Auto-increment build_number for targets that need monotonic build numbers
    let is_windows = target_name == "windows";
    let is_web = target_name == "web";
    let is_macos = !is_ios
        && !is_visionos
        && !is_tvos
        && !is_watchos
        && !is_android
        && !is_linux
        && !is_windows
        && !is_web;
    let macos_needs_upload =
        is_macos && matches!(macos_distribute.as_deref(), Some("appstore") | Some("both"));
    let build_number =
        if is_ios || is_visionos || is_tvos || is_watchos || is_android || macos_needs_upload {
            let n = toml_build_number + 1;
            if let Ok(content) = fs::read_to_string(&perry_toml_path) {
                let updated = if content.contains("build_number =") {
                    content.replace(
                        &format!("build_number = {}", toml_build_number),
                        &format!("build_number = {}", n),
                    )
                } else {
                    // Insert build_number after the version line
                    content.replace(
                        &format!("version = \"{}\"", version),
                        &format!("version = \"{}\"\nbuild_number = {}", version, n),
                    )
                };
                fs::write(&perry_toml_path, &updated).ok();
            }
            n
        } else {
            toml_build_number
        };

    let app_bundle_id = config.app.as_ref().and_then(|a| a.bundle_id.clone());
    let project_bundle_id = config.project.as_ref().and_then(|p| p.bundle_id.clone());
    let bundle_id = resolve_bundle_id(
        &config,
        &app_name,
        &app_bundle_id,
        &project_bundle_id,
        is_ios,
        is_visionos,
        is_tvos,
        is_watchos,
        is_android,
    );

    let mut icon = config
        .project
        .as_ref()
        .and_then(|p| p.icons.as_ref())
        .and_then(|i| i.source.clone())
        .or_else(|| {
            config
                .app
                .as_ref()
                .and_then(|a| a.icons.as_ref())
                .and_then(|i| i.source.clone())
        });

    // Prompt for icon if missing and building for a platform that needs one
    let needs_icon = is_ios
        || is_visionos
        || is_android
        || (is_macos && matches!(macos_distribute.as_deref(), Some("appstore") | Some("both")));
    if icon.is_none() && interactive && needs_icon {
        println!();
        println!("  {} No app icon configured.", style("!").yellow().bold());
        println!("  App Store requires a 1024\u{00d7}1024 PNG icon.");
        if let Some(path_str) = prompt_input("  Path to icon image (or Enter to skip)", None) {
            let src_path = Path::new(&path_str);
            if !src_path.exists() {
                println!(
                    "  {} Icon file not found: {}",
                    style("!").yellow(),
                    path_str
                );
            } else {
                // Copy to assets/icon.png in the project
                let assets_dir = project_dir.join("assets");
                if !assets_dir.exists() {
                    fs::create_dir_all(&assets_dir).ok();
                }
                let dest = assets_dir.join("icon.png");
                if let Err(e) = fs::copy(src_path, &dest) {
                    println!("  {} Failed to copy icon: {}", style("!").yellow(), e);
                } else {
                    // Update perry.toml
                    let icon_rel = "assets/icon.png";
                    if let Ok(content) = fs::read_to_string(&perry_toml_path) {
                        let updated = if content.contains("[project]") {
                            // Insert icons line after [project] section header
                            content.replace(
                                "[project]",
                                &format!("[project]\nicons = {{ source = \"{}\" }}", icon_rel),
                            )
                        } else {
                            // Add [project] section with icons
                            format!(
                                "{}\n[project]\nicons = {{ source = \"{}\" }}\n",
                                content.trim_end(),
                                icon_rel
                            )
                        };
                        fs::write(&perry_toml_path, &updated).ok();
                    }
                    icon = Some(icon_rel.to_string());
                    println!(
                        "  {} Icon saved to {} and perry.toml updated.",
                        style("✓").green().bold(),
                        icon_rel
                    );
                }
            }
        }
    }

    let category = config.macos.as_ref().and_then(|m| m.category.clone());
    let minimum_os = config.macos.as_ref().and_then(|m| m.minimum_os.clone());
    let entitlements = config.macos.as_ref().and_then(|m| m.entitlements.clone());
    // macos_distribute already extracted above (before build_number auto-increment)
    let macos_signing_identity = if args.notarize {
        config
            .macos
            .as_ref()
            .and_then(|m| m.notarize_signing_identity.clone())
            .or_else(|| {
                config
                    .macos
                    .as_ref()
                    .and_then(|m| m.signing_identity.clone())
            })
    } else {
        config
            .macos
            .as_ref()
            .and_then(|m| m.signing_identity.clone())
    };

    // iOS-specific config from perry.toml
    let ios_deployment_target = config.ios.as_ref().and_then(|i| {
        i.deployment_target
            .clone()
            .or_else(|| i.minimum_version.clone())
    });
    let ios_device_family = config.ios.as_ref().and_then(|i| i.device_family.clone());
    let ios_orientations = config.ios.as_ref().and_then(|i| i.orientations.clone());
    let ios_capabilities = config.ios.as_ref().and_then(|i| i.capabilities.clone());
    let mut ios_distribute = config.ios.as_ref().and_then(|i| i.distribute.clone());
    let ios_encryption_exempt = config.ios.as_ref().and_then(|i| i.encryption_exempt);
    let ios_info_plist = config.ios.as_ref().and_then(|i| i.info_plist.clone());
    let visionos_deployment_target = config.visionos.as_ref().and_then(|i| {
        i.deployment_target
            .clone()
            .or_else(|| i.minimum_version.clone())
    });
    let visionos_distribute = config.visionos.as_ref().and_then(|i| i.distribute.clone());
    let visionos_encryption_exempt = config.visionos.as_ref().and_then(|i| i.encryption_exempt);
    let visionos_info_plist = config.visionos.as_ref().and_then(|i| i.info_plist.clone());
    let tvos_distribute = config.tvos.as_ref().and_then(|t| t.distribute.clone());
    let watchos_distribute = config.watchos.as_ref().and_then(|w| w.distribute.clone());
    let macos_encryption_exempt = config.macos.as_ref().and_then(|m| m.encryption_exempt);

    // Android-specific config from perry.toml
    let android_min_sdk = config.android.as_ref().and_then(|a| a.min_sdk.clone());
    let android_target_sdk = config.android.as_ref().and_then(|a| a.target_sdk.clone());
    let android_version_code = config.android.as_ref().and_then(|a| a.version_code);
    let android_permissions = config.android.as_ref().and_then(|a| a.permissions.clone());
    let android_distribute = config.android.as_ref().and_then(|a| a.distribute.clone());

    // --- Resolve authentication ---
    // Priority: --license-key flag -> PERRY_LICENSE_KEY env -> api_token -> saved license_key -> auto-register
    let api_token = saved.api_token.clone();
    let license_key = args
        .license_key
        .clone()
        .or_else(|| std::env::var("PERRY_LICENSE_KEY").ok())
        .or_else(|| saved.license_key.clone());

    let (license_key, use_bearer_auth) = if let Some(ref token) = api_token {
        // Prefer API token if available (user ran 'perry login')
        (token.clone(), true)
    } else {
        match license_key {
            Some(k) => (k, false),
            None => {
                // Auto-register a free device-bound license
                if let OutputFormat::Text = format {
                    print!("  No license key found. Registering free license...");
                    std::io::stdout().flush().ok();
                }
                let key = auto_register_license(&server_url).await?;
                if let OutputFormat::Text = format {
                    println!(" {}", style("done").green());
                    println!(
                        "  {} License: {}",
                        style("✓").green().bold(),
                        style(&key).bold()
                    );
                }
                // Save immediately
                saved.license_key = Some(key.clone());
                save_config(&saved).ok();
                (key, false)
            }
        }
    };

    // Auto-trigger iOS/macOS setup if not configured
    if (is_ios || is_macos) && interactive {
        let has_platform_cert = if is_ios {
            config
                .ios
                .as_ref()
                .and_then(|i| i.certificate.as_deref())
                .is_some()
        } else {
            config
                .macos
                .as_ref()
                .and_then(|m| m.certificate.as_deref())
                .is_some()
        };
        let has_apple_config = args.certificate.is_some()
            || std::env::var("PERRY_APPLE_CERTIFICATE").is_ok()
            || has_platform_cert;
        if !has_apple_config {
            let platform = if is_ios { "iOS" } else { "macOS" };
            println!();
            println!(
                "  {} {platform} not configured — running setup wizard",
                style("!").yellow()
            );
            println!();
            if is_ios {
                super::setup::ios_wizard(&mut saved)?;
            } else {
                super::setup::macos_wizard(&mut saved)?;
            }
            save_config(&saved)?;
            // Re-read perry.toml since setup may have updated it
            if let Ok(content) = fs::read_to_string(&perry_toml_path) {
                if let Ok(reloaded) = toml::from_str::<PerryToml>(&content) {
                    ios_distribute = reloaded.ios.as_ref().and_then(|i| i.distribute.clone());
                    config = reloaded;
                }
            }
            println!();
        }
    }

    // --- Resolve credentials using CLI → env → perry.toml (project) → ~/.perry/config.toml (global) → interactive prompt ---

    // Per-project credentials from perry.toml [ios] or [macos] take priority over global config
    let toml_team_id = if is_ios {
        config.ios.as_ref().and_then(|i| i.team_id.clone())
    } else {
        config.macos.as_ref().and_then(|m| m.team_id.clone())
    };
    let toml_signing_identity = if is_ios {
        config.ios.as_ref().and_then(|i| i.signing_identity.clone())
    } else if args.notarize {
        // --notarize: use Developer ID identity/cert instead of App Store ones
        config
            .macos
            .as_ref()
            .and_then(|m| m.notarize_signing_identity.clone())
            .or_else(|| {
                config
                    .macos
                    .as_ref()
                    .and_then(|m| m.signing_identity.clone())
            })
    } else {
        config
            .macos
            .as_ref()
            .and_then(|m| m.signing_identity.clone())
    };
    let toml_certificate = if is_ios {
        config.ios.as_ref().and_then(|i| i.certificate.clone())
    } else if args.notarize {
        // --notarize: use the Developer ID cert for notarization
        config
            .macos
            .as_ref()
            .and_then(|m| m.notarize_certificate.clone())
            .or_else(|| config.macos.as_ref().and_then(|m| m.certificate.clone()))
    } else {
        config.macos.as_ref().and_then(|m| m.certificate.clone())
    };
    let toml_key_id = if is_ios {
        config.ios.as_ref().and_then(|i| i.key_id.clone())
    } else {
        config.macos.as_ref().and_then(|m| m.key_id.clone())
    };
    let toml_issuer_id = if is_ios {
        config.ios.as_ref().and_then(|i| i.issuer_id.clone())
    } else {
        config.macos.as_ref().and_then(|m| m.issuer_id.clone())
    };
    let toml_p8_key_path = if is_ios {
        config.ios.as_ref().and_then(|i| i.p8_key_path.clone())
    } else {
        config.macos.as_ref().and_then(|m| m.p8_key_path.clone())
    };
    let toml_provisioning_profile = if is_ios {
        config
            .ios
            .as_ref()
            .and_then(|i| i.provisioning_profile.clone())
    } else if is_macos {
        config
            .macos
            .as_ref()
            .and_then(|m| m.provisioning_profile.clone())
    } else {
        None
    };

    // Apple credentials (for macOS and iOS)
    let apple_team_id = if !is_android {
        resolve_credential(
            args.apple_team_id.as_deref(),
            "PERRY_APPLE_TEAM_ID",
            toml_team_id
                .as_deref()
                .or_else(|| saved.apple.as_ref().and_then(|a| a.team_id.as_deref())),
            "  Apple Team ID",
            false,
            interactive,
        )
    } else {
        args.apple_team_id.clone()
    };

    let apple_identity_base = if !is_android {
        resolve_credential(
            args.apple_identity.as_deref(),
            "PERRY_APPLE_IDENTITY",
            toml_signing_identity.as_deref(),
            "  Signing Identity",
            false,
            interactive,
        )
    } else {
        args.apple_identity.clone()
    };
    // For macOS, prefer a target-specific signing_identity from perry.toml [macos]
    let apple_identity = if !is_ios && !is_android && !is_linux {
        macos_signing_identity
            .clone()
            .or_else(|| apple_identity_base.clone())
    } else {
        apple_identity_base.clone()
    };

    let apple_p8_key_path = if !is_android {
        resolve_path_credential(
            args.apple_p8_key.as_deref(),
            "PERRY_APPLE_P8_KEY",
            toml_p8_key_path
                .as_deref()
                .or_else(|| saved.apple.as_ref().and_then(|a| a.p8_key_path.as_deref())),
            "  App Store Connect .p8 key path",
            interactive,
        )
    } else {
        args.apple_p8_key
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
    };

    // .p12 certificate for code signing (path saved, password never saved)
    // Priority: CLI → env → perry.toml → ~/.perry/config.toml → auto-export from Keychain → skip
    let (apple_certificate_path, auto_exported_p12) = if !is_android && !is_linux {
        // Check explicit path first (CLI flag, env var, perry.toml, or saved config)
        let explicit_path = resolve_path_credential(
            args.certificate.as_deref(),
            "PERRY_APPLE_CERTIFICATE",
            toml_certificate.as_deref(),
            "",    // empty prompt — don't prompt, we'll try auto-export instead
            false, // never prompt for path
        );
        if explicit_path.is_some() {
            (explicit_path, None)
        } else {
            // Try auto-export from Keychain
            let auto = auto_export_p12_from_keychain(apple_identity.as_deref(), interactive);
            (None, auto)
        }
    } else {
        (None, None)
    };
    let apple_certificate_password = if apple_certificate_path.is_some() {
        // Explicit .p12 file — need password
        // Check for auto-generated cert (lives in ~/.perry/) — use known password
        let is_auto_generated = apple_certificate_path
            .as_deref()
            .map(|p| p.contains("/.perry/"))
            .unwrap_or(false);
        std::env::var("PERRY_APPLE_CERTIFICATE_PASSWORD")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| {
                if is_auto_generated {
                    Some("perry-auto".to_string())
                } else if interactive {
                    dialoguer::Password::new()
                        .with_prompt("  Certificate password")
                        .interact()
                        .ok()
                        .filter(|s| !s.is_empty())
                } else {
                    None
                }
            })
    } else {
        None
    };

    let apple_key_id = if !is_android {
        resolve_credential(
            args.apple_key_id.as_deref(),
            "PERRY_APPLE_KEY_ID",
            toml_key_id
                .as_deref()
                .or_else(|| saved.apple.as_ref().and_then(|a| a.key_id.as_deref())),
            "  App Store Connect Key ID",
            false,
            interactive,
        )
    } else {
        args.apple_key_id.clone()
    };

    let apple_issuer_id = if !is_android {
        resolve_credential(
            args.apple_issuer_id.as_deref(),
            "PERRY_APPLE_ISSUER_ID",
            toml_issuer_id
                .as_deref()
                .or_else(|| saved.apple.as_ref().and_then(|a| a.issuer_id.as_deref())),
            "  App Store Connect Issuer ID",
            false,
            interactive,
        )
    } else {
        args.apple_issuer_id.clone()
    };

    // Provisioning profile (iOS always, macOS when distributing to App Store)
    let macos_needs_profile = is_macos
        && matches!(
            macos_distribute.as_deref(),
            Some("appstore") | Some("both") | Some("testflight")
        );
    let provisioning_profile_path = if is_ios || macos_needs_profile {
        resolve_path_credential(
            args.provisioning_profile.as_deref(),
            "PERRY_PROVISIONING_PROFILE",
            toml_provisioning_profile.as_deref(),
            "  Provisioning profile path",
            interactive,
        )
    } else {
        args.provisioning_profile
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
    };

    // Auto-trigger Android setup if not configured
    if is_android && interactive {
        let has_keystore = args.android_keystore.is_some()
            || std::env::var("PERRY_ANDROID_KEYSTORE").is_ok()
            || saved
                .android
                .as_ref()
                .and_then(|a| a.keystore_path.as_deref())
                .is_some()
            || config
                .android
                .as_ref()
                .and_then(|a| a.keystore.as_deref())
                .is_some();
        if !has_keystore {
            println!();
            println!(
                "  {} Android not configured — running setup wizard",
                style("!").yellow()
            );
            println!();
            super::setup::android_wizard(&mut saved)?;
            save_config(&saved)?;
            println!();
        }
    }

    // Android credentials — check saved config first, then perry.toml [android] section
    let toml_android_keystore = config.android.as_ref().and_then(|a| a.keystore.as_deref());
    let toml_android_key_alias = config.android.as_ref().and_then(|a| a.key_alias.as_deref());

    let android_keystore_path = if is_android {
        resolve_path_credential(
            args.android_keystore.as_deref(),
            "PERRY_ANDROID_KEYSTORE",
            saved
                .android
                .as_ref()
                .and_then(|a| a.keystore_path.as_deref())
                .or(toml_android_keystore),
            "  Android keystore path",
            interactive,
        )
    } else {
        args.android_keystore
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
    };

    let android_key_alias = if is_android {
        resolve_credential(
            args.android_key_alias.as_deref(),
            "PERRY_ANDROID_KEY_ALIAS",
            saved
                .android
                .as_ref()
                .and_then(|a| a.key_alias.as_deref())
                .or(toml_android_key_alias),
            "  Android key alias",
            false,
            interactive,
        )
    } else {
        args.android_key_alias.clone()
    };

    // Passwords are NEVER saved — always from CLI, env, or prompt
    let android_keystore_password = args
        .android_keystore_password
        .clone()
        .or_else(|| std::env::var("PERRY_ANDROID_KEYSTORE_PASSWORD").ok());
    let android_keystore_password = if android_keystore_password.is_none()
        && is_android
        && android_keystore_path.is_some()
        && interactive
    {
        prompt_input("  Android keystore password", None)
    } else {
        android_keystore_password
    };

    let android_key_password = args
        .android_key_password
        .clone()
        .or_else(|| std::env::var("PERRY_ANDROID_KEY_PASSWORD").ok());

    // Google Play service account JSON
    let toml_google_play_key = config
        .android
        .as_ref()
        .and_then(|a| a.google_play_key.as_deref());
    let google_play_key_path = if is_android {
        resolve_path_credential(
            args.google_play_key.as_deref(),
            "PERRY_GOOGLE_PLAY_KEY_PATH",
            saved
                .android
                .as_ref()
                .and_then(|a| a.google_play_key_path.as_deref())
                .or(toml_google_play_key),
            "  Google Play service account JSON path",
            interactive && android_distribute.as_deref() == Some("playstore"),
        )
    } else {
        args.google_play_key
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
    };

    // Read file contents for credentials that need to be sent as content
    let p8_key_content = if let Some(ref path_str) = apple_p8_key_path {
        let path = Path::new(path_str);
        if path.exists() {
            Some(
                fs::read_to_string(path)
                    .with_context(|| format!("Failed to read .p8 key from {path_str}"))?,
            )
        } else {
            None
        }
    } else {
        None
    };

    let provisioning_profile_b64 = if let Some(ref path_str) = provisioning_profile_path {
        let path = Path::new(path_str);
        if path.exists() {
            use base64::Engine;
            let data = fs::read(path)
                .with_context(|| format!("Failed to read provisioning profile: {path_str}"))?;
            Some(base64::engine::general_purpose::STANDARD.encode(&data))
        } else {
            None
        }
    } else {
        None
    };

    let android_keystore_b64 = if let Some(ref path_str) = android_keystore_path {
        let path = Path::new(path_str);
        if path.exists() {
            use base64::Engine;
            let data = fs::read(path)
                .with_context(|| format!("Failed to read Android keystore: {path_str}"))?;
            Some(base64::engine::general_purpose::STANDARD.encode(&data))
        } else {
            None
        }
    } else {
        None
    };

    let (apple_certificate_p12_b64, apple_certificate_password) =
        if let Some((b64, pass)) = auto_exported_p12 {
            // Auto-exported from Keychain — data and password already available
            (Some(b64), Some(pass))
        } else if let Some(ref path_str) = apple_certificate_path {
            let path = Path::new(path_str);
            if path.exists() {
                use base64::Engine;
                let data = fs::read(path)
                    .with_context(|| format!("Failed to read .p12 certificate: {path_str}"))?;
                (
                    Some(base64::engine::general_purpose::STANDARD.encode(&data)),
                    apple_certificate_password,
                )
            } else {
                (None, apple_certificate_password)
            }
        } else {
            (None, None)
        };

    // For macOS distribute = "both": resolve the separate Developer ID cert for notarization
    let (notarize_cert_b64, notarize_cert_password, notarize_identity) =
        if is_macos && macos_distribute.as_deref() == Some("both") {
            let notarize_cert_path = config
                .macos
                .as_ref()
                .and_then(|m| m.notarize_certificate.clone());
            let notarize_identity = config
                .macos
                .as_ref()
                .and_then(|m| m.notarize_signing_identity.clone());
            let cert_b64 = if let Some(ref path_str) = notarize_cert_path {
                let path = Path::new(path_str);
                if path.exists() {
                    use base64::Engine;
                    let data = fs::read(path)
                        .with_context(|| format!("Failed to read notarize .p12: {path_str}"))?;
                    Some(base64::engine::general_purpose::STANDARD.encode(&data))
                } else {
                    None
                }
            } else {
                None
            };
            let is_auto_generated_notarize = notarize_cert_path
                .as_deref()
                .map(|p| p.contains("/.perry/"))
                .unwrap_or(false);
            let password = std::env::var("PERRY_APPLE_NOTARIZE_CERTIFICATE_PASSWORD")
                .ok()
                .or_else(|| {
                    if is_auto_generated_notarize {
                        Some("perry-auto".to_string())
                    } else {
                        apple_certificate_password.clone()
                    }
                });
            (cert_b64, password, notarize_identity)
        } else {
            (None, None, None)
        };

    // For macOS appstore/both: resolve the separate installer cert for .pkg signing
    let (installer_cert_b64, installer_cert_password) = if is_macos
        && (macos_distribute.as_deref() == Some("both")
            || macos_distribute.as_deref() == Some("appstore")
            || macos_distribute.as_deref() == Some("testflight"))
    {
        let installer_cert_path = config
            .macos
            .as_ref()
            .and_then(|m| m.installer_certificate.clone());
        let cert_b64 = if let Some(ref path_str) = installer_cert_path {
            let path = Path::new(path_str);
            if path.exists() {
                use base64::Engine;
                let data = fs::read(path)
                    .with_context(|| format!("Failed to read installer .p12: {path_str}"))?;
                Some(base64::engine::general_purpose::STANDARD.encode(&data))
            } else {
                None
            }
        } else {
            None
        };
        let is_auto_generated = installer_cert_path
            .as_deref()
            .map(|p| p.contains("/.perry/"))
            .unwrap_or(false);
        let password = std::env::var("PERRY_APPLE_INSTALLER_CERTIFICATE_PASSWORD")
            .ok()
            .or_else(|| {
                if is_auto_generated {
                    Some("perry-auto".to_string())
                } else {
                    apple_certificate_password.clone()
                }
            });
        (cert_b64, password)
    } else {
        (None, None)
    };

    let google_play_json = if let Some(ref path_str) = google_play_key_path {
        let path = Path::new(path_str);
        if path.exists() {
            Some(
                fs::read_to_string(path)
                    .with_context(|| format!("Failed to read Google Play key: {path_str}"))?,
            )
        } else {
            None
        }
    } else {
        None
    };

    // Pre-flight credential validation — fail fast before building the tarball
    {
        let is_macos = !is_android
            && !is_ios
            && !is_visionos
            && !is_tvos
            && !is_watchos
            && !is_linux
            && !is_windows
            && !is_web;
        validate_credentials_for_distribute(
            is_android,
            android_distribute.as_deref(),
            google_play_json.as_deref(),
            is_ios,
            ios_distribute.as_deref(),
            apple_key_id.as_deref(),
            apple_issuer_id.as_deref(),
            p8_key_content.as_deref(),
            is_macos,
            macos_distribute.as_deref(),
            is_tvos,
            tvos_distribute.as_deref(),
            is_watchos,
            watchos_distribute.as_deref(),
        )?;
    }

    // A standalone watchOS app uploaded to App Store Connect must have its own
    // unique bundle id, distinct from any companion iOS app. Require it explicitly
    // rather than silently inheriting (and colliding with) the iOS bundle id.
    if is_watchos
        && matches!(
            watchos_distribute.as_deref(),
            Some("appstore") | Some("testflight")
        )
        && config
            .watchos
            .as_ref()
            .and_then(|w| w.bundle_id.clone())
            .is_none()
    {
        bail!(
            "watchos.distribute = \"{}\" requires an explicit [watchos] bundle_id.\n\
             A standalone watchOS app must have its own bundle id, distinct from your iOS app \
             (App Store Connect rejects duplicate bundle ids).\n\
             Run `perry setup watchos` or add `bundle_id = \"...\"` under [watchos] in perry.toml.",
            watchos_distribute.as_deref().unwrap_or("appstore")
        );
    }

    // Pre-flight validation for iOS App Store / TestFlight — detect common rejection reasons
    if is_ios {
        ios_preflight_validation(
            ios_distribute.as_deref(),
            provisioning_profile_path.as_deref(),
            &bundle_id,
            apple_team_id.as_deref(),
            icon.as_deref(),
            &project_dir,
            &version,
            build_number,
            apple_certificate_p12_b64.as_deref(),
            ios_encryption_exempt,
        )?;
    }

    // Pre-flight validation for macOS App Store / Both
    if is_macos {
        macos_preflight_validation(
            macos_distribute.as_deref(),
            icon.as_deref(),
            &project_dir,
            &version,
            build_number,
            apple_certificate_p12_b64.as_deref(),
            notarize_cert_b64.as_deref(),
            macos_encryption_exempt,
        )?;
    }

    // --- Show summary and confirm ---
    if let OutputFormat::Text = format {
        println!("  Version:   {version}");
        println!("  Bundle ID: {bundle_id}");
        println!("  Target:    {target_display}");
        println!("  Server:    {server_url}");
        if is_windows {
            if config
                .windows
                .as_ref()
                .and_then(|w| w.gcloud_kms_key.as_ref())
                .is_some()
            {
                println!("  Signing:   Google Cloud KMS (EV code signing)");
            }
        } else if is_ios || is_macos || is_tvos || is_watchos {
            if let Some(ref id) = apple_identity {
                println!("  Signing:   {id}");
            }
        }
        if is_android && android_distribute.as_deref() == Some("playstore") {
            println!("  Distribute: Google Play");
        } else if is_ios
            && matches!(
                ios_distribute.as_deref(),
                Some("appstore") | Some("testflight")
            )
        {
            println!("  Distribute: App Store Connect (TestFlight)");
        } else if (is_tvos || is_watchos)
            && matches!(
                if is_tvos {
                    tvos_distribute.as_deref()
                } else {
                    watchos_distribute.as_deref()
                },
                Some("appstore") | Some("testflight")
            )
        {
            println!("  Distribute: App Store Connect (TestFlight)");
        } else if is_macos {
            match macos_distribute.as_deref() {
                Some("both") => println!("  Distribute: App Store + Notarized DMG"),
                Some("appstore") => println!("  Distribute: App Store Connect (TestFlight)"),
                Some("notarize") => println!("  Distribute: Notarized DMG"),
                _ => {}
            }
        }
        println!();
    }

    if interactive {
        let confirm = Confirm::new()
            .with_prompt("  Confirm and publish?")
            .default(true)
            .interact()
            .unwrap_or(false);
        if !confirm {
            bail!("Publish cancelled.");
        }
        println!();
    }

    // --- Save non-sensitive config ---
    saved.license_key = Some(license_key.clone());
    saved.default_target = Some(target_name.clone());
    if server_url != "https://hub.perryts.com" {
        saved.server = Some(server_url.clone());
    }
    if !is_android {
        let apple = saved.apple.get_or_insert_with(AppleSavedConfig::default);
        if apple_team_id.is_some() {
            apple.team_id = apple_team_id.clone();
        }
        if apple_p8_key_path.is_some() {
            apple.p8_key_path = apple_p8_key_path.clone();
        }
        if apple_key_id.is_some() {
            apple.key_id = apple_key_id.clone();
        }
        if apple_issuer_id.is_some() {
            apple.issuer_id = apple_issuer_id.clone();
        }
        // Project-specific fields (signing_identity, certificate, provisioning_profile)
        // are NOT saved to global config — they belong in perry.toml [ios]/[macos]
    }
    if is_android {
        let android_saved = saved
            .android
            .get_or_insert_with(AndroidSavedConfig::default);
        if android_keystore_path.is_some() {
            android_saved.keystore_path = android_keystore_path.clone();
        }
        if android_key_alias.is_some() {
            android_saved.key_alias = android_key_alias.clone();
        }
        if google_play_key_path.is_some() {
            android_saved.google_play_key_path = google_play_key_path.clone();
        }
    }
    if let Err(e) = save_config(&saved) {
        if let OutputFormat::Text = format {
            println!("  {} Could not save config: {e}", style("!").yellow());
        }
    } else if interactive {
        println!(
            "  Saved settings to {}",
            style(config_path().display()).dim()
        );
        println!();
    }

    // Build manifest
    // For iOS/Android/macOS-appstore: version = build_number (CFBundleVersion), short_version = marketing version
    // For macOS-notarize/Linux: version = marketing version string, short_version = None
    let (manifest_version, manifest_short_version) =
        if is_ios || is_visionos || is_android || macos_needs_upload {
            (build_number.to_string(), Some(version.clone()))
        } else {
            (version.clone(), None)
        };
    // Home Screen display name (CFBundleDisplayName): prefer the target's own
    // [platform].display_name, else [project].display_name. None → the build
    // service omits the key and the icon label falls back to the app name.
    let platform_display_name = if is_ios {
        config.ios.as_ref().and_then(|c| c.display_name.clone())
    } else if is_macos {
        config.macos.as_ref().and_then(|c| c.display_name.clone())
    } else if is_tvos {
        config.tvos.as_ref().and_then(|c| c.display_name.clone())
    } else {
        None
    };
    let display_name = platform_display_name
        .or_else(|| config.project.as_ref().and_then(|p| p.display_name.clone()));

    let manifest = BuildManifest {
        app_name: app_name.clone(),
        display_name,
        bundle_id,
        version: manifest_version,
        short_version: manifest_short_version,
        entry,
        icon: icon.clone(),
        targets: vec![target_name.clone()],
        category,
        minimum_os_version: minimum_os,
        entitlements,
        ios_deployment_target: if is_ios { ios_deployment_target } else { None },
        ios_device_family: if is_ios { ios_device_family } else { None },
        ios_orientations: if is_ios { ios_orientations } else { None },
        ios_capabilities: if is_ios { ios_capabilities } else { None },
        ios_distribute: if is_ios { ios_distribute } else { None },
        ios_encryption_exempt: if is_ios { ios_encryption_exempt } else { None },
        ios_info_plist: if is_ios { ios_info_plist } else { None },
        visionos_deployment_target: if is_visionos {
            visionos_deployment_target
        } else {
            None
        },
        visionos_distribute: if is_visionos {
            visionos_distribute
        } else {
            None
        },
        visionos_encryption_exempt: if is_visionos {
            visionos_encryption_exempt
        } else {
            None
        },
        visionos_info_plist: if is_visionos {
            visionos_info_plist
        } else {
            None
        },
        macos_distribute: if is_macos { macos_distribute } else { None },
        macos_encryption_exempt: if is_macos {
            macos_encryption_exempt
        } else {
            None
        },
        tvos_deployment_target: if is_tvos {
            config
                .tvos
                .as_ref()
                .and_then(|t| t.deployment_target.clone())
        } else {
            None
        },
        tvos_encryption_exempt: if is_tvos {
            config.tvos.as_ref().and_then(|t| t.encryption_exempt)
        } else {
            None
        },
        tvos_info_plist: if is_tvos {
            config.tvos.as_ref().and_then(|t| t.info_plist.clone())
        } else {
            None
        },
        tvos_distribute: if is_tvos { tvos_distribute } else { None },
        watchos_deployment_target: if is_watchos {
            config
                .watchos
                .as_ref()
                .and_then(|w| w.deployment_target.clone())
        } else {
            None
        },
        watchos_encryption_exempt: if is_watchos {
            config.watchos.as_ref().and_then(|w| w.encryption_exempt)
        } else {
            None
        },
        watchos_info_plist: if is_watchos {
            config.watchos.as_ref().and_then(|w| w.info_plist.clone())
        } else {
            None
        },
        watchos_distribute: if is_watchos { watchos_distribute } else { None },
        android_min_sdk: if is_android { android_min_sdk } else { None },
        android_target_sdk: if is_android { android_target_sdk } else { None },
        android_permissions: if is_android {
            android_permissions
        } else {
            None
        },
        android_distribute: if is_android { android_distribute } else { None },
        android_version_code: if is_android {
            android_version_code
        } else {
            None
        },
        linux_format: if is_linux {
            config.linux.as_ref().and_then(|l| l.format.clone())
        } else {
            None
        },
        linux_category: if is_linux {
            config.linux.as_ref().and_then(|l| l.category.clone())
        } else {
            None
        },
        linux_description: if is_linux {
            config
                .linux
                .as_ref()
                .and_then(|l| l.description.clone())
                .or_else(|| config.app.as_ref().and_then(|a| a.description.clone()))
        } else {
            None
        },
        linux_libc: linux_libc.clone(),
        build_march: build_march.clone(),
        release_notes: config.release_notes.clone(),
        features: config.project.as_ref().and_then(|p| p.features.clone()),
    };

    // Read GCloud KMS signing credentials for Windows
    let (gcloud_kms_key, gcloud_kms_cert_b64, gcloud_sa_b64) = if is_windows {
        let win = config.windows.as_ref();
        let kms_key = win.and_then(|w| w.gcloud_kms_key.clone());
        let cert_b64 = win
            .and_then(|w| w.gcloud_kms_cert.as_ref())
            .and_then(|path_str| {
                let path = if path_str.starts_with("~/") {
                    dirs::home_dir().unwrap_or_default().join(&path_str[2..])
                } else {
                    std::path::PathBuf::from(path_str)
                };
                if path.exists() {
                    use base64::Engine;
                    fs::read(&path)
                        .ok()
                        .map(|data| base64::engine::general_purpose::STANDARD.encode(&data))
                } else {
                    None
                }
            });
        let sa_b64 = win
            .and_then(|w| w.gcloud_service_account.as_ref())
            .and_then(|path_str| {
                let path = if path_str.starts_with("~/") {
                    dirs::home_dir().unwrap_or_default().join(&path_str[2..])
                } else {
                    std::path::PathBuf::from(path_str)
                };
                if path.exists() {
                    use base64::Engine;
                    fs::read(&path)
                        .ok()
                        .map(|data| base64::engine::general_purpose::STANDARD.encode(&data))
                } else {
                    None
                }
            });
        (kms_key, cert_b64, sa_b64)
    } else {
        (None, None, None)
    };

    let credentials = CredentialsPayload {
        apple_team_id,
        apple_signing_identity: apple_identity,
        apple_key_id,
        apple_issuer_id,
        apple_p8_key: p8_key_content,
        provisioning_profile_base64: provisioning_profile_b64,
        apple_certificate_p12_base64: apple_certificate_p12_b64,
        apple_certificate_password,
        apple_notarize_certificate_p12_base64: notarize_cert_b64,
        apple_notarize_certificate_password: notarize_cert_password,
        apple_notarize_signing_identity: notarize_identity,
        apple_installer_certificate_p12_base64: installer_cert_b64,
        apple_installer_certificate_password: installer_cert_password,
        android_keystore_base64: android_keystore_b64,
        android_keystore_password,
        android_key_alias,
        android_key_password,
        google_play_service_account_json: google_play_json,
        gcloud_kms_key,
        gcloud_kms_cert_base64: gcloud_kms_cert_b64,
        gcloud_service_account_base64: gcloud_sa_b64,
    };

    // Create project tarball
    if let OutputFormat::Text = format {
        print!("  Packaging project...");
        std::io::stdout().flush().ok();
    }

    let publish_excludes = config
        .publish
        .as_ref()
        .and_then(|p| p.exclude.clone())
        .unwrap_or_default();
    let publish_includes = config
        .publish
        .as_ref()
        .and_then(|p| p.include.clone())
        .unwrap_or_default();

    // #1303 — force the vendored optional-framework dir
    // (`[google_auth].framework_dir`, project-relative) into the upload set
    // so the remote worker has the SDK binaries. Without this, the static
    // archive (extension-less, usually >1 MB) is dropped by the default
    // binary-artifact exclusion and the worker links the no-SDK stub.
    let force_include_dirs: Vec<PathBuf> = config
        .google_auth
        .as_ref()
        .and_then(|ga| ga.framework_dir.as_deref())
        .map(|rel| project_dir.join(rel))
        .filter(|p| p.is_dir())
        .into_iter()
        .collect();
    let tarball = tarball::create_project_tarball_with_includes(
        &project_dir,
        &publish_excludes,
        &force_include_dirs,
        &publish_includes,
    )
    .context("Failed to create project tarball")?;

    let tarball_size = tarball.len();
    if let OutputFormat::Text = format {
        println!(
            " {} ({:.1} MB)",
            style("done").green(),
            tarball_size as f64 / 1_048_576.0
        );
    }

    // Upload to build server
    if let OutputFormat::Text = format {
        print!("  Uploading to build server...");
        std::io::stdout().flush().ok();
    }

    // Base64-encode tarball for safe transmission (perry hub uses text-based multipart parsing,
    // which corrupts raw binary. Base64 is pure ASCII and round-trips safely.)
    use base64::Engine;
    let tarball_b64 = base64::engine::general_purpose::STANDARD.encode(&tarball);

    let client = reqwest::Client::new();
    let mut form = multipart::Form::new()
        .text("manifest", serde_json::to_string(&manifest)?)
        .text("credentials", serde_json::to_string(&credentials)?)
        .text("tarball_b64", tarball_b64);

    // Add license_key to form for legacy auth (non-Bearer)
    if !use_bearer_auth {
        form = form.text("license_key", license_key.clone());
    }

    let mut req = client
        .post(format!("{server_url}/api/v1/build"))
        .multipart(form);

    // Add Bearer token for API-token auth
    if use_bearer_auth {
        req = req.header("Authorization", format!("Bearer {}", license_key));
    }

    let resp = req
        .send()
        .await
        .context("Failed to connect to build server")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();

        // Handle specific error codes with helpful messages
        if let Ok(err_json) = serde_json::from_str::<serde_json::Value>(&body) {
            if let Some(code) = err_json
                .get("error")
                .and_then(|e| e.get("code"))
                .and_then(|c| c.as_str())
            {
                match code {
                    "PUBLISH_LIMIT_REACHED" => {
                        let msg = err_json
                            .get("error")
                            .and_then(|e| e.get("message"))
                            .and_then(|m| m.as_str())
                            .unwrap_or("Monthly publish limit reached");
                        eprintln!();
                        eprintln!("  {} {}", style("!").yellow().bold(), msg);
                        eprintln!();
                        bail!("Publish limit reached");
                    }
                    "ACCOUNT_REQUIRED" => {
                        let msg = err_json
                            .get("error")
                            .and_then(|e| e.get("message"))
                            .and_then(|m| m.as_str())
                            .unwrap_or("Account required for multiple projects");
                        eprintln!();
                        eprintln!("  {} {}", style("!").yellow().bold(), msg);
                        eprintln!();
                        bail!("Account required");
                    }
                    _ => {}
                }
            }
        }

        bail!("Build server returned {status}: {body}");
    }

    let build_resp: BuildResponse = resp.json().await.context("Invalid build response")?;

    if let OutputFormat::Text = format {
        println!(" {}", style("done").green());
        println!("  Job ID:    {}", style(&build_resp.job_id).dim());
        println!("  Position:  {}", build_resp.position);
        println!();
    }

    // Connect WebSocket for progress
    // Hub returns either an absolute WS URL (ws://host:port) or a relative path
    let ws_url =
        if build_resp.ws_url.starts_with("ws://") || build_resp.ws_url.starts_with("wss://") {
            // Absolute URL from hub
            build_resp.ws_url.clone()
        } else if server_url.starts_with("https://") {
            format!(
                "wss://{}{}",
                &server_url["https://".len()..],
                build_resp.ws_url
            )
        } else {
            format!(
                "ws://{}{}",
                &server_url["http://".len()..],
                build_resp.ws_url
            )
        };

    let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .context("Failed to connect WebSocket")?;

    let (mut ws_write, mut read) = ws_stream.split();

    // Send subscribe message to identify as CLI client for this job
    use futures_util::SinkExt;
    ws_write
        .send(Message::Text(
            format!(r#"{{"type":"subscribe","job_id":"{}"}}"#, build_resp.job_id).into(),
        ))
        .await
        .context("Failed to send subscribe message")?;

    let pb = if let OutputFormat::Text = format {
        let pb = ProgressBar::new(100);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("  {spinner:.cyan} [{bar:30.cyan/dim}] {msg}")
                .unwrap()
                .progress_chars("━╸─"),
        );
        pb.set_message("Waiting for build...");
        Some(pb)
    } else {
        None
    };

    let mut download_url: Option<String> = None;
    let mut download_path: Option<String> = None;
    let mut artifact_name: Option<String> = None;
    let mut build_success = false;
    // `done` = a terminal Complete/Error was received. Until then, a dropped or
    // closed WebSocket (the hub drops connections while a job sits in the queue)
    // must RECONNECT + re-subscribe rather than silently end the publish — else
    // the command exits without ever downloading the artifact (#flaky-publish).
    let mut done = false;
    // `published` = the hub confirmed a server-side publish (TestFlight / App
    // Store etc.), where no local artifact is downloaded.
    let mut published = false;
    // `downloaded` = a local artifact was actually written to the output dir.
    let mut downloaded = false;
    let mut ws_retries = 0u32;
    let max_ws_retries = 60u32; // ~10 minutes with backoff

    use futures_util::StreamExt;

    // Reconnect to the hub and re-subscribe to the job. Used whenever the stream
    // errors, closes, or ends before a terminal message. Bails after exhausting
    // retries so CI fails loudly instead of going green with no artifact.
    macro_rules! reconnect_or_bail {
        ($why:expr) => {{
            loop {
                ws_retries += 1;
                if ws_retries > max_ws_retries {
                    if let Some(ref pb) = pb {
                        pb.abandon_with_message(format!(
                            "WebSocket {} — lost after {max_ws_retries} retries",
                            $why
                        ));
                    }
                    bail!(
                        "WebSocket {} and could not be re-established after {max_ws_retries} retries (no build result received)",
                        $why
                    );
                }
                let delay = std::cmp::min(ws_retries as u64 * 2, 30);
                if let OutputFormat::Text = format {
                    if let Some(ref pb) = pb {
                        pb.println(format!("    {} Connection lost ({}), reconnecting in {delay}s ({ws_retries}/{max_ws_retries})...", style("!").yellow(), $why));
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                match tokio_tungstenite::connect_async(&ws_url).await {
                    Ok((new_ws, _)) => {
                        let (mut new_write, new_read) = new_ws.split();
                        let _ = new_write
                            .send(Message::Text(
                                format!(
                                    r#"{{"type":"subscribe","job_id":"{}"}}"#,
                                    build_resp.job_id
                                )
                                .into(),
                            ))
                            .await;
                        read = new_read;
                        ws_retries = 0; // reset on successful reconnect
                        break;
                    }
                    // Keep retrying — don't bail on a single reconnect failure.
                    Err(_re) => continue,
                }
            }
        }};
    }

    'ws_loop: loop {
        loop {
            let msg = match read.next().await {
                Some(Ok(m)) => m,
                Some(Err(_e)) => {
                    reconnect_or_bail!("errored");
                    continue 'ws_loop;
                }
                None => {
                    // Stream ended. If we already have a terminal result, proceed
                    // to the download/finish step; otherwise the hub dropped us —
                    // reconnect rather than exit empty-handed.
                    if done {
                        break;
                    }
                    reconnect_or_bail!("stream ended");
                    continue 'ws_loop;
                }
            };

            let text = match msg {
                Message::Text(t) => t,
                Message::Close(_) => {
                    if done {
                        break;
                    }
                    reconnect_or_bail!("closed by server");
                    continue 'ws_loop;
                }
                _ => continue,
            };

            let server_msg: ServerMessage = match serde_json::from_str(&text) {
                Ok(m) => m,
                Err(_e) => {
                    // Unknown message type from hub, skip it
                    continue;
                }
            };

            match server_msg {
                ServerMessage::JobCreated { .. } => {
                    if let Some(ref pb) = pb {
                        pb.set_message("Build started");
                    }
                }
                ServerMessage::QueueUpdate { position, .. } => {
                    if let Some(ref pb) = pb {
                        pb.set_message(format!("Queue position: {position}"));
                    }
                }
                ServerMessage::Stage { stage, message } => {
                    if let Some(ref pb) = pb {
                        let icon = match stage.as_str() {
                            "extracting" => "📦",
                            "compiling" => "⚙️ ",
                            "generating_assets" => "🎨",
                            "bundling" => "📁",
                            "signing" => "🔏",
                            "notarizing" => "🍎",
                            "packaging" => "💿",
                            "uploading" => "☁️ ",
                            "verifying" => "🔍",
                            _ => "▶️ ",
                        };
                        pb.set_message(format!("{icon} {message}"));
                    }
                }
                ServerMessage::Log { line, stream, .. } => {
                    if let Some(ref pb) = pb {
                        if stream == "stderr" {
                            pb.println(format!("    {}", style(&line).dim()));
                        }
                    }
                }
                ServerMessage::Progress { percent, .. } => {
                    if let Some(ref pb) = pb {
                        pb.set_position(percent as u64);
                    }
                }
                ServerMessage::ArtifactReady {
                    artifact_name: name,
                    artifact_size,
                    sha256,
                    download_url: url,
                    download_path: dl_path,
                    ..
                } => {
                    if let Some(ref pb) = pb {
                        pb.set_position(100);
                        pb.finish_with_message(format!(
                            "{} Artifact ready: {} ({:.1} MB)",
                            style("✓").green().bold(),
                            name,
                            artifact_size as f64 / 1_048_576.0
                        ));
                    }
                    download_url = Some(url);
                    download_path = dl_path;
                    artifact_name = Some(name);

                    if let OutputFormat::Text = format {
                        println!("  SHA-256:   {}", style(&sha256).dim());
                    }
                }
                ServerMessage::Error {
                    code,
                    message,
                    stage: _,
                } => {
                    if let Some(ref pb) = pb {
                        pb.abandon_with_message(format!(
                            "{} {} ({})",
                            style("✗").red().bold(),
                            message,
                            code
                        ));
                    }
                    bail!("Build error [{}]: {}", code, message);
                }
                ServerMessage::Complete {
                    success,
                    duration_secs,
                    ..
                } => {
                    build_success = success;
                    done = true;
                    if let OutputFormat::Text = format {
                        println!();
                        if success {
                            println!(
                                "  {} Build completed in {:.1}s",
                                style("✓").green().bold(),
                                duration_secs
                            );
                        } else {
                            println!(
                                "  {} Build failed after {:.1}s",
                                style("✗").red().bold(),
                                duration_secs
                            );
                        }
                    }
                    break;
                }
                ServerMessage::Published {
                    platform, message, ..
                } => {
                    published = true;
                    if let OutputFormat::Text = format {
                        println!(
                            "  {} Published to {} — {}",
                            style("✓").green().bold(),
                            style(&platform).cyan(),
                            message
                        );
                    }
                }
                // #853: kept as a forward-compat safety net in case future
                // OutputFormat variants land. Current variants are exhausted.
                #[allow(unreachable_patterns)]
                _ => {}
            }
        }

        // Download artifact
        if build_success && !args.no_download {
            if let (Some(url), Some(name)) = (download_url, artifact_name) {
                if let OutputFormat::Text = format {
                    print!("  Downloading {name}...");
                    std::io::stdout().flush().ok();
                }

                fs::create_dir_all(&args.output)?;
                // `name` is supplied verbatim by the build server over the
                // WebSocket. Reduce it to a bare, traversal-free file name so a
                // malicious hub cannot write outside the output directory
                // (GHSA-x55v-q459-68ch).
                let safe_name = sanitize_artifact_name(&name)?;
                let dest = args.output.join(&safe_name);

                if let Some(ref src_path) = download_path {
                    // `download_path` is also server-controlled. A filesystem
                    // path is only meaningful when the hub shares this machine's
                    // filesystem; honoring it for a remote hub would let a
                    // malicious server copy out any file the user can read
                    // (GHSA-x55v-q459-68ch, Path B). Fall back to HTTP otherwise.
                    if !server_is_local(&server_url) {
                        bail!(
                            "Hub at {server_url} reported a local artifact path ({src_path}) but is not a local hub; refusing to read from an arbitrary local path"
                        );
                    }
                    // Local path available (self-hosted hub on this machine) - copy directly
                    fs::copy(src_path, &dest)
                        .with_context(|| format!("Failed to copy artifact from {src_path}"))?;
                } else {
                    // Remote hub - download via HTTP
                    let full_url = if url.starts_with("http://") || url.starts_with("https://") {
                        url.clone()
                    } else {
                        format!("{server_url}{url}")
                    };
                    let resp = client
                        .get(&full_url)
                        .send()
                        .await
                        .context("Failed to download artifact")?;

                    if !resp.status().is_success() {
                        bail!("Download failed: {}", resp.status());
                    }

                    let bytes = resp.bytes().await?;
                    // The hub may store artifacts as base64 (perry runtime doesn't
                    // decode Buffer.from(data, 'base64')). Detect and decode.
                    let data = if bytes.len() > 4
                        && bytes.iter().all(|&b| {
                            b.is_ascii_alphanumeric()
                                || b == b'+'
                                || b == b'/'
                                || b == b'='
                                || b == b'\n'
                                || b == b'\r'
                        }) {
                        use base64::Engine;
                        base64::engine::general_purpose::STANDARD
                            .decode(&bytes)
                            .unwrap_or_else(|_| bytes.to_vec())
                    } else {
                        bytes.to_vec()
                    };
                    fs::write(&dest, &data)?;
                }

                if let OutputFormat::Text = format {
                    println!(
                        " {} → {}",
                        style("done").green(),
                        style(dest.display()).bold()
                    );
                    println!();
                }

                if let OutputFormat::Text = format {
                    println!(
                        "  {} {}",
                        style("Ready!").green().bold(),
                        style(format!("Open with: open {}", dest.display())).dim()
                    );
                    println!();
                }

                downloaded = true;
            }
        }
        break; // terminal result handled above
    } // end 'ws_loop

    if !build_success {
        bail!("Build failed");
    }

    // The build reported success but we neither downloaded an artifact nor got a
    // server-side publish confirmation — almost always a hub connection that
    // dropped between the artifact notice and completion. Fail loudly so CI does
    // not go green with an empty release; re-running the publish recovers it.
    if !args.no_download && !downloaded && !published {
        bail!(
            "Build reported success but no artifact was received from the hub (connection likely interrupted). Re-run `perry publish`."
        );
    }

    Ok(())
}

/// Reduce a server-supplied artifact name to a single, traversal-free file
/// name. The build server controls this value over the WebSocket, so it must
/// never be able to escape the chosen output directory: absolute paths, `..`,
/// `.`, and embedded path separators are all rejected (GHSA-x55v-q459-68ch).
fn sanitize_artifact_name(name: &str) -> Result<String> {
    let trimmed = name.trim();
    let is_unsafe = trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || trimmed.contains('/')
        || trimmed.contains('\\')
        // Anything whose final path component is not exactly the input (drive
        // prefixes, embedded NULs, platform-specific separators, ...).
        || Path::new(trimmed).file_name().and_then(|s| s.to_str()) != Some(trimmed);

    if is_unsafe {
        bail!("Server sent an unsafe artifact name {name:?}; refusing to write outside the output directory");
    }
    Ok(trimmed.to_string())
}

/// Whether the resolved hub URL points at the local machine. Used to gate the
/// `download_path` local-copy shortcut, which trusts a server-controlled local
/// filesystem path (GHSA-x55v-q459-68ch, Path B).
fn server_is_local(server_url: &str) -> bool {
    match Url::parse(server_url) {
        Ok(u) => match u.host() {
            Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            None => false,
        },
        Err(_) => false,
    }
}

pub(crate) async fn auto_register_license(server_url: &str) -> Result<String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{server_url}/api/v1/license/register"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .context("Failed to register license")?;
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("License registration failed: {body}");
    }
    let reg: RegisterResponse = resp.json().await?;
    Ok(reg.license_key)
}
