//! Run command - compile and launch a TypeScript file in one step

use anyhow::{anyhow, bail, Context, Result};
use clap::Args;
use console::style;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::compile::{CompileArgs, CompileResult};
use crate::{OutputFormat, Platform};

#[derive(Args, Debug)]
pub struct RunArgs {
    /// Positional args: [platform] [input]. Platform is one of: macos, ios,
    /// visionos, watchos, tvos, android, linux, windows, web. If the first arg is not a
    /// known platform it is treated as the input file/directory.
    pub positional: Vec<String>,

    /// Specific iOS simulator UDID to target
    #[arg(long)]
    pub simulator: Option<String>,

    /// Specific iOS device UDID to target
    #[arg(long)]
    pub device: Option<String>,

    /// Enable V8 JavaScript runtime
    #[arg(long)]
    pub enable_js_runtime: bool,

    /// Enable type checking via tsgo
    #[arg(long)]
    pub type_check: bool,

    /// Force local compilation (error if toolchain missing)
    #[arg(long)]
    pub local: bool,

    /// Force remote compilation via Perry Hub build server
    #[arg(long)]
    pub remote: bool,

    /// Enable geisterhand in-process input fuzzer (debug/testing)
    #[arg(long)]
    pub enable_geisterhand: bool,

    /// Port for the geisterhand HTTP server (default: 7676).
    /// Implies --enable-geisterhand.
    #[arg(long)]
    pub geisterhand_port: Option<u16>,

    /// Arguments passed to the compiled program
    #[arg(last = true)]
    pub program_args: Vec<String>,
}

impl RunArgs {
    /// Parse the positional arguments into an optional platform and optional input path.
    /// If the first positional arg is a known platform name, it's used as the platform;
    /// otherwise it's treated as the input file/directory.
    pub fn parse_positional(&self) -> (Option<Platform>, Option<PathBuf>) {
        let mut iter = self.positional.iter();
        let first = match iter.next() {
            Some(v) => v,
            None => return (None, None),
        };

        // Try to parse as a platform
        if let Some(platform) = parse_platform(first) {
            let input = iter.next().map(PathBuf::from);
            (Some(platform), input)
        } else {
            // Not a platform — treat as input path
            (None, Some(PathBuf::from(first)))
        }
    }
}

fn parse_platform(s: &str) -> Option<Platform> {
    match s.to_lowercase().as_str() {
        "macos" => Some(Platform::Macos),
        "ios" => Some(Platform::Ios),
        "visionos" => Some(Platform::Visionos),
        "watchos" => Some(Platform::Watchos),
        "tvos" => Some(Platform::Tvos),
        "android" => Some(Platform::Android),
        "linux" => Some(Platform::Linux),
        "windows" => Some(Platform::Windows),
        "web" => Some(Platform::Web),
        _ => None,
    }
}

/// A detected simulator or device
struct DeviceInfo {
    udid: String,
    name: String,
}

pub fn run(args: RunArgs, format: OutputFormat, use_color: bool, verbose: u8) -> Result<()> {
    // 0. Parse positional args into platform + input
    let (platform, input_path) = args.parse_positional();

    // 1. Resolve entry file
    let input = resolve_entry_file(input_path.as_deref())?;

    // 2. Resolve target and device
    let (target, device_udid) = resolve_target(platform, &args)?;

    // 3. Decide local vs remote compilation
    let needs_cross = matches!(target.as_deref(),
        Some("ios-simulator") | Some("ios") |
        Some("visionos-simulator") | Some("visionos") |
        Some("android") |
        Some("watchos-simulator") | Some("watchos") |
        Some("tvos-simulator") | Some("tvos")
    );
    let can_local = !needs_cross || can_compile_locally(target.as_deref());

    let use_remote = if args.remote {
        true
    } else if args.local {
        if !can_local {
            bail!(
                "Local compilation for {:?} requires cross-compiled runtime libraries.\n\
                 Build with: cargo build --release -p perry-runtime -p perry-stdlib --target {}\n\
                 Or use --remote to compile via Perry Hub.",
                target.as_deref().unwrap_or("native"),
                rust_target_triple(target.as_deref()).unwrap_or("unknown")
            );
        }
        false
    } else {
        // Auto-detect: use remote when local isn't possible
        needs_cross && !can_local
    };

    if use_remote {
        let target_str = target.as_deref().unwrap_or("native");
        let rt = tokio::runtime::Runtime::new()?;
        let result = rt.block_on(remote_build_and_launch(
            &input,
            target_str,
            device_udid.as_deref(),
            &args.program_args,
            args.enable_geisterhand || args.geisterhand_port.is_some(),
            args.geisterhand_port,
            format,
        ));
        return result;
    }

    // Read app metadata from perry.toml / package.json
    let project_dir = input.parent().unwrap_or(Path::new("."))
        .canonicalize().unwrap_or_else(|_| PathBuf::from("."));
    let project_root = find_project_root(&project_dir);
    let (app_name, bundle_id) = read_app_metadata(&project_root, &input);

    // Local compile path
    let compile_args = CompileArgs {
        input: input.clone(),
        output: Some(PathBuf::from(&app_name)),
        keep_intermediates: false,
        print_hir: false,
        no_link: false,
        enable_js_runtime: args.enable_js_runtime,
        target: target.clone(),
        app_bundle_id: Some(bundle_id),
        output_type: "executable".to_string(),
        bundle_extensions: None,
        type_check: args.type_check,
        minify: target.as_deref() == Some("web"),
        features: None,
        enable_geisterhand: args.enable_geisterhand || args.geisterhand_port.is_some(),
        geisterhand_port: args.geisterhand_port,
        minimal_stdlib: false,
        no_auto_optimize: false,
        no_cache: false,
    };

    let result = super::compile::run(compile_args, format, use_color, verbose)?;

    // Local iOS device builds need code signing before install
    if matches!(target.as_deref(), Some("ios") | Some("visionos")) {
        if let Some(udid) = device_udid.as_deref() {
            let config = super::publish::load_config();
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(resign_for_development(&result.output_path, &config, udid, format))?;
        }
    }

    launch(&result, device_udid.as_deref(), &args.program_args, format)
}

/// Check if we have the cross-compiled runtime libraries for a target.
/// Uses the same search logic as compile.rs find_library().
fn can_compile_locally(target: Option<&str>) -> bool {
    let triple = match rust_target_triple(target) {
        Some(t) => t,
        None => return true, // host build, always available
    };
    // Check CWD (running from source tree)
    let cwd_path = format!("target/{triple}/release/libperry_runtime.a");
    if Path::new(&cwd_path).exists() {
        return true;
    }
    // Check original source tree (when cargo install'd)
    let source_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target").join(triple).join("release/libperry_runtime.a");
    source_path.exists()
}

/// Map perry target names to Rust target triples
fn rust_target_triple(target: Option<&str>) -> Option<&'static str> {
    match target {
        Some("ios-simulator") => Some("aarch64-apple-ios-sim"),
        Some("ios") => Some("aarch64-apple-ios"),
        Some("visionos-simulator") => Some("aarch64-apple-visionos-sim"),
        Some("visionos") => Some("aarch64-apple-visionos"),
        Some("tvos-simulator") => Some("aarch64-apple-tvos-sim"),
        Some("tvos") => Some("aarch64-apple-tvos"),
        Some("android") => Some("aarch64-linux-android"),
        _ => None,
    }
}

// --- Remote build ---

/// Build remotely via Perry Hub and launch the result
async fn remote_build_and_launch(
    input: &Path,
    target: &str,
    device_udid: Option<&str>,
    program_args: &[String],
    enable_geisterhand: bool,
    geisterhand_port: Option<u16>,
    format: OutputFormat,
) -> Result<()> {
    use super::publish::{
        auto_register_license, create_project_tarball_with_excludes, load_config, save_config,
    };
    use base64::Engine;
    use futures_util::{SinkExt, StreamExt};
    use indicatif::{ProgressBar, ProgressStyle};
    use reqwest::multipart;
    use serde::Deserialize;
    use std::io::Write;
    use tokio_tungstenite::tungstenite::Message;

    let project_dir = input
        .parent()
        .unwrap_or(Path::new("."))
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from("."));

    // Walk up to find project root (directory containing package.json or perry.toml)
    let project_root = find_project_root(&project_dir);

    // Resolve server URL and license key
    let mut config = load_config();
    let server_url = config
        .server
        .clone()
        .unwrap_or_else(|| "https://hub.perryts.com".into());

    let license_key = match config.license_key.clone() {
        Some(key) => key,
        None => {
            if let OutputFormat::Text = format {
                println!("  Registering with Perry Hub...");
            }
            let key = auto_register_license(&server_url).await?;
            config.license_key = Some(key.clone());
            save_config(&config)?;
            key
        }
    };

    // Determine app name and bundle ID from package.json
    let (app_name, bundle_id) = read_app_metadata(&project_root, input);

    // Determine entry path relative to project root
    let entry = input
        .canonicalize()
        .unwrap_or_else(|_| input.to_path_buf())
        .strip_prefix(&project_root)
        .unwrap_or(input)
        .to_string_lossy()
        .to_string();

    // The build target for the manifest
    let build_target = match target {
        "ios-simulator" | "ios" => "ios",
        other => other,
    };

    if let OutputFormat::Text = format {
        println!();
        println!(
            "  {} Building {} for {} via Perry Hub",
            style("▶").cyan().bold(),
            style(&app_name).bold(),
            style(target).cyan()
        );
        println!();
    }

    // Package project
    if let OutputFormat::Text = format {
        print!("  Packaging project...");
        std::io::stdout().flush().ok();
    }

    // Read [publish].exclude from perry.toml so `run` excludes the same dirs as `publish`
    let publish_excludes = std::fs::read_to_string(project_root.join("perry.toml"))
        .ok()
        .and_then(|s| s.parse::<toml::Value>().ok())
        .and_then(|v| v.get("publish")?.get("exclude")?.as_array().cloned())
        .map(|arr| arr.into_iter().filter_map(|v| v.as_str().map(String::from)).collect::<Vec<_>>())
        .unwrap_or_default();
    let tarball = create_project_tarball_with_excludes(&project_root, &publish_excludes).context("Failed to create project tarball")?;

    if let OutputFormat::Text = format {
        println!(
            " {} ({:.1} MB)",
            style("done").green(),
            tarball.len() as f64 / 1_048_576.0
        );
    }

    // Build manifest
    let ios_distribute = match target {
        "ios" => "development",   // device build needs dev signing, not distribution
        "ios-simulator" => "simulator",
        _ => "none",
    };
    let manifest = serde_json::json!({
        "app_name": app_name,
        "bundle_id": bundle_id,
        "version": "0.0.1",
        "entry": entry,
        "targets": [build_target],
        "ios_distribute": ios_distribute,
        "enable_geisterhand": enable_geisterhand,
        "geisterhand_port": geisterhand_port.unwrap_or(7676),
    });

    // Build credentials — device builds need signing
    let ios_toml = read_perry_toml_ios(&project_root);
    let credentials = if target == "ios" {
        build_device_credentials(&config, &bundle_id, ios_toml.as_ref())?
    } else {
        serde_json::json!({
            "apple_team_id": null,
            "apple_signing_identity": null,
            "apple_key_id": null,
            "apple_issuer_id": null,
            "apple_p8_key": null
        })
    };

    // Upload
    if let OutputFormat::Text = format {
        print!("  Uploading to build server...");
        std::io::stdout().flush().ok();
    }

    let tarball_b64 = base64::engine::general_purpose::STANDARD.encode(&tarball);

    let client = reqwest::Client::new();
    let form = multipart::Form::new()
        .text("license_key", license_key)
        .text("manifest", serde_json::to_string(&manifest)?)
        .text("credentials", serde_json::to_string(&credentials)?);

    let form = form.text("tarball_b64", tarball_b64);

    let resp = client
        .post(format!("{server_url}/api/v1/build"))
        .multipart(form)
        .send()
        .await
        .context("Failed to connect to build server")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("Build server returned {status}: {body}");
    }

    #[derive(Deserialize)]
    struct BuildResponse {
        job_id: String,
        ws_url: String,
        position: usize,
    }

    let build_resp: BuildResponse = resp.json().await.context("Invalid build response")?;

    if let OutputFormat::Text = format {
        println!(" {}", style("done").green());
        println!("  Job ID:    {}", style(&build_resp.job_id).dim());
        if build_resp.position > 1 {
            println!("  Position:  {}", build_resp.position);
        }
        println!();
    }

    // WebSocket progress
    let ws_url = if build_resp.ws_url.starts_with("ws://") || build_resp.ws_url.starts_with("wss://") {
        build_resp.ws_url.clone()
    } else if server_url.starts_with("https://") {
        format!("wss://{}{}", &server_url["https://".len()..], build_resp.ws_url)
    } else {
        format!("ws://{}{}", &server_url["http://".len()..], build_resp.ws_url)
    };

    let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .context("Failed to connect WebSocket")?;

    let (mut ws_write, mut read) = ws_stream.split();

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

    #[derive(Debug, Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    enum ServerMsg {
        JobCreated {
            #[serde(default)]
            job_id: Option<String>,
        },
        QueueUpdate {
            position: usize,
        },
        Stage {
            #[allow(dead_code)]
            stage: String,
            message: String,
        },
        Log {
            line: String,
            stream: String,
        },
        Progress {
            percent: u8,
        },
        ArtifactReady {
            artifact_name: String,
            download_url: String,
            #[serde(default)]
            download_path: Option<String>,
        },
        Published {
            #[allow(dead_code)]
            platform: String,
            #[allow(dead_code)]
            message: String,
        },
        Error {
            message: String,
        },
        #[serde(other)]
        Unknown,
    }

    let mut download_url: Option<String> = None;
    let mut download_path: Option<String> = None;
    let mut artifact_name: Option<String> = None;
    let mut build_success = false;

    while let Some(msg) = read.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                if let Some(ref pb) = pb {
                    pb.abandon_with_message(format!("WebSocket error: {e}"));
                }
                bail!("WebSocket error: {e}");
            }
        };

        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => break,
            _ => continue,
        };

        let server_msg: ServerMsg = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(_) => continue,
        };

        match server_msg {
            ServerMsg::JobCreated { .. } => {
                if let Some(ref pb) = pb {
                    pb.set_message("Build started");
                }
            }
            ServerMsg::QueueUpdate { position, .. } => {
                if let Some(ref pb) = pb {
                    pb.set_message(format!("Queue position: {position}"));
                }
            }
            ServerMsg::Stage { message, .. } => {
                if let Some(ref pb) = pb {
                    pb.set_message(format!("▶️  {message}"));
                }
            }
            ServerMsg::Log { line, stream, .. } => {
                if let Some(ref pb) = pb {
                    if stream == "stderr" {
                        pb.println(format!("    {}", style(&line).dim()));
                    }
                }
            }
            ServerMsg::Progress { percent, .. } => {
                if let Some(ref pb) = pb {
                    pb.set_position(percent as u64);
                }
            }
            ServerMsg::ArtifactReady {
                artifact_name: name,
                download_url: url,
                download_path: path,
                ..
            } => {
                if let Some(ref pb) = pb {
                    pb.set_position(100);
                    pb.finish_with_message(format!(
                        "✓ Build complete: {}",
                        style(&name).bold()
                    ));
                }
                download_url = Some(url);
                download_path = path;
                artifact_name = Some(name);
                build_success = true;
                break; // artifact ready, proceed to download
            }
            ServerMsg::Published { .. } => {
                build_success = true;
                break;
            }
            ServerMsg::Error { message, .. } => {
                if let Some(ref pb) = pb {
                    pb.abandon_with_message(format!("✗ {message}"));
                }
                bail!("Build failed: {message}");
            }
            ServerMsg::Unknown => {}
        }
    }

    if !build_success {
        bail!("Build failed (no artifact received)");
    }

    // Download artifact
    let (url, name) = match (download_url, artifact_name) {
        (Some(u), Some(n)) => (u, n),
        _ => bail!("Build succeeded but no download URL received"),
    };

    if let OutputFormat::Text = format {
        print!("  Downloading {}...", name);
        std::io::stdout().flush().ok();
    }

    let dist_dir = PathBuf::from("dist");
    std::fs::create_dir_all(&dist_dir)?;
    let dest = dist_dir.join(&name);

    if let Some(ref src_path) = download_path {
        std::fs::copy(src_path, &dest)
            .with_context(|| format!("Failed to copy artifact from {src_path}"))?;
    } else {
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
        // Detect base64-encoded content
        let data = if bytes.len() > 4
            && bytes.iter().all(|&b| {
                b.is_ascii_alphanumeric()
                    || b == b'+'
                    || b == b'/'
                    || b == b'='
                    || b == b'\n'
                    || b == b'\r'
            })
        {
            base64::engine::general_purpose::STANDARD
                .decode(&bytes)
                .unwrap_or_else(|_| bytes.to_vec())
        } else {
            bytes.to_vec()
        };
        std::fs::write(&dest, &data)?;
    }

    if let OutputFormat::Text = format {
        println!(" {} → {}", style("done").green(), style(dest.display()).bold());
        println!();
    }

    // For iOS: extract .app from .ipa and install
    if target == "ios-simulator" || target == "ios" || target == "visionos-simulator" || target == "visionos" {
        let app_dir = extract_app_from_ipa(&dest, &dist_dir)?;
        let udid = device_udid.ok_or_else(|| anyhow!("No device UDID for iOS launch"))?;

        // Bundle resource files from the project into the .app
        // (the hub may not include logo/, assets/, etc.)
        bundle_project_resources(&app_dir, &project_root);

        // Embed app icon from project source if missing from the bundle
        embed_app_icon(&app_dir, &project_root);

        // For device builds, re-sign with a local development identity
        // (the hub may have signed with a distribution profile)
        if target == "ios" || target == "visionos" {
            resign_for_development(&app_dir, &config, udid, format).await?;
        }

        if target == "ios-simulator" || target == "visionos-simulator" {
            launch_ios_simulator(&app_dir, &bundle_id, udid, format)
        } else {
            launch_ios_device(&app_dir, &bundle_id, udid, format)
        }
    } else if target == "android" {
        let serial = device_udid.ok_or_else(|| anyhow!("No Android device serial — use perry run android with a connected device or emulator"))?;
        install_and_launch_android(&dest, &bundle_id, &serial, format)
    } else {
        // Native binary
        launch_native(&dest, program_args, format)
    }
}

/// Extract .app bundle from an .ipa file
/// Embed app icon into the .app bundle if missing.
/// Reads icon path from perry.toml [project].icons.source or package.json perry.icon,
/// converts to the required iOS icon sizes using sips, and adds CFBundleIcons to Info.plist.
/// Copy resource directories (logo/, assets/, resources/, images/) from the project
/// into the .app bundle so ImageFile() references resolve at runtime.
fn bundle_project_resources(app_dir: &Path, project_root: &Path) {
    for dir_name in &["logo", "assets", "resources", "images"] {
        let src = project_root.join(dir_name);
        if src.is_dir() {
            let dest = app_dir.join(dir_name);
            let _ = copy_dir_recursive(&src, &dest);
        }
    }
}

/// Recursively copy a directory
fn copy_dir_recursive(src: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else {
            std::fs::copy(&src_path, &dest_path)?;
        }
    }
    Ok(())
}

fn embed_app_icon(app_dir: &Path, project_root: &Path) {
    // Skip if icons already exist
    if app_dir.join("AppIcon60x60@2x.png").exists() || app_dir.join("Assets.car").exists() {
        return;
    }

    // Find icon source from perry.toml or package.json
    let icon_path = find_icon_source(project_root);
    let icon_path = match icon_path {
        Some(p) if p.exists() => p,
        _ => return,
    };

    // Generate required iOS icon sizes
    let sizes = [
        ("AppIcon60x60@2x.png", 120),
        ("AppIcon60x60@3x.png", 180),
        ("AppIcon76x76@2x.png", 152),
        ("AppIcon83.5x83.5@2x.png", 167),
    ];

    for (name, size) in &sizes {
        let dest = app_dir.join(name);
        let _ = Command::new("sips")
            .args([
                "-z", &size.to_string(), &size.to_string(),
                "--setProperty", "format", "png",
            ])
            .arg(&icon_path)
            .args(["--out"])
            .arg(&dest)
            .output();
    }

    // Update Info.plist to reference the icons
    let info_plist = app_dir.join("Info.plist");
    let _ = Command::new("/usr/libexec/PlistBuddy")
        .args(["-c", "Add :CFBundleIcons dict"])
        .arg(&info_plist)
        .output();
    let _ = Command::new("/usr/libexec/PlistBuddy")
        .args(["-c", "Add :CFBundleIcons:CFBundlePrimaryIcon dict"])
        .arg(&info_plist)
        .output();
    let _ = Command::new("/usr/libexec/PlistBuddy")
        .args(["-c", "Add :CFBundleIcons:CFBundlePrimaryIcon:CFBundleIconFiles array"])
        .arg(&info_plist)
        .output();
    let _ = Command::new("/usr/libexec/PlistBuddy")
        .args(["-c", "Add :CFBundleIcons:CFBundlePrimaryIcon:CFBundleIconFiles:0 string AppIcon60x60"])
        .arg(&info_plist)
        .output();
    let _ = Command::new("/usr/libexec/PlistBuddy")
        .args(["-c", "Add :CFBundleIcons:CFBundlePrimaryIcon:CFBundleIconFiles:1 string AppIcon76x76"])
        .arg(&info_plist)
        .output();
    let _ = Command::new("/usr/libexec/PlistBuddy")
        .args(["-c", "Add :CFBundleIcons:CFBundlePrimaryIcon:CFBundleIconFiles:2 string AppIcon83.5x83.5"])
        .arg(&info_plist)
        .output();
}

/// Find the icon source file from project config
fn find_icon_source(project_root: &Path) -> Option<PathBuf> {
    // Check perry.toml [project].icons.source
    let toml_path = project_root.join("perry.toml");
    if let Ok(content) = std::fs::read_to_string(&toml_path) {
        if let Ok(config) = content.parse::<toml::Value>() {
            if let Some(source) = config
                .get("project")
                .and_then(|p| p.get("icons"))
                .and_then(|i| i.get("source"))
                .and_then(|s| s.as_str())
            {
                return Some(project_root.join(source));
            }
        }
    }

    // Check package.json perry.icon
    let pkg_path = project_root.join("package.json");
    if let Ok(content) = std::fs::read_to_string(&pkg_path) {
        if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(icon) = pkg.get("perry").and_then(|p| p.get("icon")).and_then(|i| i.as_str()) {
                return Some(project_root.join(icon));
            }
        }
    }

    None
}

fn extract_app_from_ipa(ipa_path: &Path, dest_dir: &Path) -> Result<PathBuf> {
    use std::io::Read;

    let file = std::fs::File::open(ipa_path).context("Failed to open .ipa")?;
    let mut archive = zip::ZipArchive::new(file).context("Failed to read .ipa as ZIP")?;

    // .ipa structure: Payload/<AppName>.app/...
    let mut app_name = None;
    for i in 0..archive.len() {
        let entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        if name.starts_with("Payload/") && name.ends_with(".app/") {
            // Extract the .app directory name
            let parts: Vec<&str> = name.split('/').collect();
            if parts.len() >= 2 {
                app_name = Some(parts[1].to_string());
                break;
            }
        }
    }

    let app_name = app_name.ok_or_else(|| anyhow!("No .app found in .ipa"))?;
    let app_dir = dest_dir.join(&app_name);
    let _ = std::fs::remove_dir_all(&app_dir); // clean previous

    // Extract all files under Payload/<app_name>/
    let prefix = format!("Payload/{}/", app_name);
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        if let Some(rel) = name.strip_prefix(&prefix) {
            if rel.is_empty() {
                continue;
            }
            let out_path = app_dir.join(rel);
            if name.ends_with('/') {
                std::fs::create_dir_all(&out_path)?;
            } else {
                if let Some(parent) = out_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut out_file = std::fs::File::create(&out_path)?;
                std::io::copy(&mut entry, &mut out_file)?;
            }
        }
    }

    // Make the main executable... executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Find the executable inside the .app (same name as app, without .app)
        let exe_name = app_name.strip_suffix(".app").unwrap_or(&app_name);
        let exe_path = app_dir.join(exe_name);
        if exe_path.exists() {
            let _ = std::fs::set_permissions(&exe_path, std::fs::Permissions::from_mode(0o755));
        }
    }

    Ok(app_dir)
}

/// Re-sign an .app bundle for development device installs.
///
/// Searches for an existing dev provisioning profile, or creates one via
/// the App Store Connect API (registers device, creates App ID + profile).
/// Then re-signs with a local Apple Development identity.
async fn resign_for_development(
    app_dir: &Path,
    config: &super::publish::PerryConfig,
    device_udid: &str,
    format: OutputFormat,
) -> Result<()> {
    // Read bundle ID from Info.plist
    let bundle_id = read_bundle_id_from_app(app_dir)
        .unwrap_or_else(|| "com.perry.app".to_string());

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
            if q2 <= q1 { return None; }
            let name = line[q1 + 1..q2].to_string();
            if !name.starts_with("Apple Development") && !name.starts_with("iPhone Developer") {
                return None;
            }
            let after_paren = line.find(") ").map(|i| i + 2).unwrap_or(0);
            let hash_end = line.find(" \"").unwrap_or(line.len());
            if hash_end <= after_paren { return None; }
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
    let team_id = config.apple.as_ref()
        .and_then(|a| a.team_id.clone())
        .ok_or_else(|| anyhow!(
            "No Apple team ID in ~/.perry/config.toml — run `perry setup ios` first"
        ))?;

    // Pick the identity that belongs to our team by checking TeamIdentifier
    // via a test codesign. The cert ID in the name (e.g. RY57F22743) is NOT
    // the team ID — we must verify which hash produces the right TeamIdentifier.
    let identity_hash = find_identity_for_team(&dev_identities, &team_id)
        .ok_or_else(|| anyhow!(
            "No Apple Development certificate for team {team_id} found in Keychain.\n\
             Use Xcode to set up development signing for this team."
        ))?;
    let identity = dev_identities.iter()
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
            println!("  Using existing dev profile: {}", style(path.display()).dim());
        }
        std::fs::read(&path)?
    } else {
        // Create via App Store Connect API
        if let OutputFormat::Text = format {
            println!("  Creating development provisioning profile via App Store Connect...");
        }
        create_dev_profile_via_api(config, &bundle_id, &team_id, device_udid, format).await
            .context(
                "Could not create development provisioning profile.\n\
                 Ensure your App Store Connect API key has the right permissions,\n\
                 or use a simulator instead: perry run ios --simulator <UDID>"
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

/// Search system provisioning profile directories for a development profile
/// Find the signing identity hash that belongs to the given team ID.
/// Signs a temp file with each identity and checks the resulting TeamIdentifier.
fn find_identity_for_team(identities: &[(String, String)], team_id: &str) -> Option<String> {
    let tmp = std::env::temp_dir().join("perry_team_check");
    let _ = std::fs::write(&tmp, b"x");

    for (hash, _name) in identities {
        let sign = Command::new("codesign")
            .args(["--force", "--sign", hash])
            .arg(&tmp)
            .output();
        if sign.map(|o| o.status.success()).unwrap_or(false) {
            let verify = Command::new("codesign")
                .args(["-dvv"])
                .arg(&tmp)
                .output();
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

fn find_system_dev_profile(bundle_id: &str, team_id: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let profile_dirs = [
        home.join("Library/MobileDevice/Provisioning Profiles"),
        home.join(".perry"),
    ];

    for dir in &profile_dirs {
        if !dir.exists() { continue; }
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("mobileprovision") {
                    continue;
                }
                if let Ok(output) = Command::new("security")
                    .args(["cms", "-D", "-i"]).arg(&path).output()
                {
                    if output.status.success() {
                        let c = String::from_utf8_lossy(&output.stdout);
                        let is_dev = c.contains("<key>ProvisionedDevices</key>")
                            || c.contains("<key>get-task-allow</key>\n\t\t<true/>");
                        let matches = (c.contains(bundle_id) || c.contains(&format!("{team_id}.*")))
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
async fn create_dev_profile_via_api(
    config: &super::publish::PerryConfig,
    bundle_id: &str,
    team_id: &str,
    device_udid: &str,
    format: OutputFormat,
) -> Result<Vec<u8>> {
    let apple = config.apple.as_ref()
        .ok_or_else(|| anyhow!("No Apple credentials in ~/.perry/config.toml — run `perry setup ios` first"))?;

    let key_id = apple.key_id.as_deref()
        .ok_or_else(|| anyhow!("Missing apple.key_id in config"))?;
    let issuer_id = apple.issuer_id.as_deref()
        .ok_or_else(|| anyhow!("Missing apple.issuer_id in config"))?;
    let p8_path = apple.p8_key_path.as_deref()
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
    let device_name = format!("Perry Dev Device {}", &device_udid[..8.min(device_udid.len())]);
    let _ = client.post(format!("{base}/devices"))
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
        .send().await;
    if let OutputFormat::Text = format { println!(" done"); }

    // 2. Find or create App ID (bundleId)
    if let OutputFormat::Text = format {
        print!("    Resolving App ID...");
        std::io::Write::flush(&mut std::io::stdout()).ok();
    }
    let resp = client.get(format!("{base}/bundleIds"))
        .bearer_auth(&token)
        .query(&[("filter[identifier]", bundle_id)])
        .send().await
        .context("Failed to query bundleIds")?;
    let body: serde_json::Value = resp.json().await?;

    let bundle_id_resource_id = if let Some(first) = body["data"].as_array().and_then(|a| a.first()) {
        first["id"].as_str().unwrap_or("").to_string()
    } else {
        // Create App ID
        let app_name = bundle_id.split('.').last().unwrap_or("app");
        let resp = client.post(format!("{base}/bundleIds"))
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
            .send().await
            .context("Failed to create bundleId")?;
        let body: serde_json::Value = resp.json().await?;
        body["data"]["id"].as_str().unwrap_or("").to_string()
    };
    if bundle_id_resource_id.is_empty() {
        bail!("Could not resolve App ID for {bundle_id}");
    }
    if let OutputFormat::Text = format { println!(" done"); }

    // 3. Find a development certificate
    if let OutputFormat::Text = format {
        print!("    Finding development certificate...");
        std::io::Write::flush(&mut std::io::stdout()).ok();
    }
    let resp = client.get(format!("{base}/certificates"))
        .bearer_auth(&token)
        .query(&[("filter[certificateType]", "IOS_DEVELOPMENT,DEVELOPMENT")])
        .send().await
        .context("Failed to query certificates")?;
    let body: serde_json::Value = resp.json().await?;

    let cert_ids: Vec<String> = body["data"].as_array()
        .map(|arr| arr.iter().filter_map(|c| c["id"].as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    if cert_ids.is_empty() {
        bail!("No iOS development certificates found in your Apple Developer account");
    }
    if let OutputFormat::Text = format { println!(" done ({})", cert_ids.len()); }

    // 4. Get all registered device IDs
    let resp = client.get(format!("{base}/devices"))
        .bearer_auth(&token)
        .query(&[("filter[platform]", "IOS"), ("limit", "200")])
        .send().await
        .context("Failed to query devices")?;
    let body: serde_json::Value = resp.json().await?;
    let device_ids: Vec<String> = body["data"].as_array()
        .map(|arr| arr.iter().filter_map(|d| d["id"].as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();

    // 5. Create the provisioning profile
    if let OutputFormat::Text = format {
        print!("    Creating development profile...");
        std::io::Write::flush(&mut std::io::stdout()).ok();
    }

    let cert_relationships: Vec<serde_json::Value> = cert_ids.iter()
        .map(|id| serde_json::json!({"type": "certificates", "id": id}))
        .collect();
    let device_relationships: Vec<serde_json::Value> = device_ids.iter()
        .map(|id| serde_json::json!({"type": "devices", "id": id}))
        .collect();

    let profile_name = format!("Perry Dev - {bundle_id}");
    let resp = client.post(format!("{base}/profiles"))
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
        .send().await
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

    if let OutputFormat::Text = format { println!(" done"); }

    // Save for future use
    if let Some(home) = dirs::home_dir() {
        let save_path = home.join(".perry").join(format!("{}_dev.mobileprovision", bundle_id.replace('.', "_")));
        let _ = std::fs::write(&save_path, &profile_data);
    }

    Ok(profile_data)
}

/// Generate a JWT for App Store Connect API authentication
fn generate_asc_jwt(key_id: &str, issuer_id: &str, p8_key: &str) -> Result<String> {
    use jsonwebtoken::{encode, EncodingKey, Header, Algorithm};

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

    let key = EncodingKey::from_ec_pem(p8_key.as_bytes())
        .context("Failed to parse .p8 key")?;

    encode(&header, &claims, &key)
        .context("Failed to generate JWT")
}

/// Find project root by walking up from a directory
/// Read CFBundleIdentifier from an .app's Info.plist
fn read_bundle_id_from_app(app_dir: &Path) -> Option<String> {
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

fn find_project_root(start: &Path) -> PathBuf {
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
fn read_app_metadata(project_root: &Path, input: &Path) -> (String, String) {
    // Check perry.toml first (has [ios].bundle_id, [project].name)
    let toml_path = project_root.join("perry.toml");
    let toml_config = std::fs::read_to_string(&toml_path)
        .ok()
        .and_then(|s| s.parse::<toml::Value>().ok());

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
                .or_else(|| t.get("ios")
                .and_then(|i| i.get("bundle_id"))
                )
                .or_else(|| t.get("macos").and_then(|m| m.get("bundle_id")))
                .or_else(|| t.get("app").and_then(|a| a.get("bundle_id")))
                .or_else(|| t.get("project").and_then(|p| p.get("bundle_id")))
                .or_else(|| t.get("bundle_id"))
        })
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

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
        .map(|s| s.to_string());

    let name = toml_name
        .or(pkg_name)
        .unwrap_or_else(|| {
            input.file_stem().and_then(|s| s.to_str()).unwrap_or("app").to_string()
        });

    let bundle_id = toml_bundle_id
        .or(pkg_bundle_id)
        .unwrap_or_else(|| format!("com.perry.{}", name));

    (name, bundle_id)
}

/// Read iOS-specific config from perry.toml
fn read_perry_toml_ios(project_root: &Path) -> Option<toml::Value> {
    let toml_path = project_root.join("perry.toml");
    let content = std::fs::read_to_string(&toml_path).ok()?;
    let config: toml::Value = content.parse().ok()?;
    config.get("ios").cloned()
}

/// Build signing credentials for physical iOS device builds.
/// Priority: perry.toml [ios] → ~/.perry/config.toml → Keychain auto-detect
fn build_device_credentials(
    config: &super::publish::PerryConfig,
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
fn detect_signing_identity() -> Option<String> {
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
        .or_else(|| identities.iter().find(|n| n.starts_with("iPhone Distribution")))
        .or_else(|| identities.first())
        .cloned()
}

/// Auto-export a .p12 from Keychain for the given identity
fn auto_export_p12(identity: Option<&str>) -> (Option<String>, Option<String>) {
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

    let hash = match hash {
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

/// Find a provisioning profile for the given bundle ID in ~/.perry/
/// Check if a provisioning profile is a development profile (has get-task-allow = true)
fn is_development_profile(path: &Path) -> bool {
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
fn find_development_provisioning_profile(bundle_id: &str) -> Option<String> {
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

// --- Local compilation helpers ---

/// Resolve the entry TypeScript file
fn resolve_entry_file(input: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = input {
        if path.exists() {
            return Ok(path.to_path_buf());
        }
        return Err(anyhow!("File not found: {}", path.display()));
    }

    // Try perry.toml
    if let Some(entry) = read_perry_toml_entry() {
        if entry.exists() {
            return Ok(entry);
        }
    }

    // Fallback: src/main.ts, then main.ts
    for candidate in &["src/main.ts", "main.ts"] {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return Ok(path);
        }
    }

    Err(anyhow!(
        "No input file specified and no main.ts found.\n\
         Usage: perry run <file.ts>\n\
         Or create src/main.ts or main.ts, or set entry in perry.toml"
    ))
}

/// Read entry point from perry.toml if present
fn read_perry_toml_entry() -> Option<PathBuf> {
    let toml_str = std::fs::read_to_string("perry.toml").ok()?;
    for line in toml_str.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("entry") {
            if let Some(eq_pos) = trimmed.find('=') {
                let value = trimmed[eq_pos + 1..].trim().trim_matches('"');
                return Some(PathBuf::from(value));
            }
        }
    }
    None
}

/// Resolve the compilation target and optional device UDID
fn resolve_target(platform: Option<Platform>, args: &RunArgs) -> Result<(Option<String>, Option<String>)> {
    match platform {
        Some(Platform::Web) => Ok((Some("web".to_string()), None)),
        Some(Platform::Android) => {
            let devices = detect_android_devices()?;
            if devices.is_empty() {
                return Err(anyhow!(
                    "No Android devices found. Connect a device or start an emulator, then try again."
                ));
            }
            let serial = if devices.len() == 1 {
                devices[0].udid.clone()
            } else {
                pick_device(&devices, "Android device")?
            };
            Ok((Some("android".to_string()), Some(serial)))
        }
        Some(Platform::Ios) => {
            if let Some(ref udid) = args.simulator {
                return Ok((Some("ios-simulator".to_string()), Some(udid.clone())));
            }
            if let Some(ref udid) = args.device {
                return Ok((Some("ios".to_string()), Some(udid.clone())));
            }

            // Auto-detect: booted simulators + connected devices
            let simulators = detect_booted_simulators().unwrap_or_default();
            let devices = detect_ios_devices().unwrap_or_default();

            let mut all: Vec<(DeviceInfo, &str)> = Vec::new();
            for s in simulators {
                all.push((s, "ios-simulator"));
            }
            for d in devices {
                all.push((d, "ios"));
            }

            if all.is_empty() {
                return Err(anyhow!(
                    "No iOS simulators or devices found.\n\
                     Boot a simulator:  xcrun simctl boot <UDID>\n\
                     Or specify one:    perry run ios --simulator <UDID>"
                ));
            }

            if all.len() == 1 {
                let (dev, target) = all.remove(0);
                return Ok((Some(target.to_string()), Some(dev.udid)));
            }

            // Multiple options: prompt
            let names: Vec<String> = all
                .iter()
                .map(|(d, t)| format!("{} ({})", d.name, t))
                .collect();
            let selection = pick_from_list(&names, "Select iOS target")?;
            let (dev, target) = all.remove(selection);
            Ok((Some(target.to_string()), Some(dev.udid)))
        }
        Some(Platform::Visionos) => {
            if let Some(ref udid) = args.simulator {
                return Ok((Some("visionos-simulator".to_string()), Some(udid.clone())));
            }
            if let Some(ref udid) = args.device {
                return Ok((Some("visionos".to_string()), Some(udid.clone())));
            }

            let simulators = detect_booted_visionos_simulators().unwrap_or_default();

            if simulators.is_empty() {
                return Err(anyhow!(
                    "No Apple Vision Pro simulators found.\n\
                     Boot a simulator:  xcrun simctl boot <UDID>\n\
                     Or specify one:    perry run visionos --simulator <UDID>"
                ));
            }

            if simulators.len() == 1 {
                let dev = simulators.into_iter().next().unwrap();
                return Ok((Some("visionos-simulator".to_string()), Some(dev.udid)));
            }

            let names: Vec<String> = simulators.iter().map(|d| d.name.clone()).collect();
            let selection = pick_from_list(&names, "Select Apple Vision Pro simulator")?;
            let dev = &simulators[selection];
            Ok((Some("visionos-simulator".to_string()), Some(dev.udid.clone())))
        }
        Some(Platform::Watchos) => {
            if let Some(ref udid) = args.simulator {
                return Ok((Some("watchos-simulator".to_string()), Some(udid.clone())));
            }
            if let Some(ref udid) = args.device {
                return Ok((Some("watchos".to_string()), Some(udid.clone())));
            }

            // Auto-detect booted Apple Watch simulators
            let simulators = detect_booted_watch_simulators().unwrap_or_default();

            if simulators.is_empty() {
                return Err(anyhow!(
                    "No Apple Watch simulators found.\n\
                     Boot a simulator:  xcrun simctl boot <UDID>\n\
                     Or specify one:    perry run watchos --simulator <UDID>"
                ));
            }

            if simulators.len() == 1 {
                let dev = simulators.into_iter().next().unwrap();
                return Ok((Some("watchos-simulator".to_string()), Some(dev.udid)));
            }

            let names: Vec<String> = simulators.iter().map(|d| d.name.clone()).collect();
            let selection = pick_from_list(&names, "Select Apple Watch simulator")?;
            let dev = &simulators[selection];
            Ok((Some("watchos-simulator".to_string()), Some(dev.udid.clone())))
        }
        Some(Platform::Tvos) => {
            if let Some(ref udid) = args.simulator {
                return Ok((Some("tvos-simulator".to_string()), Some(udid.clone())));
            }
            if let Some(ref udid) = args.device {
                return Ok((Some("tvos".to_string()), Some(udid.clone())));
            }

            // Auto-detect booted Apple TV simulators
            let simulators = detect_booted_tv_simulators().unwrap_or_default();

            if simulators.is_empty() {
                return Err(anyhow!(
                    "No Apple TV simulators found.\n\
                     Boot a simulator:  xcrun simctl boot <UDID>\n\
                     Or specify one:    perry run tvos --simulator <UDID>"
                ));
            }

            if simulators.len() == 1 {
                let dev = simulators.into_iter().next().unwrap();
                return Ok((Some("tvos-simulator".to_string()), Some(dev.udid)));
            }

            let names: Vec<String> = simulators.iter().map(|d| d.name.clone()).collect();
            let selection = pick_from_list(&names, "Select Apple TV simulator")?;
            let dev = &simulators[selection];
            Ok((Some("tvos-simulator".to_string()), Some(dev.udid.clone())))
        }
        Some(Platform::Macos) | Some(Platform::Linux) | Some(Platform::Windows) => Ok((None, None)),
        None => Ok((None, None)),
    }
}

/// Launch the compiled output based on target
fn launch(
    result: &CompileResult,
    device_udid: Option<&str>,
    program_args: &[String],
    format: OutputFormat,
) -> Result<()> {
    match result.target.as_str() {
        "web" => launch_web(&result.output_path, format),
        "ios-simulator" => {
            let udid =
                device_udid.ok_or_else(|| anyhow!("No simulator UDID — use --simulator <UDID>"))?;
            let bundle_id = result
                .bundle_id
                .as_deref()
                .ok_or_else(|| anyhow!("No bundle ID found for iOS app"))?;
            launch_ios_simulator(&result.output_path, bundle_id, udid, format)
        }
        "ios" => {
            let udid =
                device_udid.ok_or_else(|| anyhow!("No device UDID — use --device <UDID>"))?;
            let bundle_id = result
                .bundle_id
                .as_deref()
                .ok_or_else(|| anyhow!("No bundle ID found for iOS app"))?;
            launch_ios_device(&result.output_path, bundle_id, udid, format)
        }
        "visionos-simulator" => {
            let udid =
                device_udid.ok_or_else(|| anyhow!("No simulator UDID — use --simulator <UDID>"))?;
            let bundle_id = result
                .bundle_id
                .as_deref()
                .ok_or_else(|| anyhow!("No bundle ID found for visionOS app"))?;
            launch_ios_simulator(&result.output_path, bundle_id, udid, format)
        }
        "visionos" => {
            let udid =
                device_udid.ok_or_else(|| anyhow!("No device UDID — use --device <UDID>"))?;
            let bundle_id = result
                .bundle_id
                .as_deref()
                .ok_or_else(|| anyhow!("No bundle ID found for visionOS app"))?;
            launch_ios_device(&result.output_path, bundle_id, udid, format)
        }
        "watchos-simulator" => {
            let udid =
                device_udid.ok_or_else(|| anyhow!("No simulator UDID — use --simulator <UDID>"))?;
            let bundle_id = result
                .bundle_id
                .as_deref()
                .ok_or_else(|| anyhow!("No bundle ID found for watchOS app"))?;
            // Reuse iOS simulator launch — simctl install/launch works the same for watchOS
            launch_ios_simulator(&result.output_path, bundle_id, udid, format)
        }
        "tvos-simulator" => {
            let udid =
                device_udid.ok_or_else(|| anyhow!("No simulator UDID — use --simulator <UDID>"))?;
            let bundle_id = result
                .bundle_id
                .as_deref()
                .ok_or_else(|| anyhow!("No bundle ID found for tvOS app"))?;
            // Reuse iOS simulator launch — simctl install/launch works the same for tvOS
            launch_ios_simulator(&result.output_path, bundle_id, udid, format)
        }
        "android" => {
            let bundle_id = result
                .bundle_id
                .as_deref()
                .unwrap_or("com.perry.app");
            let serial = device_udid.unwrap_or("");
            build_and_run_android(&result.output_path, bundle_id, serial, format)
        }
        _ => launch_native(&result.output_path, program_args, format),
    }
}

// --- Launch functions ---

/// Launch a native executable
fn launch_native(exe_path: &Path, program_args: &[String], format: OutputFormat) -> Result<()> {
    let exe = if exe_path.is_absolute() {
        exe_path.to_path_buf()
    } else {
        std::env::current_dir()?.join(exe_path)
    };

    if !exe.exists() {
        return Err(anyhow!(
            "Compiled executable not found: {}",
            exe.display()
        ));
    }

    if let OutputFormat::Text = format {
        println!();
        println!("Running {}...", exe_path.display());
        println!();
    }

    let status = Command::new(&exe)
        .args(program_args)
        .status()
        .map_err(|e| anyhow!("Failed to launch {}: {}", exe.display(), e))?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

/// Launch on iOS Simulator: install + launch
fn launch_ios_simulator(
    app_dir: &Path,
    bundle_id: &str,
    udid: &str,
    format: OutputFormat,
) -> Result<()> {
    if let OutputFormat::Text = format {
        println!();
        println!("Installing on simulator {}...", udid);
    }

    let install = Command::new("xcrun")
        .args(["simctl", "install", udid])
        .arg(app_dir)
        .status()
        .map_err(|e| anyhow!("Failed to run xcrun simctl install: {}", e))?;

    if !install.success() {
        return Err(anyhow!("Failed to install app on simulator {}", udid));
    }

    if let OutputFormat::Text = format {
        println!("Launching {}...", bundle_id);
        println!();
    }

    let launch = Command::new("xcrun")
        .args(["simctl", "launch", "--console-pty", udid, bundle_id])
        .status()
        .map_err(|e| anyhow!("Failed to run xcrun simctl launch: {}", e))?;

    if !launch.success() {
        return Err(anyhow!("App exited with error on simulator"));
    }
    Ok(())
}

/// Launch on a physical iOS device via devicectl (Xcode 15+)
fn launch_ios_device(
    app_dir: &Path,
    bundle_id: &str,
    udid: &str,
    format: OutputFormat,
) -> Result<()> {
    if let OutputFormat::Text = format {
        println!();
        println!("Installing on device {}...", udid);
    }

    let install = Command::new("xcrun")
        .args(["devicectl", "device", "install", "app", "--device", udid])
        .arg(app_dir)
        .status()
        .map_err(|e| anyhow!("Failed to run xcrun devicectl install: {}", e))?;

    if !install.success() {
        return Err(anyhow!("Failed to install app on device {}", udid));
    }

    if let OutputFormat::Text = format {
        println!("Launching {}...", bundle_id);
        println!();
    }

    let launch = Command::new("xcrun")
        .args([
            "devicectl",
            "device",
            "process",
            "launch",
            "--console",
            "--device",
            udid,
            bundle_id,
        ])
        .status()
        .map_err(|e| anyhow!("Failed to run xcrun devicectl launch: {}", e))?;

    if !launch.success() {
        return Err(anyhow!("App exited with error on device"));
    }
    Ok(())
}

/// Build an Android APK from the compiled .so and install/launch on a device.
///
/// Steps:
/// 1. Copy the Gradle template from perry-ui-android/template/ to a temp dir
/// 2. Place the compiled .so in app/src/main/jniLibs/arm64-v8a/
/// 3. Update the applicationId in build.gradle.kts
/// 4. Run ./gradlew assembleDebug
/// 5. Sign and install the resulting APK
fn build_and_run_android(
    so_path: &Path,
    bundle_id: &str,
    serial: &str,
    format: OutputFormat,
) -> Result<()> {
    // Find the perry workspace root to locate the Android template
    let workspace_root = super::compile::find_perry_workspace_root()
        .ok_or_else(|| anyhow!("Cannot find Perry workspace root — needed for Android template"))?;
    let template_dir = workspace_root.join("crates/perry-ui-android/template");
    if !template_dir.exists() {
        bail!("Android template not found at {}", template_dir.display());
    }

    // Create a build directory alongside the .so
    let build_dir = so_path.parent().unwrap_or(Path::new(".")).join("android-build");
    if build_dir.exists() {
        std::fs::remove_dir_all(&build_dir).ok();
    }

    if let OutputFormat::Text = format {
        println!();
        println!("Building Android APK...");
    }

    // Copy template to build directory
    copy_dir_recursive(&template_dir, &build_dir)
        .map_err(|e| anyhow!("Failed to copy Android template: {}", e))?;

    // Create jniLibs directory and copy .so
    let jni_dir = build_dir.join("app/src/main/jniLibs/arm64-v8a");
    std::fs::create_dir_all(&jni_dir)?;
    std::fs::copy(so_path, jni_dir.join("libperry_app.so"))
        .map_err(|e| anyhow!("Failed to copy .so to jniLibs: {}", e))?;

    // Copy resource directories (assets/, logo/, etc.) into APK assets
    // Android loads ImageFile('assets/foo.png') from the APK's assets/ directory,
    // so we need 'assets/' inside 'app/src/main/assets/' → accessible as 'assets/foo.png'
    let project_root = so_path.parent().unwrap_or(Path::new("."));
    let apk_assets = build_dir.join("app/src/main/assets");
    std::fs::create_dir_all(&apk_assets)?;
    for dir_name in &["logo", "assets", "resources", "images"] {
        let resource_dir = project_root.join(dir_name);
        if resource_dir.is_dir() {
            let dest = apk_assets.join(dir_name);
            let _ = copy_dir_recursive(&resource_dir, &dest);
        }
    }

    // Update applicationId in build.gradle.kts
    let gradle_path = build_dir.join("app/build.gradle.kts");
    if gradle_path.exists() {
        let content = std::fs::read_to_string(&gradle_path)?;
        let updated = content.replace(
            "applicationId = \"com.perry.template\"",
            &format!("applicationId = \"{}\"", bundle_id),
        );
        std::fs::write(&gradle_path, updated)?;
    }

    // Generate gradle wrapper if not present
    let gradlew = build_dir.join("gradlew");
    if !gradlew.exists() {
        if let OutputFormat::Text = format {
            println!("Generating Gradle wrapper...");
        }
        let wrapper_status = Command::new("gradle")
            .arg("wrapper")
            .current_dir(&build_dir)
            .status();
        match wrapper_status {
            Ok(s) if s.success() => {}
            _ => bail!(
                "Failed to generate Gradle wrapper. Install Gradle: brew install gradle\n\
                 Or install Android Studio which includes Gradle."
            ),
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if gradlew.exists() {
            let mut perms = std::fs::metadata(&gradlew)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&gradlew, perms)?;
        }
    }

    if let OutputFormat::Text = format {
        println!("Running Gradle assembleDebug...");
    }

    // Run gradle build
    let gradle_status = Command::new(&gradlew)
        .arg("assembleDebug")
        .current_dir(&build_dir)
        .status()
        .map_err(|e| anyhow!("Failed to run Gradle: {}. Is the Android SDK/Gradle installed?", e))?;

    if !gradle_status.success() {
        bail!("Gradle build failed. Check the output above for errors.");
    }

    // Find the APK
    let apk_path = build_dir.join("app/build/outputs/apk/debug/app-debug.apk");
    if !apk_path.exists() {
        bail!("APK not found at expected path: {}", apk_path.display());
    }

    if let OutputFormat::Text = format {
        println!("APK built: {}", apk_path.display());
    }

    // Install and launch
    install_and_launch_android(&apk_path, bundle_id, serial, format)
}

/// Sign an unsigned APK with the Android debug keystore for local testing.
/// Creates the debug keystore if it doesn't exist.
fn debug_sign_apk(apk_path: &Path, format: OutputFormat) -> Result<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let debug_keystore = PathBuf::from(&home).join(".android/debug.keystore");

    // Create debug keystore if it doesn't exist
    if !debug_keystore.exists() {
        if let Some(parent) = debug_keystore.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let status = Command::new("keytool")
            .args([
                "-genkeypair", "-v",
                "-keystore", &debug_keystore.to_string_lossy(),
                "-storepass", "android",
                "-alias", "androiddebugkey",
                "-keypass", "android",
                "-keyalg", "RSA",
                "-keysize", "2048",
                "-validity", "10000",
                "-dname", "CN=Android Debug,O=Android,C=US",
            ])
            .status()
            .map_err(|e| anyhow!("keytool not found: {}", e))?;
        if !status.success() {
            bail!("Failed to create debug keystore");
        }
    }

    if let OutputFormat::Text = format {
        println!("Signing APK with debug key...");
    }

    // Find apksigner from the Android SDK
    let android_home = std::env::var("ANDROID_HOME")
        .or_else(|_| std::env::var("ANDROID_SDK_ROOT"))
        .unwrap_or_else(|_| format!("{}/Library/Android/sdk", home));

    let apksigner = find_apksigner(&android_home);

    // zipalign first (required before signing)
    let aligned_path = apk_path.with_extension("aligned.apk");
    let zipalign = PathBuf::from(&android_home).join("build-tools");
    if let Some(zipalign_bin) = find_latest_build_tool(&zipalign, "zipalign") {
        let status = Command::new(&zipalign_bin)
            .args(["4"])
            .arg(apk_path)
            .arg(&aligned_path)
            .status();
        if let Ok(s) = status {
            if s.success() {
                std::fs::rename(&aligned_path, apk_path).ok();
            }
        }
    }

    // Sign with apksigner
    if let Some(signer) = apksigner {
        let status = Command::new(&signer)
            .args([
                "sign",
                "--ks", &debug_keystore.to_string_lossy(),
                "--ks-pass", "pass:android",
                "--ks-key-alias", "androiddebugkey",
                "--key-pass", "pass:android",
            ])
            .arg(apk_path)
            .status()
            .map_err(|e| anyhow!("apksigner failed: {}", e))?;
        if !status.success() {
            bail!("Failed to sign APK with debug keystore");
        }
    } else {
        // Fallback: use jarsigner
        let status = Command::new("jarsigner")
            .args([
                "-keystore", &debug_keystore.to_string_lossy(),
                "-storepass", "android",
                "-keypass", "android",
                "-signedjar",
            ])
            .arg(apk_path)
            .arg(apk_path)
            .arg("androiddebugkey")
            .status()
            .map_err(|e| anyhow!("jarsigner not found: {}", e))?;
        if !status.success() {
            bail!("Failed to sign APK with debug keystore");
        }
    }

    Ok(apk_path.to_path_buf())
}

/// Find apksigner in the Android SDK build-tools
fn find_apksigner(android_home: &str) -> Option<PathBuf> {
    find_latest_build_tool(&PathBuf::from(android_home).join("build-tools"), "apksigner")
}

/// Find the latest version of a build tool
fn find_latest_build_tool(build_tools_dir: &Path, tool_name: &str) -> Option<PathBuf> {
    let mut versions: Vec<_> = std::fs::read_dir(build_tools_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .collect();
    versions.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    for v in versions {
        let tool = v.path().join(tool_name);
        if tool.exists() {
            return Some(tool);
        }
    }
    None
}

/// Install and launch an APK on an Android device/emulator via adb
fn install_and_launch_android(
    apk_path: &Path,
    bundle_id: &str,
    serial: &str,
    format: OutputFormat,
) -> Result<()> {
    // Debug-sign the APK if unsigned (Android requires signatures for install)
    debug_sign_apk(apk_path, format)?;

    if let OutputFormat::Text = format {
        println!();
        println!("Installing on {}...", serial);
    }

    let install = Command::new("adb")
        .args(["-s", serial, "install", "-r"])
        .arg(apk_path)
        .output()
        .map_err(|e| anyhow!("Failed to run adb install: {}", e))?;

    if !install.status.success() {
        let stderr = String::from_utf8_lossy(&install.stderr);
        let stdout = String::from_utf8_lossy(&install.stdout);
        return Err(anyhow!("Failed to install APK on device {}: {}{}", serial, stderr, stdout));
    }

    if let OutputFormat::Text = format {
        println!("Installed successfully.");
    }

    if let OutputFormat::Text = format {
        println!("Launching {}...", bundle_id);
        println!();
    }

    // Android activity name: use .PerryActivity (from the perry-ui-android template)
    let component = format!("{}/com.perry.app.PerryActivity", bundle_id);

    let launch = Command::new("adb")
        .args(["-s", serial, "shell", "am", "start", "-n", &component])
        .status()
        .map_err(|e| anyhow!("Failed to run adb shell am start: {}", e))?;

    if !launch.success() {
        return Err(anyhow!("Failed to launch app on device {}", serial));
    }

    if let OutputFormat::Text = format {
        println!("App launched. Streaming logs (Ctrl+C to stop)...");
        println!();
    }

    // Stream logcat filtered to the app's package
    // Use logcat's --pid if we can find the PID, otherwise fall back to grep
    std::thread::sleep(std::time::Duration::from_millis(1000));
    let pid = get_android_pid(serial, bundle_id);

    if !pid.is_empty() && pid != "0" {
        let _ = Command::new("adb")
            .args(["-s", serial, "logcat", "--pid", &pid])
            .status();
    } else {
        // Fallback: clear logcat and show all (app may not have started yet)
        let _ = Command::new("adb")
            .args(["-s", serial, "logcat", "-c"])
            .status();
        let _ = Command::new("adb")
            .args(["-s", serial, "logcat"])
            .status();
    }

    Ok(())
}

/// Get the PID of a running Android app
fn get_android_pid(serial: &str, bundle_id: &str) -> String {
    // Try a few times since the app may still be starting
    for _ in 0..3 {
        let output = Command::new("adb")
            .args(["-s", serial, "shell", "pidof", "-s", bundle_id])
            .output();
        match output {
            Ok(o) if o.status.success() => {
                let pid = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if !pid.is_empty() {
                    return pid;
                }
            }
            _ => {}
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    String::new()
}

/// Launch a web build: open HTML in browser
fn launch_web(html_path: &Path, format: OutputFormat) -> Result<()> {
    if let OutputFormat::Text = format {
        println!();
        println!("Opening {} in browser...", html_path.display());
    }

    let cmd = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "start"
    } else {
        "xdg-open"
    };

    Command::new(cmd)
        .arg(html_path)
        .status()
        .map_err(|e| anyhow!("Failed to open browser: {}", e))?;

    Ok(())
}

// --- Device detection ---

/// Detect booted iOS simulators via `xcrun simctl list`
fn detect_booted_simulators() -> Result<Vec<DeviceInfo>> {
    let output = Command::new("xcrun")
        .args(["simctl", "list", "devices", "booted", "--json"])
        .output()
        .map_err(|e| anyhow!("Failed to run xcrun simctl: {}", e))?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).unwrap_or(serde_json::Value::Null);

    let mut devices = Vec::new();
    if let Some(device_map) = json.get("devices").and_then(|d| d.as_object()) {
        for (_runtime, device_list) in device_map {
            if let Some(arr) = device_list.as_array() {
                for dev in arr {
                    let state = dev.get("state").and_then(|s| s.as_str()).unwrap_or("");
                    if state == "Booted" {
                        if let (Some(udid), Some(name)) = (
                            dev.get("udid").and_then(|s| s.as_str()),
                            dev.get("name").and_then(|s| s.as_str()),
                        ) {
                            devices.push(DeviceInfo {
                                udid: udid.to_string(),
                                name: name.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(devices)
}

/// Detect booted Apple Watch simulators
fn detect_booted_visionos_simulators() -> Result<Vec<DeviceInfo>> {
    let output = Command::new("xcrun")
        .args(["simctl", "list", "devices", "booted", "--json"])
        .output()
        .map_err(|e| anyhow!("Failed to run xcrun simctl: {}", e))?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).unwrap_or(serde_json::Value::Null);

    let mut devices = Vec::new();
    if let Some(device_map) = json.get("devices").and_then(|d| d.as_object()) {
        for (runtime, device_list) in device_map {
            if !runtime.contains("visionOS") && !runtime.contains("xrOS") {
                continue;
            }
            if let Some(arr) = device_list.as_array() {
                for dev in arr {
                    let state = dev.get("state").and_then(|s| s.as_str()).unwrap_or("");
                    if state == "Booted" {
                        if let (Some(udid), Some(name)) = (
                            dev.get("udid").and_then(|s| s.as_str()),
                            dev.get("name").and_then(|s| s.as_str()),
                        ) {
                            devices.push(DeviceInfo {
                                udid: udid.to_string(),
                                name: name.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(devices)
}

/// Detect booted Apple Watch simulators
fn detect_booted_watch_simulators() -> Result<Vec<DeviceInfo>> {
    let output = Command::new("xcrun")
        .args(["simctl", "list", "devices", "booted", "--json"])
        .output()
        .map_err(|e| anyhow!("Failed to run xcrun simctl: {}", e))?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).unwrap_or(serde_json::Value::Null);

    let mut devices = Vec::new();
    if let Some(device_map) = json.get("devices").and_then(|d| d.as_object()) {
        for (runtime, device_list) in device_map {
            // Only include watchOS runtimes
            if !runtime.contains("watchOS") && !runtime.contains("WatchOS") {
                continue;
            }
            if let Some(arr) = device_list.as_array() {
                for dev in arr {
                    let state = dev.get("state").and_then(|s| s.as_str()).unwrap_or("");
                    if state == "Booted" {
                        if let (Some(udid), Some(name)) = (
                            dev.get("udid").and_then(|s| s.as_str()),
                            dev.get("name").and_then(|s| s.as_str()),
                        ) {
                            devices.push(DeviceInfo {
                                udid: udid.to_string(),
                                name: name.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(devices)
}

/// Auto-detect booted Apple TV simulators via xcrun simctl
fn detect_booted_tv_simulators() -> Result<Vec<DeviceInfo>> {
    let output = Command::new("xcrun")
        .args(["simctl", "list", "devices", "booted", "--json"])
        .output()
        .map_err(|e| anyhow!("Failed to run xcrun simctl: {}", e))?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).unwrap_or(serde_json::Value::Null);

    let mut devices = Vec::new();
    if let Some(device_map) = json.get("devices").and_then(|d| d.as_object()) {
        for (runtime, device_list) in device_map {
            // Only include tvOS runtimes
            if !runtime.contains("tvOS") && !runtime.contains("AppleTV") {
                continue;
            }
            if let Some(arr) = device_list.as_array() {
                for dev in arr {
                    let state = dev.get("state").and_then(|s| s.as_str()).unwrap_or("");
                    if state == "Booted" {
                        if let (Some(udid), Some(name)) = (
                            dev.get("udid").and_then(|s| s.as_str()),
                            dev.get("name").and_then(|s| s.as_str()),
                        ) {
                            devices.push(DeviceInfo {
                                udid: udid.to_string(),
                                name: name.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(devices)
}

/// Detect connected iOS devices via `xcrun devicectl` (Xcode 15+)
fn detect_ios_devices() -> Result<Vec<DeviceInfo>> {
    let output = Command::new("xcrun")
        .args(["devicectl", "list", "devices", "--json-output", "-"])
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return Ok(Vec::new()),
    };

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).unwrap_or(serde_json::Value::Null);

    let mut devices = Vec::new();
    if let Some(arr) = json
        .get("result")
        .and_then(|r| r.get("devices"))
        .and_then(|d| d.as_array())
    {
        for dev in arr {
            let connected = dev
                .get("connectionProperties")
                .and_then(|c| c.get("transportType"))
                .and_then(|t| t.as_str())
                .is_some();
            if connected {
                if let Some(udid) = dev.get("identifier").and_then(|s| s.as_str()) {
                    let name = dev
                        .get("deviceProperties")
                        .and_then(|p| p.get("name"))
                        .and_then(|n| n.as_str())
                        .unwrap_or("iOS Device");
                    devices.push(DeviceInfo {
                        udid: udid.to_string(),
                        name: name.to_string(),
                    });
                }
            }
        }
    }

    Ok(devices)
}

/// Detect connected Android devices via `adb devices`
fn detect_android_devices() -> Result<Vec<DeviceInfo>> {
    let output = Command::new("adb")
        .args(["devices", "-l"])
        .output()
        .map_err(|_| anyhow!("adb not found. Install Android SDK platform-tools."))?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut devices = Vec::new();

    for line in stdout.lines().skip(1) {
        let line = line.trim();
        if line.is_empty() || line.starts_with('*') {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 && parts[1] == "device" {
            let serial = parts[0].to_string();
            let name = parts
                .iter()
                .find(|p| p.starts_with("model:"))
                .map(|p| p.trim_start_matches("model:").to_string())
                .unwrap_or_else(|| serial.clone());
            devices.push(DeviceInfo {
                udid: serial,
                name,
            });
        }
    }

    Ok(devices)
}

// --- Interactive prompts ---

/// Pick a device from a list using dialoguer, or auto-select if non-interactive
fn pick_device(devices: &[DeviceInfo], label: &str) -> Result<String> {
    let names: Vec<String> = devices
        .iter()
        .map(|d| format!("{} ({})", d.name, d.udid))
        .collect();
    let idx = pick_from_list(&names, &format!("Select {}", label))?;
    Ok(devices[idx].udid.clone())
}

/// Interactive selection from a list of options
fn pick_from_list(items: &[String], prompt: &str) -> Result<usize> {
    if items.is_empty() {
        return Err(anyhow!("No options available"));
    }

    // Non-interactive: pick first
    if !atty::is(atty::Stream::Stdin) {
        eprintln!("Non-interactive terminal, selecting: {}", items[0]);
        return Ok(0);
    }

    let selection = dialoguer::Select::new()
        .with_prompt(prompt)
        .items(items)
        .default(0)
        .interact()
        .map_err(|e| anyhow!("Selection cancelled: {}", e))?;

    Ok(selection)
}
