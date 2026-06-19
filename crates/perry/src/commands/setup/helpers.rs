use anyhow::{bail, Context, Result};
use console::style;
use dialoguer::Input;

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Expand leading `~/` to the user's home directory.
pub fn expand_tilde(path: &str) -> String {
    if path.starts_with("~/") {
        dirs::home_dir()
            .map(|h| h.join(&path[2..]).to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string())
    } else {
        path.to_string()
    }
}

/// Prompt for a writable *output* file path (file doesn't have to exist
/// yet, but its parent directory must exist and be writable by the
/// current user). Re-prompts on failure instead of bailing — typo-tolerant
/// for interactive flows like `perry setup android` where a slipped
/// `~/../foo` would otherwise crash the whole command after the user has
/// already entered an alias + password.
pub fn prompt_output_path(prompt: &str) -> Result<String> {
    loop {
        let raw = Input::<String>::new().with_prompt(prompt).interact_text()?;
        let expanded = expand_tilde(&raw);
        match check_writable_output(&expanded) {
            Ok(()) => return Ok(expanded),
            Err(msg) => {
                println!();
                println!("  {} {msg}", style("✗").red());
                println!("    Try a path inside your home directory, e.g. ~/release-key.keystore");
                println!();
            }
        }
    }
}

/// Probe whether the given output path is writable by the current user:
/// resolves the parent directory (canonicalizing through any `..`
/// components so the error message names the *real* directory the OS
/// will refuse), then attempts to create-and-delete a tiny probe file.
/// Returns a human-readable message on failure rather than a typed error,
/// because the only consumer formats it straight back to the prompt.
fn check_writable_output(path: &str) -> std::result::Result<(), String> {
    if path.trim().is_empty() {
        return Err("Path cannot be empty".to_string());
    }
    let p = std::path::Path::new(path);
    let parent = p
        .parent()
        .filter(|pp| !pp.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let canonical_parent = parent.canonicalize().map_err(|e| {
        format!(
            "Parent directory {} is not reachable: {e}",
            parent.display()
        )
    })?;
    if !canonical_parent.is_dir() {
        return Err(format!(
            "Parent path {} is not a directory",
            canonical_parent.display()
        ));
    }
    let probe = canonical_parent.join(format!(".perry-write-test-{}", std::process::id()));
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            Ok(())
        }
        Err(e) => Err(format!(
            "Cannot write to {} ({})",
            canonical_parent.display(),
            e.kind()
        )),
    }
}

/// Prompt for a file path, validate it exists and has the expected extension.
pub fn prompt_file_path(prompt: &str, expected_ext: &str) -> Result<String> {
    let path = Input::<String>::new().with_prompt(prompt).interact_text()?;
    let path = expand_tilde(&path);
    if !std::path::Path::new(&path).exists() {
        bail!("File not found: {path}");
    }
    if !path.ends_with(expected_ext) {
        bail!("Expected a {expected_ext} file, got: {path}");
    }
    Ok(path)
}

/// Display a "Press Enter to continue" prompt.
pub fn press_enter_to_continue(prompt: &str) {
    let _ = Input::<String>::new()
        .with_prompt(prompt)
        .allow_empty(true)
        .interact_text();
}

/// Update perry.toml [ios] section with project-specific signing credentials.
pub fn update_perry_toml_ios(
    perry_toml_path: &std::path::Path,
    certificate: &str,
    provisioning_profile: &str,
    signing_identity: Option<&str>,
    bundle_id: &str,
) -> Result<()> {
    let content = std::fs::read_to_string(perry_toml_path)?;
    let mut doc = content
        .parse::<toml::Table>()
        .context("Failed to parse perry.toml")?;

    let ios = doc
        .entry("ios")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[ios] in perry.toml is not a table"))?;

    ios.insert("bundle_id".into(), toml::Value::String(bundle_id.into()));
    ios.insert(
        "certificate".into(),
        toml::Value::String(certificate.into()),
    );
    ios.insert(
        "provisioning_profile".into(),
        toml::Value::String(provisioning_profile.into()),
    );
    if let Some(identity) = signing_identity {
        ios.insert(
            "signing_identity".into(),
            toml::Value::String(identity.into()),
        );
    }
    if !ios.contains_key("distribute") {
        ios.insert(
            "distribute".into(),
            toml::Value::String("testflight".into()),
        );
    }

    // Ensure [project] has version and build_number — required for App Store uploads.
    // build_number is auto-incremented by `perry publish` on each upload.
    let project = doc
        .entry("project")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[project] in perry.toml is not a table"))?;
    if !project.contains_key("version") {
        project.insert("version".into(), toml::Value::String("1.0.0".into()));
    }
    if !project.contains_key("build_number") {
        project.insert("build_number".into(), toml::Value::Integer(0));
    }

    let new_content = toml::to_string_pretty(&doc).context("Failed to serialize perry.toml")?;
    std::fs::write(perry_toml_path, new_content)?;
    Ok(())
}

/// Update perry.toml [ios] section with encryption_exempt flag.
pub fn update_perry_toml_encryption_exempt(
    perry_toml_path: &std::path::Path,
    encryption_exempt: bool,
) -> Result<()> {
    update_perry_toml_section_bool(
        perry_toml_path,
        "ios",
        "encryption_exempt",
        encryption_exempt,
    )
}

/// Update a boolean field in a named section of perry.toml.
pub fn update_perry_toml_section_bool(
    perry_toml_path: &std::path::Path,
    section: &str,
    key: &str,
    value: bool,
) -> Result<()> {
    let content = std::fs::read_to_string(perry_toml_path)?;
    let mut doc = content
        .parse::<toml::Table>()
        .context("Failed to parse perry.toml")?;

    let table = doc
        .entry(section)
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[{section}] in perry.toml is not a table"))?;

    table.insert(key.into(), toml::Value::Boolean(value));

    let new_content = toml::to_string_pretty(&doc).context("Failed to serialize perry.toml")?;
    std::fs::write(perry_toml_path, new_content)?;
    Ok(())
}

/// Update perry.toml [android] section with keystore and distribute settings.
pub fn update_perry_toml_android(
    perry_toml_path: &std::path::Path,
    keystore_path: &str,
    key_alias: &str,
    google_play_key: Option<&str>,
) -> Result<()> {
    let content = std::fs::read_to_string(perry_toml_path)?;
    let mut doc = content
        .parse::<toml::Table>()
        .context("Failed to parse perry.toml")?;

    let android = doc
        .entry("android")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[android] in perry.toml is not a table"))?;

    android.insert("keystore".into(), toml::Value::String(keystore_path.into()));
    android.insert("key_alias".into(), toml::Value::String(key_alias.into()));
    if let Some(key) = google_play_key {
        android.insert("google_play_key".into(), toml::Value::String(key.into()));
    }
    if !android.contains_key("distribute") {
        android.insert("distribute".into(), toml::Value::String("playstore".into()));
    }

    // Ensure [project] has version and build_number — required for Play Store uploads.
    // build_number is auto-incremented by `perry publish` on each upload.
    let project = doc
        .entry("project")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[project] in perry.toml is not a table"))?;
    if !project.contains_key("version") {
        project.insert("version".into(), toml::Value::String("1.0.0".into()));
    }
    if !project.contains_key("build_number") {
        project.insert("build_number".into(), toml::Value::Integer(0));
    }

    let new_content = toml::to_string_pretty(&doc).context("Failed to serialize perry.toml")?;
    std::fs::write(perry_toml_path, new_content)?;
    Ok(())
}

/// Update perry.toml [macos] section with project-specific signing credentials.
pub fn update_perry_toml_macos(
    perry_toml_path: &std::path::Path,
    distribute: &str,
    certificate: &str,
    signing_identity: Option<&str>,
    notarize_certificate: Option<&str>,
    notarize_signing_identity: Option<&str>,
    installer_certificate: Option<&str>,
) -> Result<()> {
    let content = std::fs::read_to_string(perry_toml_path)?;
    let mut doc = content
        .parse::<toml::Table>()
        .context("Failed to parse perry.toml")?;

    let macos = doc
        .entry("macos")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[macos] in perry.toml is not a table"))?;

    macos.insert("distribute".into(), toml::Value::String(distribute.into()));
    macos.insert(
        "certificate".into(),
        toml::Value::String(certificate.into()),
    );
    if let Some(identity) = signing_identity {
        macos.insert(
            "signing_identity".into(),
            toml::Value::String(identity.into()),
        );
    }
    if let Some(notarize_cert) = notarize_certificate {
        macos.insert(
            "notarize_certificate".into(),
            toml::Value::String(notarize_cert.into()),
        );
    }
    if let Some(notarize_identity) = notarize_signing_identity {
        macos.insert(
            "notarize_signing_identity".into(),
            toml::Value::String(notarize_identity.into()),
        );
    }
    if let Some(installer_cert) = installer_certificate {
        macos.insert(
            "installer_certificate".into(),
            toml::Value::String(installer_cert.into()),
        );
    }

    let new_content = toml::to_string_pretty(&doc).context("Failed to serialize perry.toml")?;
    std::fs::write(perry_toml_path, new_content)?;
    Ok(())
}
