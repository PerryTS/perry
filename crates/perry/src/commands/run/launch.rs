//! Per-target launch dispatch (native, iOS sim/device, Android, web).

use super::*;

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

    // Execute inside the bundle so Foundation and AppKit see its application
    // identity. Direct execution preserves argv, terminal I/O, and exit status.
    let executable = native_executable_path(&exe)?;
    let status = Command::new(&executable)
        .args(program_args)
        .status()
        .map_err(|e| anyhow!("Failed to launch {}: {}", exe.display(), e))?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

fn native_executable_path(output: &Path) -> Result<PathBuf> {
    if !cfg!(target_os = "macos")
        || !output.is_dir()
        || output.extension().is_none_or(|ext| ext != "app")
    {
        return Ok(output.to_path_buf());
    }
    let plist = output.join("Contents/Info.plist");
    let result = Command::new("/usr/bin/plutil")
        .args(["-extract", "CFBundleExecutable", "raw", "-o", "-"])
        .arg(&plist)
        .output()
        .with_context(|| format!("read application executable from {}", plist.display()))?;
    if !result.status.success() {
        bail!("Cannot read CFBundleExecutable from {}", plist.display());
    }
    let value =
        String::from_utf8(result.stdout).context("application executable name is not UTF-8")?;
    let name = value.strip_suffix('\n').unwrap_or(&value);
    let mut components = Path::new(name).components();
    if !matches!(components.next(), Some(std::path::Component::Normal(_)))
        || components.next().is_some()
    {
        bail!("Invalid CFBundleExecutable in {}", plist.display());
    }
    let executable = output.join("Contents/MacOS").join(name);
    if !executable.is_file() {
        bail!("Application executable not found: {}", executable.display());
    }
    Ok(executable)
}

#[cfg(all(test, target_os = "macos"))]
mod macos_bundle_tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Stdio;

    #[test]
    fn launch_uses_plist_executable_even_after_the_bundle_is_renamed() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("Renamed Application.app");
        std::fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        let binary = bundle.join("Contents/MacOS/Original Engine");
        std::fs::write(&binary, "executable witness").unwrap();
        std::fs::write(
            bundle.join("Contents/Info.plist"),
            r#"<?xml version="1.0"?><plist version="1.0"><dict>
            <key>CFBundleExecutable</key><string>Original Engine</string>
            </dict></plist>"#,
        )
        .unwrap();
        assert_eq!(native_executable_path(&bundle).unwrap(), binary);
        assert_eq!(native_executable_path(&binary).unwrap(), binary);
        std::fs::remove_file(&binary).unwrap();
        assert!(native_executable_path(&bundle).is_err());
    }

    #[test]
    fn launch_bundle_preserves_arguments_terminal_streams_and_exit_status() {
        const CHILD: &str = "PERRY_TEST_BUNDLE_LAUNCH_CHILD";
        if let Some(bundle) = std::env::var_os(CHILD) {
            launch_native(
                Path::new(&bundle),
                &["argument with spaces".into()],
                OutputFormat::Text,
            )
            .unwrap();
            unreachable!("the launched witness exits with status 7");
        }
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("Terminal Witness.app");
        std::fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        let binary = bundle.join("Contents/MacOS/engine");
        std::fs::write(&binary, "#!/bin/sh\nread -r line\nprintf 'argv=%s input=%s\\n' \"$1\" \"$line\"\nprintf 'stderr witness\\n' >&2\nexit 7\n").unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(bundle.join("Contents/Info.plist"),
            "<plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>engine</string></dict></plist>").unwrap();
        let thread = std::thread::current();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                thread.name().unwrap(),
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD, &bundle)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"input witness\n")
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(7));
        assert!(String::from_utf8_lossy(&output.stdout)
            .contains("argv=argument with spaces input=input witness\n"));
        assert!(String::from_utf8_lossy(&output.stderr).contains("stderr witness\n"));
    }
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

    let launch = simctl_launch_command(udid, bundle_id, perry_env())
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

    let launch = devicectl_launch_command(udid, bundle_id, perry_env())
        .status()
        .map_err(|e| anyhow!("Failed to run xcrun devicectl launch: {}", e))?;

    if !launch.success() {
        return Err(anyhow!("App exited with error on device"));
    }
    Ok(())
}

/// The `PERRY_*` variables of this process, sorted. Only this prefix is
/// forwarded: the app runs in its own sandbox with its own PATH and HOME.
fn perry_env() -> Vec<(String, String)> {
    let mut vars: Vec<_> = std::env::vars()
        .filter(|(k, _)| k.starts_with("PERRY_"))
        .collect();
    vars.sort();
    vars
}

/// simctl hands the launched app every variable of its own environment
/// prefixed `SIMCTL_CHILD_`, with the prefix stripped.
fn simctl_launch_command(udid: &str, bundle_id: &str, vars: Vec<(String, String)>) -> Command {
    let mut cmd = Command::new("xcrun");
    cmd.args(["simctl", "launch", "--console-pty", udid, bundle_id]);
    for (key, value) in vars {
        cmd.env(format!("SIMCTL_CHILD_{key}"), value);
    }
    cmd
}

/// devicectl takes the app environment as one JSON object argument.
fn devicectl_launch_command(udid: &str, bundle_id: &str, vars: Vec<(String, String)>) -> Command {
    let mut cmd = Command::new("xcrun");
    cmd.args([
        "devicectl",
        "device",
        "process",
        "launch",
        "--console",
        "--device",
        udid,
    ]);
    if !vars.is_empty() {
        let env: serde_json::Map<_, _> = vars
            .into_iter()
            .map(|(k, v)| (k, serde_json::Value::String(v)))
            .collect();
        cmd.arg("--environment-variables")
            .arg(serde_json::Value::Object(env).to_string());
    }
    cmd.arg(bundle_id);
    cmd
}

#[cfg(test)]
mod apple_launch_env_tests {
    use super::*;
    use std::ffi::OsStr;

    fn vars() -> Vec<(String, String)> {
        vec![
            ("PERRY_UI_TEST_MODE".into(), "1".into()),
            ("PERRY_Q".into(), "a\"b".into()),
        ]
    }

    #[test]
    fn simctl_launch_forwards_perry_vars_with_the_simctl_child_prefix() {
        let cmd = simctl_launch_command("UDID", "com.x.app", vars());
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(
            args,
            ["simctl", "launch", "--console-pty", "UDID", "com.x.app"]
        );
        let envs: Vec<_> = cmd.get_envs().collect();
        assert!(envs.contains(&(
            OsStr::new("SIMCTL_CHILD_PERRY_UI_TEST_MODE"),
            Some(OsStr::new("1"))
        )));
        assert!(envs.contains(&(OsStr::new("SIMCTL_CHILD_PERRY_Q"), Some(OsStr::new("a\"b")))));
    }

    #[test]
    fn devicectl_launch_passes_perry_vars_as_one_json_object_before_the_bundle() {
        let cmd = devicectl_launch_command("UDID", "com.x.app", vars());
        let args: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_str().unwrap().to_string())
            .collect();
        let n = args.len();
        assert_eq!(args[n - 3], "--environment-variables");
        assert_eq!(args[n - 1], "com.x.app");
        let json: serde_json::Value = serde_json::from_str(&args[n - 2]).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"PERRY_UI_TEST_MODE": "1", "PERRY_Q": "a\"b"})
        );
    }

    #[test]
    fn devicectl_launch_without_perry_vars_omits_the_environment_flag() {
        let cmd = devicectl_launch_command("UDID", "com.x.app", Vec::new());
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(
            args,
            [
                "devicectl",
                "device",
                "process",
                "launch",
                "--console",
                "--device",
                "UDID",
                "com.x.app"
            ]
        );
    }
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
