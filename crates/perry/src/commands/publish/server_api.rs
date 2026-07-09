use super::*;

// --- Server API types ---

// #854: deserialized server response — full wire shape kept even where a
// field isn't consumed on the client path.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub(super) struct RegisterResponse {
    pub(super) license_key: String,
    pub(super) tier: String,
    pub(super) platforms: Vec<String>,
}

// #854: deserialized server response — full wire shape kept.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub(super) struct BuildResponse {
    pub(super) job_id: String,
    pub(super) ws_url: String,
    pub(super) position: usize,
}

// #854: full server-message protocol — every variant/field is part of the
// deserialized wire contract; some payload fields aren't consumed yet.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ServerMessage {
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
pub(super) struct BuildManifest {
    pub(super) app_name: String,
    /// Home Screen display name (CFBundleDisplayName). `None` → the build
    /// service omits the key and the icon label falls back to CFBundleName.
    pub(super) display_name: Option<String>,
    pub(super) bundle_id: String,
    pub(super) version: String,
    pub(super) short_version: Option<String>,
    pub(super) entry: String,
    pub(super) icon: Option<String>,
    pub(super) targets: Vec<String>,
    pub(super) category: Option<String>,
    pub(super) minimum_os_version: Option<String>,
    pub(super) entitlements: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) ios_deployment_target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) ios_device_family: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) ios_orientations: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) ios_capabilities: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) ios_distribute: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) ios_encryption_exempt: Option<bool>,
    /// Custom Info.plist entries for iOS (e.g. NSMicrophoneUsageDescription)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) ios_info_plist: Option<std::collections::HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) visionos_deployment_target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) visionos_distribute: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) visionos_encryption_exempt: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) visionos_info_plist: Option<std::collections::HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) macos_distribute: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) macos_encryption_exempt: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) tvos_deployment_target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) tvos_encryption_exempt: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) tvos_info_plist: Option<std::collections::HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) tvos_distribute: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) watchos_deployment_target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) watchos_encryption_exempt: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) watchos_info_plist: Option<std::collections::HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) watchos_distribute: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) android_min_sdk: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) android_target_sdk: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) android_permissions: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) android_distribute: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) android_version_code: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) linux_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) linux_category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) linux_description: Option<String>,
    /// `glibc` (default) or `musl` (fully static). Drives the worker's
    /// `perry compile --target linux[-musl]` selection. #4826.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) linux_libc: Option<String>,
    /// CPU baseline for the produced binaries (#6125): an LLVM CPU name
    /// (`x86-64-v2`, …), `native`, or `generic`. Drives the worker's
    /// `perry compile --march <value>`. The client defaults linux builds to
    /// `x86-64-v2` — the hub worker compiles linux-on-linux natively, and
    /// without a pinned baseline the binary inherits the build box's full
    /// ISA (AVX-512) and SIGILLs on older x86-64 CPUs (Zen2/EPYC-Rome,
    /// pre-Skylake Xeons). `[build] native_tuning = true` /
    /// `--march native` opts back into build-machine tuning.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) build_march: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) release_notes: Option<std::collections::HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) features: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub(super) struct CredentialsPayload {
    pub(super) apple_team_id: Option<String>,
    pub(super) apple_signing_identity: Option<String>,
    pub(super) apple_key_id: Option<String>,
    pub(super) apple_issuer_id: Option<String>,
    pub(super) apple_p8_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) provisioning_profile_base64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) apple_certificate_p12_base64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) apple_certificate_password: Option<String>,
    /// For macOS distribute = "both": separate Developer ID cert for notarization
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) apple_notarize_certificate_p12_base64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) apple_notarize_certificate_password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) apple_notarize_signing_identity: Option<String>,
    /// Separate .p12 for the Mac Installer Distribution cert (for .pkg signing)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) apple_installer_certificate_p12_base64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) apple_installer_certificate_password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) android_keystore_base64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) android_keystore_password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) android_key_alias: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) android_key_password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) google_play_service_account_json: Option<String>,
    /// Google Cloud KMS key path for Windows code signing
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) gcloud_kms_key: Option<String>,
    /// Base64-encoded code signing certificate for GCloud KMS
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) gcloud_kms_cert_base64: Option<String>,
    /// Base64-encoded GCP service account JSON key
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) gcloud_service_account_base64: Option<String>,
}
