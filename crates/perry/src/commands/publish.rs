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
use walkdir::WalkDir;

use crate::{OutputFormat, Platform};

#[derive(Args, Debug)]
pub struct PublishArgs {
    /// Target platform (macos, ios, android, linux)
    #[arg(value_enum)]
    pub platform: Option<Platform>,

    /// Build server URL
    #[arg(long, default_value = "https://hub.perryts.com")]
    pub server: Option<String>,

    /// License key (or set PERRY_LICENSE_KEY env)
    #[arg(long)]
    pub license_key: Option<String>,

    /// Apple Developer Team ID
    #[arg(long)]
    pub apple_team_id: Option<String>,

    /// Apple signing identity (e.g. "Developer ID Application: ..." or "Apple Distribution: ...")
    #[arg(long)]
    pub apple_identity: Option<String>,

    /// Path to App Store Connect .p8 key file
    #[arg(long)]
    pub apple_p8_key: Option<PathBuf>,

    /// App Store Connect API Key ID
    #[arg(long)]
    pub apple_key_id: Option<String>,

    /// App Store Connect Issuer ID
    #[arg(long)]
    pub apple_issuer_id: Option<String>,

    /// Path to Apple .p12 certificate bundle for code signing.
    /// The worker imports it into a temporary keychain per build.
    /// Saved path is remembered; password is never saved (use PERRY_APPLE_CERTIFICATE_PASSWORD).
    #[arg(long)]
    pub certificate: Option<PathBuf>,

    /// Path to iOS provisioning profile (.mobileprovision)
    #[arg(long)]
    pub provisioning_profile: Option<PathBuf>,

    /// Path to Android keystore (.jks/.keystore) for signing
    #[arg(long)]
    pub android_keystore: Option<PathBuf>,

    /// Android keystore password
    #[arg(long)]
    pub android_keystore_password: Option<String>,

    /// Android key alias within keystore
    #[arg(long)]
    pub android_key_alias: Option<String>,

    /// Android key password (defaults to keystore password)
    #[arg(long)]
    pub android_key_password: Option<String>,

    /// Path to Google Play service account JSON key file
    #[arg(long)]
    pub google_play_key: Option<PathBuf>,

    /// Project directory (default: current)
    #[arg(long, default_value = ".")]
    pub project: PathBuf,

    /// Don't download artifact, just build
    #[arg(long)]
    pub no_download: bool,

    /// Output directory for downloaded artifacts
    #[arg(short, long, default_value = "dist")]
    pub output: PathBuf,

    /// Skip security audit before building
    #[arg(long)]
    pub skip_audit: bool,

    /// Skip runtime verification after download
    #[arg(long)]
    pub skip_verify: bool,

    /// Minimum audit grade to proceed (A, B, C, D)
    #[arg(long, default_value = "C")]
    pub audit_fail_on: String,

    /// Verify service URL
    #[arg(long, default_value = "https://verify.perryts.com")]
    pub verify_url: String,

    /// Create a notarized DMG instead of uploading to App Store Connect (macOS only).
    /// Overrides the `distribute` setting in perry.toml.
    #[arg(long)]
    pub notarize: bool,

}

// --- Config types matching perry.toml ---

#[derive(Debug, Deserialize)]
struct PerryToml {
    project: Option<ProjectConfig>,
    app: Option<AppConfig>,
    macos: Option<MacosConfig>,
    ios: Option<IosConfig>,
    visionos: Option<VisionosConfig>,
    watchos: Option<WatchosConfig>,
    tvos: Option<TvosConfig>,
    android: Option<AndroidConfig>,
    linux: Option<LinuxConfig>,
    windows: Option<WindowsConfig>,
    build: Option<BuildConfig>,
    publish: Option<PublishConfig>,
    audit: Option<AuditConfig>,
    verify: Option<VerifyConfig>,
    release_notes: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
struct ProjectConfig {
    name: Option<String>,
    version: Option<String>,
    build_number: Option<u64>,
    bundle_id: Option<String>,
    description: Option<String>,
    entry: Option<String>,
    icons: Option<IconsConfig>,
    features: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct AppConfig {
    name: Option<String>,
    version: Option<String>,
    build_number: Option<u64>,
    bundle_id: Option<String>,
    description: Option<String>,
    entry: Option<String>,
    icons: Option<IconsConfig>,
}

#[derive(Debug, Deserialize)]
struct IconsConfig {
    source: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MacosConfig {
    bundle_id: Option<String>,
    category: Option<String>,
    minimum_os: Option<String>,
    entitlements: Option<Vec<String>>,
    /// "appstore", "notarize", or "both"
    distribute: Option<String>,
    signing_identity: Option<String>,
    // Per-project signing credentials (override global ~/.perry/config.toml)
    certificate: Option<String>,
    team_id: Option<String>,
    key_id: Option<String>,
    issuer_id: Option<String>,
    p8_key_path: Option<String>,
    /// If true, adds ITSAppUsesNonExemptEncryption=NO to Info.plist
    encryption_exempt: Option<bool>,
    /// For distribute = "both": separate Developer ID cert for notarization
    notarize_certificate: Option<String>,
    notarize_signing_identity: Option<String>,
    /// Separate .p12 for the Mac Installer Distribution cert (for .pkg signing)
    installer_certificate: Option<String>,
    /// Provisioning profile for App Store / TestFlight distribution
    provisioning_profile: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IosConfig {
    bundle_id: Option<String>,
    deployment_target: Option<String>,
    /// Alias for deployment_target (perry.toml uses minimum_version)
    minimum_version: Option<String>,
    device_family: Option<Vec<String>>,
    orientations: Option<Vec<String>>,
    capabilities: Option<Vec<String>>,
    distribute: Option<String>,
    entry: Option<String>,
    // Per-project signing credentials (override global ~/.perry/config.toml)
    provisioning_profile: Option<String>,
    certificate: Option<String>,
    signing_identity: Option<String>,
    team_id: Option<String>,
    key_id: Option<String>,
    issuer_id: Option<String>,
    p8_key_path: Option<String>,
    /// If true, adds ITSAppUsesNonExemptEncryption=NO to Info.plist
    /// (skips the export compliance prompt in App Store Connect)
    encryption_exempt: Option<bool>,
    /// Custom Info.plist entries (key-value pairs added to the generated plist).
    /// Use for privacy descriptions, custom URL schemes, etc.
    /// Example: { NSMicrophoneUsageDescription = "Measures ambient sound levels" }
    info_plist: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
struct VisionosConfig {
    bundle_id: Option<String>,
    deployment_target: Option<String>,
    minimum_version: Option<String>,
    distribute: Option<String>,
    entry: Option<String>,
    provisioning_profile: Option<String>,
    certificate: Option<String>,
    signing_identity: Option<String>,
    team_id: Option<String>,
    key_id: Option<String>,
    issuer_id: Option<String>,
    p8_key_path: Option<String>,
    encryption_exempt: Option<bool>,
    info_plist: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
struct AndroidConfig {
    package_name: Option<String>,
    min_sdk: Option<String>,
    target_sdk: Option<String>,
    permissions: Option<Vec<String>>,
    distribute: Option<String>,
    keystore: Option<String>,
    key_alias: Option<String>,
    google_play_key: Option<String>,
    entry: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WatchosConfig {
    bundle_id: Option<String>,
    deployment_target: Option<String>,
    encryption_exempt: Option<bool>,
    info_plist: Option<std::collections::HashMap<String, String>>,
    team_id: Option<String>,
    signing_identity: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TvosConfig {
    bundle_id: Option<String>,
    entry: Option<String>,
    deployment_target: Option<String>,
    encryption_exempt: Option<bool>,
    info_plist: Option<std::collections::HashMap<String, String>>,
    team_id: Option<String>,
    signing_identity: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LinuxConfig {
    format: Option<String>,
    category: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WindowsConfig {
    /// Google Cloud KMS key path for code signing
    /// e.g. "projects/skelpo/locations/europe-west3/keyRings/code-signing-eu/cryptoKeys/skelpo-codesign/cryptoKeyVersions/1"
    gcloud_kms_key: Option<String>,
    /// Path to the code signing certificate (.crt)
    gcloud_kms_cert: Option<String>,
    /// Path to GCP service account JSON key file
    gcloud_service_account: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BuildConfig {
    out_dir: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PublishConfig {
    server: Option<String>,
    /// Extra directories to exclude from the upload tarball (e.g. ["screenshots", "docs"])
    exclude: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct AuditConfig {
    fail_on: Option<String>,
    ignore: Option<Vec<String>>,
    severity: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VerifyConfig {
    url: Option<String>,
}

// --- Server API types ---

#[derive(Debug, Deserialize)]
struct RegisterResponse {
    license_key: String,
    tier: String,
    platforms: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct BuildResponse {
    job_id: String,
    ws_url: String,
    position: usize,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerMessage {
    JobCreated {
        job_id: String,
        position: usize,
        estimated_wait_secs: Option<u64>,
    },
    QueueUpdate {
        position: usize,
        estimated_wait_secs: Option<u64>,
    },
    Stage {
        stage: String,
        message: String,
    },
    Log {
        stage: String,
        line: String,
        stream: String,
    },
    Progress {
        stage: String,
        percent: u8,
        message: Option<String>,
    },
    ArtifactReady {
        artifact_name: String,
        artifact_size: u64,
        sha256: String,
        download_url: String,
        expires_in_secs: u64,
        #[serde(default)]
        download_path: Option<String>,
    },
    Published {
        platform: String,
        message: String,
        url: Option<String>,
    },
    Error {
        code: String,
        message: String,
        stage: Option<String>,
    },
    Complete {
        job_id: String,
        success: bool,
        duration_secs: f64,
        artifacts: Vec<serde_json::Value>,
    },
}

// --- Manifest sent to the build server ---

#[derive(Debug, Serialize)]
struct BuildManifest {
    app_name: String,
    bundle_id: String,
    version: String,
    short_version: Option<String>,
    entry: String,
    icon: Option<String>,
    targets: Vec<String>,
    category: Option<String>,
    minimum_os_version: Option<String>,
    entitlements: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ios_deployment_target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ios_device_family: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ios_orientations: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ios_capabilities: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ios_distribute: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ios_encryption_exempt: Option<bool>,
    /// Custom Info.plist entries for iOS (e.g. NSMicrophoneUsageDescription)
    #[serde(skip_serializing_if = "Option::is_none")]
    ios_info_plist: Option<std::collections::HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    visionos_deployment_target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    visionos_distribute: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    visionos_encryption_exempt: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    visionos_info_plist: Option<std::collections::HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    macos_distribute: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    macos_encryption_exempt: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tvos_deployment_target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tvos_encryption_exempt: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tvos_info_plist: Option<std::collections::HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    android_min_sdk: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    android_target_sdk: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    android_permissions: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    android_distribute: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    linux_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    linux_category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    linux_description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    release_notes: Option<std::collections::HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    features: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
struct CredentialsPayload {
    apple_team_id: Option<String>,
    apple_signing_identity: Option<String>,
    apple_key_id: Option<String>,
    apple_issuer_id: Option<String>,
    apple_p8_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provisioning_profile_base64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    apple_certificate_p12_base64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    apple_certificate_password: Option<String>,
    /// For macOS distribute = "both": separate Developer ID cert for notarization
    #[serde(skip_serializing_if = "Option::is_none")]
    apple_notarize_certificate_p12_base64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    apple_notarize_certificate_password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    apple_notarize_signing_identity: Option<String>,
    /// Separate .p12 for the Mac Installer Distribution cert (for .pkg signing)
    #[serde(skip_serializing_if = "Option::is_none")]
    apple_installer_certificate_p12_base64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    apple_installer_certificate_password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    android_keystore_base64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    android_keystore_password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    android_key_alias: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    android_key_password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    google_play_service_account_json: Option<String>,
    /// Google Cloud KMS key path for Windows code signing
    #[serde(skip_serializing_if = "Option::is_none")]
    gcloud_kms_key: Option<String>,
    /// Base64-encoded code signing certificate for GCloud KMS
    #[serde(skip_serializing_if = "Option::is_none")]
    gcloud_kms_cert_base64: Option<String>,
    /// Base64-encoded GCP service account JSON key
    #[serde(skip_serializing_if = "Option::is_none")]
    gcloud_service_account_base64: Option<String>,
}

pub fn run(args: PublishArgs, format: OutputFormat, use_color: bool, _verbose: u8) -> Result<()> {
    if !check_beta_consent("publish") {
        bail!("Aborted.");
    }

    let target_hint = match args.platform {
        Some(Platform::Ios) => Some("ios"),
        Some(Platform::Visionos) => Some("visionos"),
        Some(Platform::Android) => Some("android"),
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
        let content = fs::read_to_string(&perry_toml_path)
            .context("Failed to read perry.toml")?;
        toml::from_str(&content)
            .context("Failed to parse perry.toml")?
    } else {
        bail!(
            "No perry.toml found in {}. Run 'perry init' first.",
            project_dir.display()
        );
    };

    // --- Integration: Security Audit ---
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
            Some(Platform::Ios) | Some(Platform::Visionos) | Some(Platform::Android) | Some(Platform::Macos) | Some(Platform::Tvos) | Some(Platform::Web) | Some(Platform::Windows) => "gui",
            _ => "server",
        };

        match super::audit::run_audit_check(
            &project_dir,
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
                bail!(
                    "{}\n  Use {} to bypass.",
                    e,
                    style("--skip-audit").yellow()
                );
            }
        }
    }

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
        bail!("No target specified. Use: perry publish <macos|ios|visionos|tvos|android|linux|windows|web>");
    };

    let target_display = match target_name.as_str() {
        "ios" => "iOS",
        "visionos" => "visionOS",
        "tvos" => "tvOS",
        "android" => "Android",
        "linux" => "Linux",
        "windows" => "Windows",
        "web" => "Web",
        _ => "macOS",
    };
    let is_ios = target_name == "ios";
    let is_visionos = target_name == "visionos";
    let is_tvos = target_name == "tvos";
    let is_android = target_name == "android";
    let is_linux = target_name == "linux";

    // --- Resolve server URL ---
    let server_url = args
        .server
        .clone()
        .or_else(|| saved.server.clone())
        .or_else(|| config.publish.as_ref().and_then(|p| p.server.clone()))
        .unwrap_or_else(|| "https://hub.perryts.com".into());

    // --- Resolve entry point ---
    let entry = if is_android {
        config.android.as_ref().and_then(|a| a.entry.clone())
            .or_else(|| config.app.as_ref().and_then(|a| a.entry.clone()))
            .or_else(|| config.project.as_ref().and_then(|p| p.entry.clone()))
            .unwrap_or_else(|| "src/main.ts".into())
    } else if is_ios {
        config.ios.as_ref().and_then(|i| i.entry.clone())
            .or_else(|| config.app.as_ref().and_then(|a| a.entry.clone()))
            .or_else(|| config.project.as_ref().and_then(|p| p.entry.clone()))
            .unwrap_or_else(|| "src/main_ios.ts".into())
    } else if is_visionos {
        config.visionos.as_ref().and_then(|i| i.entry.clone())
            .or_else(|| config.app.as_ref().and_then(|a| a.entry.clone()))
            .or_else(|| config.project.as_ref().and_then(|p| p.entry.clone()))
            .unwrap_or_else(|| "src/main_visionos.ts".into())
    } else if is_tvos {
        config.tvos.as_ref().and_then(|t| t.entry.clone())
            .or_else(|| config.app.as_ref().and_then(|a| a.entry.clone()))
            .or_else(|| config.project.as_ref().and_then(|p| p.entry.clone()))
            .unwrap_or_else(|| "src/main_tvos.ts".into())
    } else {
        config.app.as_ref().and_then(|a| a.entry.clone())
            .or_else(|| config.project.as_ref().and_then(|p| p.entry.clone()))
            .unwrap_or_else(|| "src/main.ts".into())
    };

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
    let is_macos = !is_ios && !is_visionos && !is_tvos && !is_android && !is_linux && !is_windows && !is_web;
    let macos_needs_upload = is_macos && matches!(
        macos_distribute.as_deref(),
        Some("appstore") | Some("both")
    );
    let build_number = if is_ios || is_visionos || is_tvos || is_android || macos_needs_upload {
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
    let bundle_id = if is_android {
        config.android.as_ref().and_then(|a| a.package_name.clone())
            .or_else(|| config.ios.as_ref().and_then(|i| i.bundle_id.clone()))
            .or_else(|| config.macos.as_ref().and_then(|m| m.bundle_id.clone()))
            .or_else(|| app_bundle_id.clone())
            .or_else(|| project_bundle_id.clone())
            .unwrap_or_else(|| format!("com.perry.{}", app_name.to_lowercase().replace(' ', "-")))
    } else if is_ios {
        config.ios.as_ref().and_then(|i| i.bundle_id.clone())
            .or_else(|| app_bundle_id.clone())
            .or_else(|| project_bundle_id.clone())
            .or_else(|| config.macos.as_ref().and_then(|m| m.bundle_id.clone()))
            .unwrap_or_else(|| format!("com.perry.{}", app_name.to_lowercase().replace(' ', "-")))
    } else if is_visionos {
        config.visionos.as_ref().and_then(|i| i.bundle_id.clone())
            .or_else(|| app_bundle_id.clone())
            .or_else(|| project_bundle_id.clone())
            .or_else(|| config.ios.as_ref().and_then(|i| i.bundle_id.clone()))
            .or_else(|| config.macos.as_ref().and_then(|m| m.bundle_id.clone()))
            .unwrap_or_else(|| format!("com.perry.{}", app_name.to_lowercase().replace(' ', "-")))
    } else if is_tvos {
        config.tvos.as_ref().and_then(|t| t.bundle_id.clone())
            .or_else(|| app_bundle_id.clone())
            .or_else(|| project_bundle_id.clone())
            .or_else(|| config.ios.as_ref().and_then(|i| i.bundle_id.clone()))
            .unwrap_or_else(|| format!("com.perry.{}", app_name.to_lowercase().replace(' ', "-")))
    } else {
        config.macos.as_ref().and_then(|m| m.bundle_id.clone())
            .or_else(|| app_bundle_id.clone())
            .or_else(|| project_bundle_id.clone())
            .unwrap_or_else(|| format!("com.perry.{}", app_name.to_lowercase().replace(' ', "-")))
    };

    let mut icon = config.project.as_ref().and_then(|p| p.icons.as_ref()).and_then(|i| i.source.clone())
        .or_else(|| config.app.as_ref().and_then(|a| a.icons.as_ref()).and_then(|i| i.source.clone()));

    // Prompt for icon if missing and building for a platform that needs one
    let needs_icon = is_ios || is_visionos || is_android
        || (is_macos && matches!(macos_distribute.as_deref(), Some("appstore") | Some("both")));
    if icon.is_none() && interactive && needs_icon {
        println!();
        println!("  {} No app icon configured.", style("!").yellow().bold());
        println!("  App Store requires a 1024\u{00d7}1024 PNG icon.");
        if let Some(path_str) = prompt_input(
            "  Path to icon image (or Enter to skip)",
            None,
        ) {
            let src_path = Path::new(&path_str);
            if !src_path.exists() {
                println!("  {} Icon file not found: {}", style("!").yellow(), path_str);
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
                    println!("  {} Icon saved to {} and perry.toml updated.",
                        style("✓").green().bold(), icon_rel);
                }
            }
        }
    }

    let category = config.macos.as_ref().and_then(|m| m.category.clone());
    let minimum_os = config.macos.as_ref().and_then(|m| m.minimum_os.clone());
    let entitlements = config.macos.as_ref().and_then(|m| m.entitlements.clone());
    // macos_distribute already extracted above (before build_number auto-increment)
    let macos_signing_identity = if args.notarize {
        config.macos.as_ref()
            .and_then(|m| m.notarize_signing_identity.clone())
            .or_else(|| config.macos.as_ref().and_then(|m| m.signing_identity.clone()))
    } else {
        config.macos.as_ref().and_then(|m| m.signing_identity.clone())
    };

    // iOS-specific config from perry.toml
    let ios_deployment_target = config.ios.as_ref().and_then(|i| {
        i.deployment_target.clone().or_else(|| i.minimum_version.clone())
    });
    let ios_device_family = config.ios.as_ref().and_then(|i| i.device_family.clone());
    let ios_orientations = config.ios.as_ref().and_then(|i| i.orientations.clone());
    let ios_capabilities = config.ios.as_ref().and_then(|i| i.capabilities.clone());
    let mut ios_distribute = config.ios.as_ref().and_then(|i| i.distribute.clone());
    let ios_encryption_exempt = config.ios.as_ref().and_then(|i| i.encryption_exempt);
    let ios_info_plist = config.ios.as_ref().and_then(|i| i.info_plist.clone());
    let visionos_deployment_target = config.visionos.as_ref().and_then(|i| {
        i.deployment_target.clone().or_else(|| i.minimum_version.clone())
    });
    let visionos_distribute = config.visionos.as_ref().and_then(|i| i.distribute.clone());
    let visionos_encryption_exempt = config.visionos.as_ref().and_then(|i| i.encryption_exempt);
    let visionos_info_plist = config.visionos.as_ref().and_then(|i| i.info_plist.clone());
    let macos_encryption_exempt = config.macos.as_ref().and_then(|m| m.encryption_exempt);

    // Android-specific config from perry.toml
    let android_min_sdk = config.android.as_ref().and_then(|a| a.min_sdk.clone());
    let android_target_sdk = config.android.as_ref().and_then(|a| a.target_sdk.clone());
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
                    println!("  {} License: {}", style("✓").green().bold(), style(&key).bold());
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
            config.ios.as_ref().and_then(|i| i.certificate.as_deref()).is_some()
        } else {
            config.macos.as_ref().and_then(|m| m.certificate.as_deref()).is_some()
        };
        let has_apple_config = args.certificate.is_some()
            || std::env::var("PERRY_APPLE_CERTIFICATE").is_ok()
            || has_platform_cert;
        if !has_apple_config {
            let platform = if is_ios { "iOS" } else { "macOS" };
            println!();
            println!("  {} {platform} not configured — running setup wizard", style("!").yellow());
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
        config.macos.as_ref()
            .and_then(|m| m.notarize_signing_identity.clone())
            .or_else(|| config.macos.as_ref().and_then(|m| m.signing_identity.clone()))
    } else {
        config.macos.as_ref().and_then(|m| m.signing_identity.clone())
    };
    let toml_certificate = if is_ios {
        config.ios.as_ref().and_then(|i| i.certificate.clone())
    } else if args.notarize {
        // --notarize: use the Developer ID cert for notarization
        config.macos.as_ref()
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
        config.ios.as_ref().and_then(|i| i.provisioning_profile.clone())
    } else if is_macos {
        config.macos.as_ref().and_then(|m| m.provisioning_profile.clone())
    } else {
        None
    };

    // Apple credentials (for macOS and iOS)
    let apple_team_id = if !is_android {
        resolve_credential(
            args.apple_team_id.as_deref(),
            "PERRY_APPLE_TEAM_ID",
            toml_team_id.as_deref()
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
        macos_signing_identity.clone().or_else(|| apple_identity_base.clone())
    } else {
        apple_identity_base.clone()
    };

    let apple_p8_key_path = if !is_android {
        resolve_path_credential(
            args.apple_p8_key.as_deref(),
            "PERRY_APPLE_P8_KEY",
            toml_p8_key_path.as_deref()
                .or_else(|| saved.apple.as_ref().and_then(|a| a.p8_key_path.as_deref())),
            "  App Store Connect .p8 key path",
            interactive,
        )
    } else {
        args.apple_p8_key.as_ref().map(|p| p.to_string_lossy().to_string())
    };

    // .p12 certificate for code signing (path saved, password never saved)
    // Priority: CLI → env → perry.toml → ~/.perry/config.toml → auto-export from Keychain → skip
    let (apple_certificate_path, auto_exported_p12) = if !is_android && !is_linux {
        // Check explicit path first (CLI flag, env var, perry.toml, or saved config)
        let explicit_path = resolve_path_credential(
            args.certificate.as_deref(),
            "PERRY_APPLE_CERTIFICATE",
            toml_certificate.as_deref(),
            "", // empty prompt — don't prompt, we'll try auto-export instead
            false, // never prompt for path
        );
        if explicit_path.is_some() {
            (explicit_path, None)
        } else {
            // Try auto-export from Keychain
            let auto = auto_export_p12_from_keychain(
                apple_identity.as_deref(),
                interactive,
            );
            (None, auto)
        }
    } else {
        (None, None)
    };
    let apple_certificate_password = if apple_certificate_path.is_some() {
        // Explicit .p12 file — need password
        // Check for auto-generated cert (lives in ~/.perry/) — use known password
        let is_auto_generated = apple_certificate_path.as_deref()
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
            toml_key_id.as_deref()
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
            toml_issuer_id.as_deref()
                .or_else(|| saved.apple.as_ref().and_then(|a| a.issuer_id.as_deref())),
            "  App Store Connect Issuer ID",
            false,
            interactive,
        )
    } else {
        args.apple_issuer_id.clone()
    };

    // Provisioning profile (iOS always, macOS when distributing to App Store)
    let macos_needs_profile = is_macos && matches!(
        macos_distribute.as_deref(), Some("appstore") | Some("both") | Some("testflight")
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
        args.provisioning_profile.as_ref().map(|p| p.to_string_lossy().to_string())
    };

    // Auto-trigger Android setup if not configured
    if is_android && interactive {
        let has_keystore = args.android_keystore.is_some()
            || std::env::var("PERRY_ANDROID_KEYSTORE").is_ok()
            || saved.android.as_ref().and_then(|a| a.keystore_path.as_deref()).is_some()
            || config.android.as_ref().and_then(|a| a.keystore.as_deref()).is_some();
        if !has_keystore {
            println!();
            println!("  {} Android not configured — running setup wizard", style("!").yellow());
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
            saved.android.as_ref().and_then(|a| a.keystore_path.as_deref()).or(toml_android_keystore),
            "  Android keystore path",
            interactive,
        )
    } else {
        args.android_keystore.as_ref().map(|p| p.to_string_lossy().to_string())
    };

    let android_key_alias = if is_android {
        resolve_credential(
            args.android_key_alias.as_deref(),
            "PERRY_ANDROID_KEY_ALIAS",
            saved.android.as_ref().and_then(|a| a.key_alias.as_deref()).or(toml_android_key_alias),
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
    let android_keystore_password = if android_keystore_password.is_none() && is_android && android_keystore_path.is_some() && interactive {
        prompt_input("  Android keystore password", None)
    } else {
        android_keystore_password
    };

    let android_key_password = args
        .android_key_password
        .clone()
        .or_else(|| std::env::var("PERRY_ANDROID_KEY_PASSWORD").ok());

    // Google Play service account JSON
    let toml_google_play_key = config.android.as_ref().and_then(|a| a.google_play_key.as_deref());
    let google_play_key_path = if is_android {
        resolve_path_credential(
            args.google_play_key.as_deref(),
            "PERRY_GOOGLE_PLAY_KEY_PATH",
            saved.android.as_ref().and_then(|a| a.google_play_key_path.as_deref()).or(toml_google_play_key),
            "  Google Play service account JSON path",
            interactive && android_distribute.as_deref() == Some("playstore"),
        )
    } else {
        args.google_play_key.as_ref().map(|p| p.to_string_lossy().to_string())
    };

    // Read file contents for credentials that need to be sent as content
    let p8_key_content = if let Some(ref path_str) = apple_p8_key_path {
        let path = Path::new(path_str);
        if path.exists() {
            Some(fs::read_to_string(path)
                .with_context(|| format!("Failed to read .p8 key from {path_str}"))?)
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

    let (apple_certificate_p12_b64, apple_certificate_password) = if let Some((b64, pass)) = auto_exported_p12 {
        // Auto-exported from Keychain — data and password already available
        (Some(b64), Some(pass))
    } else if let Some(ref path_str) = apple_certificate_path {
        let path = Path::new(path_str);
        if path.exists() {
            use base64::Engine;
            let data = fs::read(path)
                .with_context(|| format!("Failed to read .p12 certificate: {path_str}"))?;
            (Some(base64::engine::general_purpose::STANDARD.encode(&data)), apple_certificate_password)
        } else {
            (None, apple_certificate_password)
        }
    } else {
        (None, None)
    };

    // For macOS distribute = "both": resolve the separate Developer ID cert for notarization
    let (notarize_cert_b64, notarize_cert_password, notarize_identity) = if is_macos
        && macos_distribute.as_deref() == Some("both")
    {
        let notarize_cert_path = config.macos.as_ref().and_then(|m| m.notarize_certificate.clone());
        let notarize_identity = config.macos.as_ref().and_then(|m| m.notarize_signing_identity.clone());
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
        let is_auto_generated_notarize = notarize_cert_path.as_deref()
            .map(|p| p.contains("/.perry/"))
            .unwrap_or(false);
        let password = std::env::var("PERRY_APPLE_NOTARIZE_CERTIFICATE_PASSWORD").ok()
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
        let installer_cert_path = config.macos.as_ref().and_then(|m| m.installer_certificate.clone());
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
        let is_auto_generated = installer_cert_path.as_deref()
            .map(|p| p.contains("/.perry/"))
            .unwrap_or(false);
        let password = std::env::var("PERRY_APPLE_INSTALLER_CERTIFICATE_PASSWORD").ok()
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
            Some(fs::read_to_string(path)
                .with_context(|| format!("Failed to read Google Play key: {path_str}"))?)
        } else {
            None
        }
    } else {
        None
    };

    // Pre-flight credential validation — fail fast before building the tarball
    {
        let is_macos = !is_android && !is_ios && !is_linux && !is_windows && !is_web;
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
        )?;
    }

    // Pre-flight validation for iOS App Store / TestFlight — detect common rejection reasons
    if is_ios {
        let distribute = ios_distribute.as_deref().unwrap_or("");
        if distribute == "appstore" || distribute == "testflight" {
            let mut warnings: Vec<String> = Vec::new();
            let mut errors: Vec<String> = Vec::new();

            // 1. Validate provisioning profile bundle ID matches project bundle_id
            if let Some(ref profile_path) = provisioning_profile_path {
                let profile_data = fs::read(profile_path)
                    .with_context(|| format!("Failed to read provisioning profile: {profile_path}"))?;
                let data_str = String::from_utf8_lossy(&profile_data);
                if let (Some(xml_start), Some(xml_end)) = (data_str.find("<?xml"), data_str.find("</plist>")) {
                    let plist_xml = &data_str[xml_start..xml_end + "</plist>".len()];

                    // Extract application-identifier from Entitlements
                    if let Some(app_id_pos) = plist_xml.find("<key>application-identifier</key>") {
                        let after_key = &plist_xml[app_id_pos + "<key>application-identifier</key>".len()..];
                        if let Some(s_start) = after_key.find("<string>") {
                            if let Some(s_end) = after_key.find("</string>") {
                                let app_identifier = &after_key[s_start + "<string>".len()..s_end];
                                // application-identifier is "TEAMID.bundle.id" — strip team prefix
                                let profile_bundle_id = if let Some(dot_pos) = app_identifier.find('.') {
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
                                let expiry_date = if expiry_str.len() >= 10 { &expiry_str[..10] } else { expiry_str };
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
                    if let Some(ref expected_team) = apple_team_id {
                        if let Some(team_pos) = plist_xml.find("<key>TeamIdentifier</key>") {
                            let after_key = &plist_xml[team_pos + "<key>TeamIdentifier</key>".len()..];
                            if let Some(s_start) = after_key.find("<string>") {
                                if let Some(s_end) = after_key.find("</string>") {
                                    let profile_team = &after_key[s_start + "<string>".len()..s_end];
                                    if profile_team != expected_team.as_str() {
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
                    .into()
                );
            } else if let Some(ref icon_path) = icon {
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
            if version_parts.len() < 2 || version_parts.len() > 3
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
                errors.push(
                    "Build number must be positive for App Store submission.".into()
                );
            }

            // 5. Check signing certificate is provided
            if apple_certificate_p12_b64.is_none() {
                errors.push(
                    "No distribution certificate (.p12) provided. Required for App Store signing.\n\
                     \x20\x20  Add certificate to [ios] in perry.toml or pass --certificate\n\
                     \x20\x20  Run `perry setup ios` to configure automatically"
                    .into()
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
    }

    // Pre-flight validation for macOS App Store / Both
    if is_macos {
        let distribute = macos_distribute.as_deref().unwrap_or("");
        if matches!(distribute, "appstore" | "both") {
            let mut warnings: Vec<String> = Vec::new();
            let mut errors: Vec<String> = Vec::new();

            // 1. Check for app icon (required for App Store)
            if icon.is_none() {
                errors.push(
                    "No app icon configured. App Store requires a 1024×1024 icon.\n\
                     \x20\x20  Add to [project] in perry.toml:\n\
                     \x20\x20  icons = { source = \"assets/icon.png\" }"
                    .into()
                );
            } else if let Some(ref icon_path) = icon {
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
            if version_parts.len() < 2 || version_parts.len() > 3
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
                errors.push(
                    "Build number must be positive for App Store submission.".into()
                );
            }

            // 4. Check signing certificate
            if apple_certificate_p12_b64.is_none() {
                errors.push(
                    "No distribution certificate (.p12) provided. Required for App Store signing.\n\
                     \x20\x20  Add certificate to [macos] in perry.toml or pass --certificate\n\
                     \x20\x20  Run `perry setup macos` to configure automatically"
                    .into()
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
    }

    // --- Show summary and confirm ---
    if let OutputFormat::Text = format {
        println!("  Version:   {version}");
        println!("  Bundle ID: {bundle_id}");
        println!("  Target:    {target_display}");
        println!("  Server:    {server_url}");
        if is_windows {
            if config.windows.as_ref().and_then(|w| w.gcloud_kms_key.as_ref()).is_some() {
                println!("  Signing:   Google Cloud KMS (EV code signing)");
            }
        } else if is_ios || is_macos {
            if let Some(ref id) = apple_identity {
                println!("  Signing:   {id}");
            }
        }
        if is_android && android_distribute.as_deref() == Some("playstore") {
            println!("  Distribute: Google Play");
        } else if is_ios && matches!(ios_distribute.as_deref(), Some("appstore") | Some("testflight")) {
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
        if apple_team_id.is_some() { apple.team_id = apple_team_id.clone(); }
        if apple_p8_key_path.is_some() { apple.p8_key_path = apple_p8_key_path.clone(); }
        if apple_key_id.is_some() { apple.key_id = apple_key_id.clone(); }
        if apple_issuer_id.is_some() { apple.issuer_id = apple_issuer_id.clone(); }
        // Project-specific fields (signing_identity, certificate, provisioning_profile)
        // are NOT saved to global config — they belong in perry.toml [ios]/[macos]
    }
    if is_android {
        let android_saved = saved.android.get_or_insert_with(AndroidSavedConfig::default);
        if android_keystore_path.is_some() { android_saved.keystore_path = android_keystore_path.clone(); }
        if android_key_alias.is_some() { android_saved.key_alias = android_key_alias.clone(); }
        if google_play_key_path.is_some() { android_saved.google_play_key_path = google_play_key_path.clone(); }
    }
    if let Err(e) = save_config(&saved) {
        if let OutputFormat::Text = format {
            println!("  {} Could not save config: {e}", style("!").yellow());
        }
    } else if interactive {
        println!("  Saved settings to {}", style(config_path().display()).dim());
        println!();
    }

    // Build manifest
    // For iOS/Android/macOS-appstore: version = build_number (CFBundleVersion), short_version = marketing version
    // For macOS-notarize/Linux: version = marketing version string, short_version = None
    let (manifest_version, manifest_short_version) = if is_ios || is_visionos || is_android || macos_needs_upload {
        (build_number.to_string(), Some(version.clone()))
    } else {
        (version.clone(), None)
    };
    let manifest = BuildManifest {
        app_name: app_name.clone(),
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
        visionos_deployment_target: if is_visionos { visionos_deployment_target } else { None },
        visionos_distribute: if is_visionos { visionos_distribute } else { None },
        visionos_encryption_exempt: if is_visionos { visionos_encryption_exempt } else { None },
        visionos_info_plist: if is_visionos { visionos_info_plist } else { None },
        macos_distribute: if is_macos { macos_distribute } else { None },
        macos_encryption_exempt: if is_macos { macos_encryption_exempt } else { None },
        tvos_deployment_target: if is_tvos { config.tvos.as_ref().and_then(|t| t.deployment_target.clone()) } else { None },
        tvos_encryption_exempt: if is_tvos { config.tvos.as_ref().and_then(|t| t.encryption_exempt) } else { None },
        tvos_info_plist: if is_tvos { config.tvos.as_ref().and_then(|t| t.info_plist.clone()) } else { None },
        android_min_sdk: if is_android { android_min_sdk } else { None },
        android_target_sdk: if is_android { android_target_sdk } else { None },
        android_permissions: if is_android { android_permissions } else { None },
        android_distribute: if is_android { android_distribute } else { None },
        linux_format: if is_linux { config.linux.as_ref().and_then(|l| l.format.clone()) } else { None },
        linux_category: if is_linux { config.linux.as_ref().and_then(|l| l.category.clone()) } else { None },
        linux_description: if is_linux {
            config.linux.as_ref().and_then(|l| l.description.clone())
                .or_else(|| config.app.as_ref().and_then(|a| a.description.clone()))
        } else { None },
        release_notes: config.release_notes.clone(),
        features: config.project.as_ref().and_then(|p| p.features.clone()),
    };

    // Read GCloud KMS signing credentials for Windows
    let (gcloud_kms_key, gcloud_kms_cert_b64, gcloud_sa_b64) = if is_windows {
        let win = config.windows.as_ref();
        let kms_key = win.and_then(|w| w.gcloud_kms_key.clone());
        let cert_b64 = win.and_then(|w| w.gcloud_kms_cert.as_ref()).and_then(|path_str| {
            let path = if path_str.starts_with("~/") {
                dirs::home_dir().unwrap_or_default().join(&path_str[2..])
            } else {
                std::path::PathBuf::from(path_str)
            };
            if path.exists() {
                use base64::Engine;
                fs::read(&path).ok().map(|data| base64::engine::general_purpose::STANDARD.encode(&data))
            } else {
                None
            }
        });
        let sa_b64 = win.and_then(|w| w.gcloud_service_account.as_ref()).and_then(|path_str| {
            let path = if path_str.starts_with("~/") {
                dirs::home_dir().unwrap_or_default().join(&path_str[2..])
            } else {
                std::path::PathBuf::from(path_str)
            };
            if path.exists() {
                use base64::Engine;
                fs::read(&path).ok().map(|data| base64::engine::general_purpose::STANDARD.encode(&data))
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

    let publish_excludes = config.publish.as_ref()
        .and_then(|p| p.exclude.clone())
        .unwrap_or_default();
    let tarball = create_project_tarball_with_excludes(&project_dir, &publish_excludes)
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
            if let Some(code) = err_json.get("error").and_then(|e| e.get("code")).and_then(|c| c.as_str()) {
                match code {
                    "PUBLISH_LIMIT_REACHED" => {
                        let msg = err_json.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()).unwrap_or("Monthly publish limit reached");
                        eprintln!();
                        eprintln!("  {} {}", style("!").yellow().bold(), msg);
                        eprintln!();
                        bail!("Publish limit reached");
                    }
                    "ACCOUNT_REQUIRED" => {
                        let msg = err_json.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()).unwrap_or("Account required for multiple projects");
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
        println!(
            "  Job ID:    {}",
            style(&build_resp.job_id).dim()
        );
        println!(
            "  Position:  {}",
            build_resp.position
        );
        println!();
    }

    // Connect WebSocket for progress
    // Hub returns either an absolute WS URL (ws://host:port) or a relative path
    let ws_url = if build_resp.ws_url.starts_with("ws://") || build_resp.ws_url.starts_with("wss://") {
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
    let mut ws_retries = 0u32;
    let max_ws_retries = 60u32; // ~10 minutes with backoff

    use futures_util::StreamExt;
    'ws_loop: loop {
    while let Some(msg) = read.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                // WebSocket dropped — try to reconnect and re-subscribe
                loop {
                    ws_retries += 1;
                    if ws_retries > max_ws_retries {
                        if let Some(ref pb) = pb {
                            pb.abandon_with_message(format!("WebSocket error after {max_ws_retries} retries: {e}"));
                        }
                        bail!("WebSocket error after {max_ws_retries} retries: {e}");
                    }
                    let delay = std::cmp::min(ws_retries as u64 * 2, 30);
                    if let OutputFormat::Text = format {
                        if let Some(ref pb) = pb {
                            pb.println(format!("    {} Connection lost, reconnecting in {delay}s ({ws_retries}/{max_ws_retries})...", style("!").yellow()));
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                    match tokio_tungstenite::connect_async(&ws_url).await {
                        Ok((new_ws, _)) => {
                            let (mut new_write, new_read) = new_ws.split();
                            let _ = new_write.send(Message::Text(
                                format!(r#"{{"type":"subscribe","job_id":"{}"}}"#, build_resp.job_id).into(),
                            )).await;
                            read = new_read;
                            ws_retries = 0; // reset on successful reconnect
                            continue 'ws_loop;
                        }
                        Err(_re) => {
                            // Keep retrying — don't bail on reconnect failure
                            continue;
                        }
                    }
                }
            }
        };

        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => break,
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
            ServerMessage::Published { platform, message, .. } => {
                if let OutputFormat::Text = format {
                    println!(
                        "  {} Published to {} — {}",
                        style("✓").green().bold(),
                        style(&platform).cyan(),
                        message
                    );
                }
            }
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
            let dest = args.output.join(&name);

            if let Some(ref src_path) = download_path {
                // Local path available (self-hosted hub) - copy directly
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
                    bail!(
                        "Download failed: {}",
                        resp.status()
                    );
                }

                let bytes = resp.bytes().await?;
                // The hub may store artifacts as base64 (perry runtime doesn't
                // decode Buffer.from(data, 'base64')). Detect and decode.
                let data = if bytes.len() > 4 && bytes.iter().all(|&b| {
                    b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=' || b == b'\n' || b == b'\r'
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
        }
    }
    break; // Normal exit from while loop means stream ended
    } // end 'ws_loop

    if !build_success {
        bail!("Build failed");
    }

    Ok(())
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

/// Should this file be excluded from the tarball?
fn should_exclude_file(path: &Path) -> bool {
    let exclude_extensions = [
        "o", "a", "dylib", "so", "dll", "exe", "dmg", "ipa", "apk", "aab",
    ];
    let name = path.file_name().unwrap_or_default().to_string_lossy();

    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        if exclude_extensions.contains(&ext) {
            return true;
        }
    }
    if name.starts_with('_')
        && path
            .metadata()
            .map(|m| m.len() > 1_000_000)
            .unwrap_or(false)
    {
        return true;
    }
    if path.extension().is_none()
        && path
            .metadata()
            .map(|m| m.len() > 1_000_000)
            .unwrap_or(false)
    {
        return true;
    }
    if name == ".DS_Store" {
        return true;
    }
    false
}

/// Resolve `file:` dependencies from package.json and return (package_name, resolved_path) pairs.
fn resolve_file_deps(project_dir: &Path) -> Vec<(String, PathBuf)> {
    let pkg_path = project_dir.join("package.json");
    let Ok(content) = fs::read_to_string(&pkg_path) else {
        return vec![];
    };
    let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) else {
        return vec![];
    };
    let mut deps = Vec::new();
    for key in ["dependencies", "devDependencies"] {
        if let Some(obj) = pkg.get(key).and_then(|v| v.as_object()) {
            for (name, value) in obj {
                if let Some(spec) = value.as_str() {
                    if let Some(rel_path) = spec.strip_prefix("file:") {
                        let resolved = project_dir.join(rel_path).canonicalize().ok();
                        if let Some(abs_path) = resolved {
                            if abs_path.is_dir() {
                                deps.push((name.clone(), abs_path));
                            }
                        }
                    }
                }
            }
        }
    }
    deps
}

pub(crate) fn create_project_tarball(project_dir: &Path) -> Result<Vec<u8>> {
    create_project_tarball_with_excludes(project_dir, &[])
}

pub(crate) fn create_project_tarball_with_excludes(project_dir: &Path, extra_excludes: &[String]) -> Result<Vec<u8>> {
    let buf = Vec::new();
    let encoder = GzEncoder::new(buf, Compression::default());
    let mut ar = tar::Builder::new(encoder);

    let builtin_exclude_dirs: Vec<&str> = vec![
        "node_modules",
        ".git",
        "dist",
        "build",
        "target",
        ".perry",
        "xcode",
    ];

    // Walk the project directory
    for entry in WalkDir::new(project_dir)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            if builtin_exclude_dirs.iter().any(|ex| name == *ex) {
                return false;
            }
            if extra_excludes.iter().any(|ex| {
                if ex.contains('/') {
                    // Path-based exclude: match against relative path from project root
                    e.path().strip_prefix(project_dir)
                        .map(|rel| rel.starts_with(ex))
                        .unwrap_or(false)
                } else {
                    name == *ex
                }
            }) {
                return false;
            }
            if name.ends_with(".app") {
                return false;
            }
            true
        })
    {
        let entry = entry?;
        let path = entry.path();
        let relative = path.strip_prefix(project_dir)?;

        if relative.as_os_str().is_empty() {
            continue;
        }

        if path.is_file() {
            if should_exclude_file(path) {
                continue;
            }
            ar.append_path_with_name(path, relative)?;
        } else if path.is_dir() {
            ar.append_dir(relative, path)?;
        }
    }

    // Include file: dependencies under node_modules/<pkg-name>/
    let file_deps = resolve_file_deps(project_dir);
    for (pkg_name, dep_dir) in &file_deps {
        let nm_prefix = PathBuf::from("node_modules").join(pkg_name);
        // Walk the dependency directory (exclude .git, target, dist, build artifacts)
        let dep_exclude_dirs = [".git", "target", "dist", "build", "xcode", "node_modules"];
        for entry in WalkDir::new(dep_dir)
            .follow_links(true)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                if dep_exclude_dirs.iter().any(|ex| name == *ex) {
                    return false;
                }
                if name.ends_with(".app") {
                    return false;
                }
                true
            })
        {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let path = entry.path();
            let relative = match path.strip_prefix(dep_dir) {
                Ok(r) => r,
                Err(_) => continue,
            };

            if relative.as_os_str().is_empty() {
                continue;
            }

            let tar_path = nm_prefix.join(relative);

            if path.is_file() {
                if should_exclude_file(path) {
                    continue;
                }
                ar.append_path_with_name(path, &tar_path)?;
            } else if path.is_dir() {
                ar.append_dir(&tar_path, path)?;
            }
        }
    }

    ar.finish()?;
    let encoder = ar.into_inner()?;
    Ok(encoder.finish()?)
}

// --- Saved config (~/.perry/config.toml) ---

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BetaConfig {
    /// User has seen and acknowledged the public beta notice
    pub(crate) acknowledged: bool,
    /// User opted in to automatic error reporting for beta commands
    #[serde(default)]
    pub(crate) report_errors: bool,
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub(crate) struct PerryConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) license_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) server: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) default_target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) api_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) github_username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) apple: Option<AppleSavedConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) ios: Option<IosSavedConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) android: Option<AndroidSavedConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) harmonyos: Option<HarmonyosSavedConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) telemetry: Option<crate::telemetry::TelemetryConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) beta: Option<BetaConfig>,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AppleSavedConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) team_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) p8_key_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) key_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) issuer_id: Option<String>,
}

/// Legacy struct kept for backward compatibility when reading old config files.
/// New configs no longer save iOS-specific fields to the global config.
#[derive(Default, Debug, Serialize, Deserialize)]
pub(crate) struct IosSavedConfig {
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub(crate) struct AndroidSavedConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) keystore_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) key_alias: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) google_play_key_path: Option<String>,
}

/// HarmonyOS signing materials. Populated by `perry setup harmonyos`.
///
/// The p12 password is stored plaintext in `~/.perry/config.toml` (the file is
/// already protected by the user's home dir perms; macOS-Keychain integration
/// is a future improvement). DevEco itself stores the same password
/// AES-encrypted in `build-profile.json5` with a machine-bound key that isn't
/// extractable to external tools — so the wizard prompts the user once and
/// caches it here for subsequent compiles.
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub(crate) struct HarmonyosSavedConfig {
    /// Path to the .p12 keystore (typically `~/.ohos/config/default_*.p12`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) p12_path: Option<String>,
    /// Plaintext password for the .p12 keystore. Same value is used as the
    /// key password — DevEco's auto-generated debug cert uses one password
    /// for both store and key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) p12_password: Option<String>,
    /// Path to the provisioning profile (.p7b).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) profile_path: Option<String>,
    /// Path to the cert chain (.cer / .pem). hap-sign-tool requires this as
    /// `-appCertFile`, distinct from `-profileFile`. DevEco's auto-signing
    /// names it `<bundleName>.cer`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) cert_path: Option<String>,
    /// bundleName the profile is bound to (e.g. `com.example.myapplication`).
    /// Auto-extracted from the .p7b's embedded JSON.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) bundle_name: Option<String>,
    /// Key alias inside the .p12 (DevEco's auto-generated cert uses
    /// `debugKey`; users with their own keystore may have a different alias).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) key_alias: Option<String>,
}

pub(crate) fn config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".perry")
        .join("config.toml")
}

pub(crate) fn load_config() -> PerryConfig {
    let path = config_path();
    if let Ok(content) = fs::read_to_string(&path) {
        toml::from_str(&content).unwrap_or_default()
    } else {
        PerryConfig::default()
    }
}

pub(crate) fn save_config(config: &PerryConfig) -> Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = toml::to_string_pretty(config)
        .context("Failed to serialize config")?;
    fs::write(&path, content)?;
    Ok(())
}

pub(crate) fn is_interactive() -> bool {
    atty::is(atty::Stream::Stdin) && atty::is(atty::Stream::Stdout)
}

/// Show a one-time public beta notice for publish/verify commands.
/// Returns true if the user acknowledges (or has previously acknowledged).
/// Non-interactive sessions skip the prompt and proceed.
pub(crate) fn check_beta_consent(command: &str) -> bool {
    let mut config = load_config();

    // Already acknowledged — nothing to do
    if let Some(ref beta) = config.beta {
        if beta.acknowledged {
            return true;
        }
    }

    // Non-interactive: proceed without prompting (errors won't be reported)
    if !is_interactive() {
        return true;
    }

    eprintln!();
    eprintln!(
        "  {} perry {} is in {}.",
        style("NOTE").yellow().bold(),
        command,
        style("public beta").yellow().bold(),
    );
    eprintln!("  It should work, but if you encounter issues please let us know.");
    eprintln!(
        "  Report issues: {}",
        style("https://github.com/PerryTS/perry/issues").cyan().underlined()
    );
    eprintln!();

    let report = Confirm::new()
        .with_prompt("  Automatically report errors to help us fix issues faster?")
        .default(true)
        .interact()
        .unwrap_or(false);

    let proceed = Confirm::new()
        .with_prompt("  Continue?")
        .default(true)
        .interact()
        .unwrap_or(false);

    if !proceed {
        return false;
    }

    config.beta = Some(BetaConfig {
        acknowledged: true,
        report_errors: report,
    });
    let _ = save_config(&config);

    true
}

/// Send a sanitized error report for a beta command failure.
/// Fire-and-forget on a background thread. No credentials or file paths are included.
pub(crate) fn report_beta_error(command: &str, error: &str, target: Option<&str>) {
    let config = load_config();
    let should_report = config
        .beta
        .as_ref()
        .map_or(false, |b| b.acknowledged && b.report_errors);

    if !should_report {
        return;
    }

    // Sanitize: strip anything that looks like a file path or credential
    let sanitized = sanitize_error_for_report(error);

    crate::telemetry::send_event(
        &format!("beta_error_{}", command),
        &[
            ("error", &sanitized),
            ("target", target.unwrap_or("unknown")),
            ("version", env!("CARGO_PKG_VERSION")),
            ("platform", std::env::consts::OS),
        ],
    );
}

/// Strip file paths, tokens, and other potentially sensitive data from error messages.
fn sanitize_error_for_report(error: &str) -> String {
    let mut result = String::new();
    for word in error.split_whitespace() {
        if !result.is_empty() {
            result.push(' ');
        }
        // Redact absolute file paths
        if word.starts_with('/') || (word.len() >= 3 && word.as_bytes()[1] == b':' && word.as_bytes()[2] == b'\\') {
            result.push_str("<path>");
        // Redact long alphanumeric strings (tokens, keys, base64 blobs)
        } else if word.len() >= 32 && word.chars().all(|c| c.is_alphanumeric() || c == '+' || c == '/' || c == '=') {
            result.push_str("<redacted>");
        } else {
            result.push_str(word);
        }
    }

    // Truncate to 500 chars max
    if result.len() > 500 {
        result.truncate(500);
        result.push_str("...");
    }

    result
}

/// Prompt user for text input with an optional default value.
/// Returns None if the user enters empty string.
pub(crate) fn prompt_input(prompt: &str, default: Option<&str>) -> Option<String> {
    let mut builder = Input::<String>::new().with_prompt(prompt);
    if let Some(d) = default {
        builder = builder.default(d.to_string());
    }
    builder = builder.allow_empty(true);
    match builder.interact_text() {
        Ok(val) if val.is_empty() => None,
        Ok(val) => Some(val),
        Err(_) => None,
    }
}

/// Prompt for target platform selection.
fn prompt_target(default: Option<&str>) -> String {
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
fn resolve_credential(
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
fn auto_export_p12_from_keychain(
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
            .or_else(|| identities.iter().find(|(_, name)| name.contains(configured)))
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
    println!(
        "  Found identity: {}",
        style(&selected.1).bold()
    );
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
            let v = ((seed.wrapping_mul(6364136223846793005).wrapping_add(i as u128 * 1442695040888963407)) >> 16) as u8 % 62;
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
            println!(
                "  {} Could not export from Keychain.",
                style("!").yellow()
            );
            return None;
        }
    }

    // Read, base64-encode, clean up
    let data = std::fs::read(&temp_path).ok()?;
    let _ = std::fs::remove_file(&temp_path);

    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&data);

    println!(
        "  {} Certificate exported successfully",
        style("✓").green()
    );
    Some((b64, password))
}

/// Resolve a file path credential: CLI → env → saved config → interactive prompt.
/// Returns the path string (not validated here).
fn resolve_path_credential(
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
fn validate_credentials_for_distribute(
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
            if apple_key_id.is_none() { missing.push("Key ID (--apple-key-id / PERRY_APPLE_KEY_ID)"); }
            if apple_issuer_id.is_none() { missing.push("Issuer ID (--apple-issuer-id / PERRY_APPLE_ISSUER_ID)"); }
            if p8_key_content.is_none() { missing.push(".p8 key (--apple-p8-key / PERRY_APPLE_P8_KEY)"); }
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
            if apple_key_id.is_none() { missing.push("Key ID (--apple-key-id / PERRY_APPLE_KEY_ID)"); }
            if apple_issuer_id.is_none() { missing.push("Issuer ID (--apple-issuer-id / PERRY_APPLE_ISSUER_ID)"); }
            if p8_key_content.is_none() { missing.push(".p8 key (--apple-p8-key / PERRY_APPLE_P8_KEY)"); }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_perry_config_roundtrip() {
        let config = PerryConfig {
            license_key: Some("FREE-abc123".into()),
            server: Some("https://build.example.com".into()),
            default_target: Some("macos".into()),
            apple: Some(AppleSavedConfig {
                team_id: Some("ABC123DEF".into()),
                p8_key_path: Some("/Users/me/AuthKey_XXX.p8".into()),
                key_id: Some("XXX".into()),
                issuer_id: Some("abc-def-ghi".into()),
            }),
            ios: Some(IosSavedConfig {}),
            android: Some(AndroidSavedConfig {
                keystore_path: Some("/Users/me/release.keystore".into()),
                key_alias: Some("key0".into()),
                google_play_key_path: Some("/Users/me/play-sa.json".into()),
            }),
            ..Default::default()
        };

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: PerryConfig = toml::from_str(&toml_str).unwrap();

        assert_eq!(parsed.license_key, config.license_key);
        assert_eq!(parsed.server, config.server);
        assert_eq!(parsed.default_target, config.default_target);
        assert_eq!(parsed.apple.as_ref().unwrap().team_id, config.apple.as_ref().unwrap().team_id);
        assert_eq!(parsed.android.as_ref().unwrap().google_play_key_path, config.android.as_ref().unwrap().google_play_key_path);
    }

    #[test]
    fn test_perry_config_minimal() {
        let config = PerryConfig {
            license_key: Some("FREE-test".into()),
            ..Default::default()
        };
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(toml_str.contains("license_key"));
        assert!(!toml_str.contains("[apple]"), "empty sections should be omitted");
        assert!(!toml_str.contains("[android]"));
    }

    #[test]
    fn test_perry_config_parse_legacy_format() {
        // Old format was just license_key = "..." — should still parse
        let legacy = r#"license_key = "FREE-legacy-key""#;
        let config: PerryConfig = toml::from_str(legacy).unwrap();
        assert_eq!(config.license_key.as_deref(), Some("FREE-legacy-key"));
        assert!(config.apple.is_none());
        assert!(config.default_target.is_none());
    }

    #[test]
    fn test_perry_config_no_passwords_in_toml() {
        let config = PerryConfig {
            license_key: Some("FREE-test".into()),
            android: Some(AndroidSavedConfig {
                keystore_path: Some("/path/to/keystore".into()),
                key_alias: Some("key0".into()),
                google_play_key_path: None,
            }),
            ..Default::default()
        };
        let toml_str = toml::to_string_pretty(&config).unwrap();
        // Config struct intentionally has no password fields
        assert!(!toml_str.contains("password"));
        assert!(toml_str.contains("keystore_path"));
    }

    #[test]
    fn test_perry_config_default_is_empty() {
        let config = PerryConfig::default();
        assert!(config.license_key.is_none());
        assert!(config.server.is_none());
        assert!(config.default_target.is_none());
        assert!(config.apple.is_none());
        assert!(config.ios.is_none());
        assert!(config.android.is_none());
    }

    #[test]
    fn test_credentials_payload_with_google_play() {
        let creds = CredentialsPayload {
            apple_team_id: None,
            apple_signing_identity: None,
            apple_key_id: None,
            apple_issuer_id: None,
            apple_p8_key: None,
            provisioning_profile_base64: None,
            apple_certificate_p12_base64: None,
            apple_certificate_password: None,
            apple_notarize_certificate_p12_base64: None,
            apple_notarize_certificate_password: None,
            apple_notarize_signing_identity: None,
            apple_installer_certificate_p12_base64: None,
            apple_installer_certificate_password: None,
            android_keystore_base64: Some("dGVzdA==".into()),
            android_keystore_password: Some("pass".into()),
            android_key_alias: Some("key0".into()),
            android_key_password: None,
            google_play_service_account_json: Some("{\"client_email\":\"test@gcp\"}".into()),
            gcloud_kms_key: None,
            gcloud_kms_cert_base64: None,
            gcloud_service_account_base64: None,
        };
        let json = serde_json::to_string(&creds).unwrap();
        assert!(json.contains("google_play_service_account_json"));
        assert!(json.contains("client_email"));
    }

    #[test]
    fn test_credentials_payload_omits_none() {
        let creds = CredentialsPayload {
            apple_team_id: Some("ABC".into()),
            apple_signing_identity: Some("Dev ID".into()),
            apple_key_id: None,
            apple_issuer_id: None,
            apple_p8_key: None,
            provisioning_profile_base64: None,
            apple_certificate_p12_base64: None,
            apple_certificate_password: None,
            apple_notarize_certificate_p12_base64: None,
            apple_notarize_certificate_password: None,
            apple_notarize_signing_identity: None,
            apple_installer_certificate_p12_base64: None,
            apple_installer_certificate_password: None,
            android_keystore_base64: None,
            android_keystore_password: None,
            android_key_alias: None,
            android_key_password: None,
            google_play_service_account_json: None,
            gcloud_kms_key: None,
            gcloud_kms_cert_base64: None,
            gcloud_service_account_base64: None,
        };
        let json = serde_json::to_string(&creds).unwrap();
        // Fields with skip_serializing_if should be absent
        assert!(!json.contains("android_keystore_base64"));
        assert!(!json.contains("google_play_service_account_json"));
        // Non-skip fields are always present (even as null)
        assert!(json.contains("apple_team_id"));
    }

    #[test]
    fn test_resolve_credential_cli_wins() {
        let result = resolve_credential(
            Some("from-cli"),
            "NONEXISTENT_ENV_VAR_XYZ",
            Some("from-saved"),
            "test",
            false,
            false, // not interactive
        );
        assert_eq!(result.as_deref(), Some("from-cli"));
    }

    #[test]
    fn test_resolve_credential_saved_fallback() {
        let result = resolve_credential(
            None,
            "NONEXISTENT_ENV_VAR_XYZ",
            Some("from-saved"),
            "test",
            false,
            false,
        );
        assert_eq!(result.as_deref(), Some("from-saved"));
    }

    #[test]
    fn test_resolve_credential_none_when_missing() {
        let result = resolve_credential(
            None,
            "NONEXISTENT_ENV_VAR_XYZ",
            None,
            "test",
            false,
            false,
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_credential_skips_empty() {
        let result = resolve_credential(
            Some(""),
            "NONEXISTENT_ENV_VAR_XYZ",
            Some("saved"),
            "test",
            false,
            false,
        );
        assert_eq!(result.as_deref(), Some("saved"));
    }

    #[test]
    fn test_resolve_path_credential() {
        let result = resolve_path_credential(
            Some(Path::new("/path/to/file")),
            "NONEXISTENT_ENV_VAR_XYZ",
            Some("/saved/path"),
            "test",
            false,
        );
        assert_eq!(result.as_deref(), Some("/path/to/file"));
    }

    #[test]
    fn test_resolve_path_credential_saved_fallback() {
        let result = resolve_path_credential(
            None,
            "NONEXISTENT_ENV_VAR_XYZ",
            Some("/saved/path"),
            "test",
            false,
        );
        assert_eq!(result.as_deref(), Some("/saved/path"));
    }

    #[test]
    fn test_config_file_write_and_read() {
        // Test writing to a temp location and reading back
        let dir = std::env::temp_dir().join("perry-test-config");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("config.toml");

        let config = PerryConfig {
            license_key: Some("TEST-KEY-123".into()),
            default_target: Some("ios".into()),
            apple: Some(AppleSavedConfig {
                team_id: Some("TEAM123".into()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let content = toml::to_string_pretty(&config).unwrap();
        fs::write(&path, &content).unwrap();

        let read_back = fs::read_to_string(&path).unwrap();
        let parsed: PerryConfig = toml::from_str(&read_back).unwrap();
        assert_eq!(parsed.license_key.as_deref(), Some("TEST-KEY-123"));
        assert_eq!(parsed.default_target.as_deref(), Some("ios"));
        assert_eq!(parsed.apple.unwrap().team_id.as_deref(), Some("TEAM123"));

        // Cleanup
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir(&dir);
    }

    #[test]
    fn test_validate_android_playstore_requires_json() {
        let result = validate_credentials_for_distribute(
            true, Some("playstore"), None, // android, no key
            false, None, None, None, None, // ios not applicable
            false, None,                   // macos not applicable
        );
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("service account JSON key"), "{msg}");
        assert!(msg.contains("perry setup android"), "{msg}");
    }

    #[test]
    fn test_validate_android_playstore_invalid_track() {
        let result = validate_credentials_for_distribute(
            true, Some("playstore:bogus"), Some("{}"),
            false, None, None, None, None,
            false, None,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid Play Store track"));
    }

    #[test]
    fn test_validate_android_playstore_valid_tracks() {
        for track in ["internal", "alpha", "beta", "production"] {
            let distribute = format!("playstore:{track}");
            let result = validate_credentials_for_distribute(
                true, Some(&distribute), Some("{\"ok\":1}"),
                false, None, None, None, None,
                false, None,
            );
            assert!(result.is_ok(), "track={track} should be valid");
        }
    }

    #[test]
    fn test_validate_ios_appstore_requires_creds() {
        let result = validate_credentials_for_distribute(
            false, None, None,
            true, Some("appstore"), None, None, None, // ios, missing creds
            false, None,
        );
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("App Store Connect API credentials"), "{msg}");
        assert!(msg.contains("perry setup ios"), "{msg}");
    }

    #[test]
    fn test_validate_ios_testflight_requires_creds() {
        let result = validate_credentials_for_distribute(
            false, None, None,
            true, Some("testflight"), Some("kid"), None, Some("key_content"),
            false, None,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Issuer ID"));
    }

    #[test]
    fn test_validate_ios_no_distribute_passes() {
        let result = validate_credentials_for_distribute(
            false, None, None,
            true, None, None, None, None, // ios but no distribute set
            false, None,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_macos_appstore_requires_creds() {
        let result = validate_credentials_for_distribute(
            false, None, None,
            false, None, None, None, None,
            true, Some("appstore"), // macos appstore, no creds
        );
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("App Store Connect API credentials"), "{msg}");
        assert!(msg.contains("perry setup macos"), "{msg}");
    }

    #[test]
    fn test_validate_macos_testflight_requires_creds() {
        let result = validate_credentials_for_distribute(
            false, None, None,
            false, None, None, None, None,
            true, Some("testflight"), // macos testflight, no creds
        );
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("App Store Connect API credentials"), "{msg}");
    }

    #[test]
    fn test_validate_macos_notarize_requires_creds() {
        let result = validate_credentials_for_distribute(
            false, None, None,
            false, None, None, None, None,
            true, Some("notarize"), // macos notarize, no creds
        );
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("notarization"), "{msg}");
    }

    #[test]
    fn test_validate_passes_when_all_present() {
        let result = validate_credentials_for_distribute(
            false, None, None,
            true, Some("appstore"), Some("kid"), Some("iid"), Some("p8"),
            false, None,
        );
        assert!(result.is_ok());
    }
}
