use super::*;

#[derive(Args, Debug)]
pub struct PublishArgs {
    /// Target platform (macos, ios, android, linux)
    #[arg(value_enum)]
    pub platform: Option<Platform>,

    /// C library for the `linux` target: `glibc` (default, dynamic) or
    /// `musl` (fully static). `--libc musl` produces a binary with no glibc
    /// loader dependency that runs on AWS Lambda `provided.al2023`,
    /// scratch/distroless containers, and Cloud Run. Overrides the
    /// `[linux] libc` setting in perry.toml. Ignored for non-Linux targets.
    /// See #4826.
    #[arg(long)]
    pub libc: Option<String>,

    /// CPU baseline for the produced binaries (#6125): an LLVM CPU name
    /// (`x86-64-v2`, `x86-64-v3`, …), `native` (tune to the build worker's
    /// machine — fastest, but the binary only runs on CPUs at least as new),
    /// or `generic` (the target architecture's portable baseline). Overrides
    /// the `[build] march` / `[build] native_tuning` perry.toml settings.
    /// Linux defaults to `x86-64-v2` so published services run on any
    /// x86-64 CPU from the last ~15 years instead of inheriting the build
    /// server's instruction set (AVX-512 → SIGILL on Zen2/EPYC and other
    /// non-AVX-512 hosts).
    #[arg(long)]
    pub march: Option<String>,

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
