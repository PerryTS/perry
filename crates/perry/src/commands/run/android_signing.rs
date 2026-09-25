//! Android SDK discovery and debug signing, shared by local and downloaded APKs.
use super::*;
use std::ffi::OsStr;

/// Sign an unsigned APK with the Android debug keystore for local testing.
/// Creates the debug keystore if it doesn't exist.
pub fn debug_sign_apk(apk_path: &Path, format: OutputFormat) -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow!("Cannot find home directory for Android debug keystore"))?;
    let android_home =
        sdk_root().ok_or_else(|| anyhow!("Cannot locate Android SDK; set ANDROID_HOME"))?;
    debug_sign_apk_in_sdk(apk_path, format, &home, &android_home, cfg!(windows))
}

fn debug_sign_apk_in_sdk(
    apk_path: &Path,
    format: OutputFormat,
    home: &Path,
    android_home: &Path,
    windows: bool,
) -> Result<PathBuf> {
    let debug_keystore = home.join(".android/debug.keystore");

    // Create debug keystore if it doesn't exist
    if !debug_keystore.exists() {
        if let Some(parent) = debug_keystore.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let status = Command::new("keytool")
            .args([
                "-genkeypair",
                "-v",
                "-keystore",
                &debug_keystore.to_string_lossy(),
                "-storepass",
                "android",
                "-alias",
                "androiddebugkey",
                "-keypass",
                "android",
                "-keyalg",
                "RSA",
                "-keysize",
                "2048",
                "-validity",
                "10000",
                "-dname",
                "CN=Android Debug,O=Android,C=US",
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
    let build_tools = android_home.join("build-tools");
    let apksigner = find_latest_build_tool(&build_tools, "apksigner", windows);
    let zipalign = find_latest_build_tool(&build_tools, "zipalign", windows);

    // An SDK signer needs aligned input. Report discovery/command failures here,
    // rather than continuing to an APK that adb rejects as invalid.
    if apksigner.is_some() && zipalign.is_none() {
        bail!(
            "zipalign not found in {}; install Android SDK build-tools",
            build_tools.display()
        );
    }
    if let Some(zipalign) = zipalign {
        align_apk(apk_path, &zipalign)?;
    }

    // Sign with apksigner
    if let Some(signer) = apksigner {
        sign_with_apksigner(apk_path, &debug_keystore, &signer)?;
    } else {
        // Fallback: use jarsigner
        let status = Command::new("jarsigner")
            .args([
                "-keystore",
                &debug_keystore.to_string_lossy(),
                "-storepass",
                "android",
                "-keypass",
                "android",
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

fn sign_with_apksigner(apk_path: &Path, debug_keystore: &Path, signer: &Path) -> Result<()> {
    // Command handles Windows .bat execution and escaping. Keep each path as
    // its own argument instead of constructing a cmd.exe command string.
    let status = Command::new(signer)
        .arg("sign")
        .arg("--ks")
        .arg(debug_keystore)
        .args([
            "--ks-pass",
            "pass:android",
            "--ks-key-alias",
            "androiddebugkey",
            "--key-pass",
            "pass:android",
        ])
        .arg(apk_path)
        .status()
        .map_err(|e| anyhow!("apksigner failed: {}", e))?;
    if !status.success() {
        bail!("Failed to sign APK with debug keystore");
    }
    Ok(())
}

fn align_apk(apk_path: &Path, zipalign: &Path) -> Result<()> {
    let aligned = apk_path.with_extension("aligned.apk");
    let status = Command::new(zipalign)
        .args(["-f", "4"])
        .arg(apk_path)
        .arg(&aligned)
        .status()
        .with_context(|| format!("Failed to run zipalign at {}", zipalign.display()))?;
    if !status.success() {
        bail!("zipalign failed for {}", apk_path.display());
    }
    std::fs::rename(&aligned, apk_path).context("Failed to replace APK with zipaligned output")?;
    Ok(())
}

fn sdk_root() -> Option<PathBuf> {
    resolve_sdk_root(
        std::env::var_os("ANDROID_HOME").as_deref(),
        std::env::var_os("ANDROID_SDK_ROOT").as_deref(),
        dirs::home_dir().as_deref(),
        std::env::var_os("LOCALAPPDATA").as_deref().map(Path::new),
        if cfg!(windows) {
            "windows"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else {
            "linux"
        },
    )
}

fn resolve_sdk_root(
    android_home: Option<&OsStr>,
    sdk_root: Option<&OsStr>,
    home: Option<&Path>,
    local_app_data: Option<&Path>,
    host: &str,
) -> Option<PathBuf> {
    if let Some(path) = android_home
        .filter(|p| !p.is_empty())
        .or_else(|| sdk_root.filter(|p| !p.is_empty()))
    {
        return Some(PathBuf::from(path));
    }
    match host {
        "windows" => local_app_data
            .filter(|p| !p.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .or_else(|| home.map(|p| p.join("AppData/Local")))
            .map(|p| p.join("Android/Sdk")),
        "macos" => home.map(|p| p.join("Library/Android/sdk")),
        _ => home.map(|p| p.join("Android/Sdk")),
    }
}

fn find_latest_build_tool(
    build_tools_dir: &Path,
    tool_name: &str,
    windows: bool,
) -> Option<PathBuf> {
    let mut versions: Vec<_> = std::fs::read_dir(build_tools_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .collect();
    versions.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    let names = if windows {
        vec![
            format!("{tool_name}.exe"),
            format!("{tool_name}.bat"),
            format!("{tool_name}.cmd"),
            tool_name.to_owned(),
        ]
    } else {
        vec![tool_name.to_owned()]
    };
    for version in versions {
        for name in &names {
            let path = version.path().join(name);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests;
