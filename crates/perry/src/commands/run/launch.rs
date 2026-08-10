//! Per-target launch dispatch (native, iOS sim/device, Android, web).

use super::*;

/// `PERRY_*` variables from the current environment, to forward into an app
/// launched on a simulator or device.
///
/// A bundled app is launched by the system, not inherited from this shell, so
/// without an explicit hand-off none of Perry's runtime knobs reach it —
/// `PERRY_FRAME_STATS=1 perry run --device …` silently collected nothing.
///
/// Scoped to the `PERRY_` prefix deliberately: forwarding the whole environment
/// would drop the developer's `PATH`, `HOME`, credentials and locale into a
/// sandboxed process that has its own.
fn perry_env_passthrough() -> Vec<(String, String)> {
    let mut vars: Vec<(String, String)> = std::env::vars()
        .filter(|(k, _)| k.starts_with("PERRY_"))
        .collect();
    // Deterministic order so a launch command is reproducible and diffable.
    vars.sort();
    vars
}

/// Launch the compiled output based on target
pub fn launch(
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
            let bundle_id = result.bundle_id.as_deref().unwrap_or("com.perry.app");
            let serial = device_udid.unwrap_or("");
            build_and_run_android(&result.output_path, bundle_id, serial, format)
        }
        "wearos" => {
            let bundle_id = result.bundle_id.as_deref().unwrap_or("com.perry.app");
            let serial = device_udid.unwrap_or("");
            build_and_run_wearos(&result.output_path, bundle_id, serial, format)
        }
        _ => launch_native(&result.output_path, program_args, format),
    }
}

/// Launch a native executable
pub fn launch_native(exe_path: &Path, program_args: &[String], format: OutputFormat) -> Result<()> {
    let exe = if exe_path.is_absolute() {
        exe_path.to_path_buf()
    } else {
        std::env::current_dir()?.join(exe_path)
    };

    if !exe.exists() {
        return Err(anyhow!("Compiled executable not found: {}", exe.display()));
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
pub fn launch_ios_simulator(
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

    // `simctl` forwards a variable to the launched app when it is prefixed with
    // `SIMCTL_CHILD_` in simctl's own environment.
    let mut launch_cmd = Command::new("xcrun");
    launch_cmd.args(["simctl", "launch", "--console-pty", udid, bundle_id]);
    for (key, value) in perry_env_passthrough() {
        launch_cmd.env(format!("SIMCTL_CHILD_{key}"), value);
    }

    let launch = launch_cmd
        .status()
        .map_err(|e| anyhow!("Failed to run xcrun simctl launch: {}", e))?;

    if !launch.success() {
        return Err(anyhow!("App exited with error on simulator"));
    }
    Ok(())
}

/// Launch on a physical iOS device via devicectl (Xcode 15+)
pub fn launch_ios_device(
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

    let mut launch_cmd = Command::new("xcrun");
    launch_cmd.args([
        "devicectl",
        "device",
        "process",
        "launch",
        "--console",
        "--device",
        udid,
    ]);

    // `devicectl` takes the app environment as a single JSON object. Built with
    // serde_json rather than string concatenation so a value containing a quote
    // or backslash cannot produce a malformed argument.
    let env_vars = perry_env_passthrough();
    if !env_vars.is_empty() {
        let map: serde_json::Map<String, serde_json::Value> = env_vars
            .into_iter()
            .map(|(k, v)| (k, serde_json::Value::String(v)))
            .collect();
        launch_cmd.arg("--environment-variables");
        launch_cmd.arg(serde_json::Value::Object(map).to_string());
    }

    launch_cmd.arg(bundle_id);

    let launch = launch_cmd
        .status()
        .map_err(|e| anyhow!("Failed to run xcrun devicectl launch: {}", e))?;

    if !launch.success() {
        return Err(anyhow!("App exited with error on device"));
    }
    Ok(())
}

/// Launch a web build: open HTML in browser
pub fn launch_web(html_path: &Path, format: OutputFormat) -> Result<()> {
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
