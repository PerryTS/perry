use super::*;

// --- Config types matching perry.toml ---

// #854: deserialized perry.toml table; not every key is read on every path.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub(super) struct PerryToml {
    pub(super) project: Option<ProjectConfig>,
    pub(super) app: Option<AppConfig>,
    pub(super) macos: Option<MacosConfig>,
    pub(super) ios: Option<IosConfig>,
    pub(super) visionos: Option<VisionosConfig>,
    pub(super) watchos: Option<WatchosConfig>,
    pub(super) tvos: Option<TvosConfig>,
    pub(super) android: Option<AndroidConfig>,
    pub(super) linux: Option<LinuxConfig>,
    pub(super) windows: Option<WindowsConfig>,
    pub(super) build: Option<BuildConfig>,
    pub(super) publish: Option<PublishConfig>,
    pub(super) audit: Option<AuditConfig>,
    pub(super) verify: Option<VerifyConfig>,
    pub(super) release_notes: Option<std::collections::HashMap<String, String>>,
    pub(super) google_auth: Option<GoogleAuthConfig>,
}

/// `[google_auth]` block (#1138 / #1303). Only `framework_dir` matters to
/// `perry publish` — the client-id keys are consumed at compile time
/// (`host_config.rs` / `apple_info_plist.rs`). `framework_dir` is a
/// project-relative path to the vendored optional-framework search dir
/// (e.g. the GoogleSignIn SDK); publish must force it into the upload
/// tarball so the worker can link the real SDK. See issue #1303.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub(super) struct GoogleAuthConfig {
    pub(super) framework_dir: Option<String>,
}

// #854: deserialized [project] table; not every key is read.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub(super) struct ProjectConfig {
    pub(super) name: Option<String>,
    pub(super) version: Option<String>,
    pub(super) build_number: Option<u64>,
    pub(super) bundle_id: Option<String>,
    pub(super) description: Option<String>,
    pub(super) entry: Option<String>,
    pub(super) icons: Option<IconsConfig>,
    pub(super) features: Option<Vec<String>>,
}

// #854: deserialized [app] table; not every key is read.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub(super) struct AppConfig {
    pub(super) name: Option<String>,
    pub(super) version: Option<String>,
    pub(super) build_number: Option<u64>,
    pub(super) bundle_id: Option<String>,
    pub(super) description: Option<String>,
    pub(super) entry: Option<String>,
    pub(super) icons: Option<IconsConfig>,
}

#[derive(Debug, Deserialize)]
pub(super) struct IconsConfig {
    pub(super) source: Option<String>,
}

// #854: deserialized [macos] table; not every key is read.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub(super) struct MacosConfig {
    pub(super) bundle_id: Option<String>,
    pub(super) category: Option<String>,
    pub(super) minimum_os: Option<String>,
    pub(super) entitlements: Option<Vec<String>>,
    /// "appstore", "notarize", or "both"
    pub(super) distribute: Option<String>,
    pub(super) signing_identity: Option<String>,
    // Per-project signing credentials (override global ~/.perry/config.toml)
    pub(super) certificate: Option<String>,
    pub(super) team_id: Option<String>,
    pub(super) key_id: Option<String>,
    pub(super) issuer_id: Option<String>,
    pub(super) p8_key_path: Option<String>,
    /// If true, adds ITSAppUsesNonExemptEncryption=NO to Info.plist
    pub(super) encryption_exempt: Option<bool>,
    /// For distribute = "both": separate Developer ID cert for notarization
    pub(super) notarize_certificate: Option<String>,
    pub(super) notarize_signing_identity: Option<String>,
    /// Separate .p12 for the Mac Installer Distribution cert (for .pkg signing)
    pub(super) installer_certificate: Option<String>,
    /// Provisioning profile for App Store / TestFlight distribution
    pub(super) provisioning_profile: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct IosConfig {
    pub(super) bundle_id: Option<String>,
    pub(super) deployment_target: Option<String>,
    /// Alias for deployment_target (perry.toml uses minimum_version)
    pub(super) minimum_version: Option<String>,
    pub(super) device_family: Option<Vec<String>>,
    pub(super) orientations: Option<Vec<String>>,
    pub(super) capabilities: Option<Vec<String>>,
    pub(super) distribute: Option<String>,
    pub(super) entry: Option<String>,
    // Per-project signing credentials (override global ~/.perry/config.toml)
    pub(super) provisioning_profile: Option<String>,
    pub(super) certificate: Option<String>,
    pub(super) signing_identity: Option<String>,
    pub(super) team_id: Option<String>,
    pub(super) key_id: Option<String>,
    pub(super) issuer_id: Option<String>,
    pub(super) p8_key_path: Option<String>,
    /// If true, adds ITSAppUsesNonExemptEncryption=NO to Info.plist
    /// (skips the export compliance prompt in App Store Connect)
    pub(super) encryption_exempt: Option<bool>,
    /// Custom Info.plist entries (key-value pairs added to the generated plist).
    /// Use for privacy descriptions, custom URL schemes, etc.
    /// Example: { NSMicrophoneUsageDescription = "Measures ambient sound levels" }
    pub(super) info_plist: Option<std::collections::HashMap<String, String>>,
}

// #854: deserialized [visionos] table; not every key is read.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub(super) struct VisionosConfig {
    pub(super) bundle_id: Option<String>,
    pub(super) deployment_target: Option<String>,
    pub(super) minimum_version: Option<String>,
    pub(super) distribute: Option<String>,
    pub(super) entry: Option<String>,
    pub(super) provisioning_profile: Option<String>,
    pub(super) certificate: Option<String>,
    pub(super) signing_identity: Option<String>,
    pub(super) team_id: Option<String>,
    pub(super) key_id: Option<String>,
    pub(super) issuer_id: Option<String>,
    pub(super) p8_key_path: Option<String>,
    pub(super) encryption_exempt: Option<bool>,
    pub(super) info_plist: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct AndroidConfig {
    pub(super) package_name: Option<String>,
    pub(super) min_sdk: Option<String>,
    pub(super) target_sdk: Option<String>,
    pub(super) permissions: Option<Vec<String>>,
    pub(super) distribute: Option<String>,
    pub(super) keystore: Option<String>,
    pub(super) key_alias: Option<String>,
    pub(super) google_play_key: Option<String>,
    pub(super) entry: Option<String>,
}

// #854: deserialized [watchos] table; not every key is read.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub(super) struct WatchosConfig {
    pub(super) bundle_id: Option<String>,
    pub(super) deployment_target: Option<String>,
    pub(super) encryption_exempt: Option<bool>,
    pub(super) info_plist: Option<std::collections::HashMap<String, String>>,
    pub(super) team_id: Option<String>,
    pub(super) signing_identity: Option<String>,
}

// #854: deserialized [tvos] table; not every key is read.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub(super) struct TvosConfig {
    pub(super) bundle_id: Option<String>,
    pub(super) entry: Option<String>,
    pub(super) deployment_target: Option<String>,
    pub(super) encryption_exempt: Option<bool>,
    pub(super) info_plist: Option<std::collections::HashMap<String, String>>,
    pub(super) team_id: Option<String>,
    pub(super) signing_identity: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct LinuxConfig {
    pub(super) format: Option<String>,
    pub(super) category: Option<String>,
    pub(super) description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct WindowsConfig {
    /// Google Cloud KMS key path for code signing
    /// e.g. "projects/skelpo/locations/europe-west3/keyRings/code-signing-eu/cryptoKeys/skelpo-codesign/cryptoKeyVersions/1"
    pub(super) gcloud_kms_key: Option<String>,
    /// Path to the code signing certificate (.crt)
    pub(super) gcloud_kms_cert: Option<String>,
    /// Path to GCP service account JSON key file
    pub(super) gcloud_service_account: Option<String>,
}

// #854: deserialized [build] table; out_dir not read on this path.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub(super) struct BuildConfig {
    pub(super) out_dir: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct PublishConfig {
    pub(super) server: Option<String>,
    /// Extra directories to exclude from the upload tarball (e.g. ["screenshots", "docs"])
    pub(super) exclude: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct AuditConfig {
    pub(super) fail_on: Option<String>,
    pub(super) ignore: Option<Vec<String>>,
    pub(super) severity: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct VerifyConfig {
    pub(super) url: Option<String>,
}
