//! Remote build via Perry Hub: tarball upload, WebSocket progress, IPA extraction.

use super::*;

/// Build remotely via Perry Hub and launch the result
pub async fn remote_build_and_launch(
    input: &Path,
    target: &str,
    device_udid: Option<&str>,
    program_args: &[String],
    enable_geisterhand: bool,
    geisterhand_port: Option<u16>,
    format: OutputFormat,
) -> Result<()> {
    use super::super::publish::{
        auto_register_license, create_project_tarball_with_filters, load_config, save_config,
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

    // Read [publish] filters so remote runs use the same archive policy as publish.
    let (publish_excludes, publish_includes) =
        std::fs::read_to_string(project_root.join("perry.toml"))
            .ok()
            .and_then(|s| toml::from_str::<toml::Value>(&s).ok())
            .and_then(|v| v.get("publish").cloned())
            .map(|publish| {
                let strings = |key: &str| {
                    publish
                        .get(key)
                        .and_then(|value| value.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|value| value.as_str().map(String::from))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                };
                (strings("exclude"), strings("include"))
            })
            .unwrap_or_default();
    let tarball =
        create_project_tarball_with_filters(&project_root, &publish_excludes, &publish_includes)
            .context("Failed to create project tarball")?;

    if let OutputFormat::Text = format {
        println!(
            " {} ({:.1} MB)",
            style("done").green(),
            tarball.len() as f64 / 1_048_576.0
        );
    }

    // Build manifest
    let ios_distribute = match target {
        "ios" => "development", // device build needs dev signing, not distribution
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
    let ws_url =
        if build_resp.ws_url.starts_with("ws://") || build_resp.ws_url.starts_with("wss://") {
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
            // #854: part of the deserialized JobCreated wire shape; not read.
            #[allow(dead_code)]
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
                    pb.finish_with_message(format!("✓ Build complete: {}", style(&name).bold()));
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
            }) {
            base64::engine::general_purpose::STANDARD
                .decode(&bytes)
                .unwrap_or_else(|_| bytes.to_vec())
        } else {
            bytes.to_vec()
        };
        std::fs::write(&dest, &data)?;
    }

    if let OutputFormat::Text = format {
        println!(
            " {} → {}",
            style("done").green(),
            style(dest.display()).bold()
        );
        println!();
    }

    // For iOS: extract .app from .ipa and install
    if target == "ios-simulator"
        || target == "ios"
        || target == "visionos-simulator"
        || target == "visionos"
    {
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
        install_and_launch_android(&dest, &bundle_id, serial, format)
    } else {
        // Native binary
        launch_native(&dest, program_args, format)
    }
}

/// Copy resource directories (logo/, assets/, resources/, images/) from the project
/// into the .app bundle so ImageFile() references resolve at runtime.
pub fn bundle_project_resources(app_dir: &Path, project_root: &Path) {
    for dir_name in &["logo", "assets", "resources", "images"] {
        let src = project_root.join(dir_name);
        if src.is_dir() {
            let dest = app_dir.join(dir_name);
            let _ = copy_dir_recursive(&src, &dest);
        }
    }
}

/// Recursively copy a directory
pub fn copy_dir_recursive(src: &Path, dest: &Path) -> std::io::Result<()> {
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

/// Embed app icon into the .app bundle if missing.
/// Reads icon path from perry.toml [project].icons.source or package.json perry.icon,
/// converts to the required iOS icon sizes using sips, and adds CFBundleIcons to Info.plist.
pub fn embed_app_icon(app_dir: &Path, project_root: &Path) {
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
                "-z",
                &size.to_string(),
                &size.to_string(),
                "--setProperty",
                "format",
                "png",
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
        .args([
            "-c",
            "Add :CFBundleIcons:CFBundlePrimaryIcon:CFBundleIconFiles array",
        ])
        .arg(&info_plist)
        .output();
    let _ = Command::new("/usr/libexec/PlistBuddy")
        .args([
            "-c",
            "Add :CFBundleIcons:CFBundlePrimaryIcon:CFBundleIconFiles:0 string AppIcon60x60",
        ])
        .arg(&info_plist)
        .output();
    let _ = Command::new("/usr/libexec/PlistBuddy")
        .args([
            "-c",
            "Add :CFBundleIcons:CFBundlePrimaryIcon:CFBundleIconFiles:1 string AppIcon76x76",
        ])
        .arg(&info_plist)
        .output();
    let _ = Command::new("/usr/libexec/PlistBuddy")
        .args([
            "-c",
            "Add :CFBundleIcons:CFBundlePrimaryIcon:CFBundleIconFiles:2 string AppIcon83.5x83.5",
        ])
        .arg(&info_plist)
        .output();
}

/// Find the icon source file from project config
pub fn find_icon_source(project_root: &Path) -> Option<PathBuf> {
    // Check perry.toml [project].icons.source
    let toml_path = project_root.join("perry.toml");
    if let Ok(content) = std::fs::read_to_string(&toml_path) {
        if let Ok(config) = toml::from_str::<toml::Value>(&content) {
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
            if let Some(icon) = pkg
                .get("perry")
                .and_then(|p| p.get("icon"))
                .and_then(|i| i.as_str())
            {
                return Some(project_root.join(icon));
            }
        }
    }

    None
}

pub fn extract_app_from_ipa(ipa_path: &Path, dest_dir: &Path) -> Result<PathBuf> {
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
