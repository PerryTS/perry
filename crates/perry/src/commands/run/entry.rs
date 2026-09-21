//! Entry-file resolution + target / device selection for `perry run`.

use super::*;

/// Check if we have the cross-compiled runtime libraries for a target.
/// Use the compiler's lookup so prebuilt installs, compressed archives, and
/// explicit library-directory overrides are available to `run` as well.
pub fn can_compile_locally(target: Option<&str>) -> bool {
    if rust_target_triple(target).is_none() {
        return true; // host build, always available
    }
    crate::commands::compile::find_library("libperry_runtime.a", target).is_some()
}

/// Map perry target names to Rust target triples
pub fn rust_target_triple(target: Option<&str>) -> Option<&'static str> {
    if let Some(android) = crate::commands::compile::android_target::android_target(target) {
        return Some(android.rust_triple);
    }

    match target {
        Some("ios-simulator") => Some("aarch64-apple-ios-sim"),
        Some("ios") => Some("aarch64-apple-ios"),
        Some("visionos-simulator") => Some("aarch64-apple-visionos-sim"),
        Some("visionos") => Some("aarch64-apple-visionos"),
        Some("tvos-simulator") => Some("aarch64-apple-tvos-sim"),
        Some("tvos") => Some("aarch64-apple-tvos"),
        _ => None,
    }
}

/// Resolve the entry TypeScript file.
///
/// Accepts a file, a directory, or nothing:
/// - a file path is returned as-is;
/// - a directory (e.g. `perry run .`) is treated as a project root — its
///   `perry.toml` `entry`, then `src/main.ts`, then `main.ts` is resolved
///   inside it (this is what users expect from a project-level command, and
///   avoids the bare "Is a directory" error from the compiler downstream);
/// - nothing falls back to the current directory the same way.
pub fn resolve_entry_file(input: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = input {
        if path.is_dir() {
            if let Some(entry) = resolve_entry_in_dir(path)? {
                return Ok(entry);
            }
            return Err(anyhow!(
                "'{}' is a directory with no entry point.\n\
                 Looked for: perry.toml `entry`, src/main.ts, main.ts\n\
                 Pass a file (perry run path/to/file.ts) or set `entry` in perry.toml.",
                path.display()
            ));
        }
        if path.exists() {
            return Ok(path.to_path_buf());
        }
        return Err(anyhow!("File not found: {}", path.display()));
    }

    // No argument: resolve against the current directory.
    if let Some(entry) = resolve_entry_in_dir(Path::new("."))? {
        return Ok(entry);
    }

    Err(anyhow!(
        "No input file specified and no main.ts found.\n\
         Usage: perry run <file.ts>\n\
         Or create src/main.ts or main.ts, or set entry in perry.toml"
    ))
}

/// Resolve the entry file inside a project directory, honoring `perry.toml`
/// `entry` first, then the conventional `src/main.ts` / `main.ts` fallbacks.
/// Returns a path that is guaranteed to exist.
fn resolve_entry_in_dir(dir: &Path) -> Result<Option<PathBuf>> {
    // perry.toml `entry` is relative to the project directory.
    if let Some(entry) = read_perry_toml_entry_in(dir)? {
        let resolved = if entry.is_absolute() {
            entry
        } else {
            dir.join(entry)
        };
        if resolved.is_file() {
            return Ok(Some(resolved));
        }
        return Err(anyhow!(
            "perry.toml configures entry '{}', but that file does not exist",
            resolved.display()
        ));
    }

    for candidate in &["src/main.ts", "main.ts"] {
        let path = dir.join(candidate);
        if path.is_file() {
            return Ok(Some(path));
        }
    }

    Ok(None)
}

/// Read the `entry` key from `<dir>/perry.toml` (`[project] entry`, or a
/// top-level `entry`), if present. A present but malformed config is an
/// error: silently falling back to `src/main.ts` would run a different program
/// from the one the project explicitly configured.
fn read_perry_toml_entry_in(dir: &Path) -> Result<Option<PathBuf>> {
    let config_path = dir.join("perry.toml");
    if !config_path.exists() {
        return Ok(None);
    }
    let toml_str = std::fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read {}", config_path.display()))?;
    let config: toml::Value =
        toml::from_str(&toml_str).with_context(|| format!("Invalid {}", config_path.display()))?;
    let entry = config
        .get("project")
        .and_then(|project| project.get("entry"))
        .or_else(|| config.get("entry"));
    match entry {
        None => Ok(None),
        Some(value) => value
            .as_str()
            .map(|entry| Some(PathBuf::from(entry)))
            .ok_or_else(|| anyhow!("{} `entry` must be a string", config_path.display())),
    }
}

/// Resolve the compilation target and optional device UDID
pub fn resolve_target(
    platform: Option<Platform>,
    args: &RunArgs,
) -> Result<(Option<String>, Option<String>)> {
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
        Some(Platform::Wearos) => {
            // Wear OS runs over adb just like a phone — the connected device is
            // a watch or a Wear emulator. Same detection, but filter to actual
            // watches (`ro.build.characteristics` contains `watch`) so a paired
            // phone on the same adb isn't selected. The `wearos` target string
            // routes packaging to the Wear Gradle template at launch.
            let devices: Vec<DeviceInfo> = detect_android_devices()?
                .into_iter()
                .filter(|d| is_wear_os_device(&d.udid))
                .collect();
            if devices.is_empty() {
                return Err(anyhow!(
                    "No Wear OS devices found. Pair a watch over adb or start a Wear OS emulator, then try again.\n\
                     Create one:  avdmanager create avd -n perry_wear \\\n               \
                     -k \"system-images;android-34;android-wear;arm64-v8a\" -d wearos_large_round\n\
                     Boot it:     emulator -avd perry_wear"
                ));
            }
            let serial = if devices.len() == 1 {
                devices[0].udid.clone()
            } else {
                pick_device(&devices, "Wear OS device")?
            };
            Ok((Some("wearos".to_string()), Some(serial)))
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
            Ok((
                Some("visionos-simulator".to_string()),
                Some(dev.udid.clone()),
            ))
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
            Ok((
                Some("watchos-simulator".to_string()),
                Some(dev.udid.clone()),
            ))
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

#[cfg(test)]
mod tests {
    use super::{can_compile_locally, resolve_entry_file, rust_target_triple};

    #[test]
    fn local_runtime_discovery_uses_install_layouts() {
        const CHILD_TARGET: &str = "PERRY_TEST_LOCAL_RUNTIME_TARGET";
        const CHILD_EXPECTED: &str = "PERRY_TEST_LOCAL_RUNTIME_EXPECTED";
        if let Ok(target) = std::env::var(CHILD_TARGET) {
            let expected = std::env::var(CHILD_EXPECTED).unwrap() == "true";
            // Check the fixture is visible to the linker before checking the
            // run command's decision. No Android SDK or device is needed.
            assert_eq!(
                crate::commands::compile::find_library("libperry_runtime.a", Some(&target))
                    .is_some(),
                expected
            );
            assert_eq!(can_compile_locally(Some(&target)), expected);
            return;
        }

        // Re-execute the test binary from a temporary install, with an empty
        // project directory. This exercises current_exe() and isolates the
        // environment overrides from other tests in this process.
        let install = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let executable = std::env::current_exe().unwrap();
        let installed_exe = install.path().join(executable.file_name().unwrap());
        std::fs::copy(&executable, &installed_exe).unwrap();
        let run = |target: &str, expected: bool, override_dir: Option<(&str, &std::path::Path)>| {
            let mut command = std::process::Command::new(&installed_exe);
            command
                .args([
                    "--exact",
                    "commands::run::entry::tests::local_runtime_discovery_uses_install_layouts",
                    "--nocapture",
                ])
                .current_dir(project.path())
                .env_remove("PERRY_RUNTIME_DIR")
                .env_remove("PERRY_LIB_DIR")
                .env("PERRY_LIB_CACHE_DIR", install.path().join("cache"))
                .env(CHILD_TARGET, target)
                .env(CHILD_EXPECTED, expected.to_string());
            if let Some((name, dir)) = override_dir {
                command.env(name, dir);
            }
            let output = command.output().unwrap();
            assert!(
                String::from_utf8_lossy(&output.stdout).contains("running 1 test"),
                "the installed test binary must execute the discovery probe"
            );
            assert!(
                output.status.success(),
                "target={target}, expected={expected}:\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        };

        let bundled = install.path().join("aarch64-linux-android/release");
        std::fs::create_dir_all(&bundled).unwrap();
        let runtime = bundled.join("libperry_runtime.a");
        std::fs::write(&runtime, b"!<arch>\n").unwrap();
        run("android", true, None);
        run("wearos", true, None);
        // A different architecture must not pick up the ARM64 archive.
        run("android-x86_64", false, None);

        std::fs::remove_file(&runtime).unwrap();
        run("android", false, None);
        // npm's compressed archive layout goes through the same discovery.
        let compressed = zstd::stream::encode_all(&b"!<arch>\n"[..], 1).unwrap();
        std::fs::write(runtime.with_extension("a.zst"), compressed).unwrap();
        run("android", true, None);

        let overrides = tempfile::tempdir().unwrap();
        std::fs::write(overrides.path().join("libperry_runtime.a"), b"!<arch>\n").unwrap();
        for name in ["PERRY_RUNTIME_DIR", "PERRY_LIB_DIR"] {
            run("android-x86_64", true, Some((name, overrides.path())));
        }

        // Flat Apple bundles use target-suffixed archive names.
        std::fs::write(
            install.path().join("libperry_runtime_ios_sim.a"),
            b"!<arch>\n",
        )
        .unwrap();
        run("ios-simulator", true, None);
        assert!(can_compile_locally(None));
    }

    #[test]
    fn directory_input_resolves_default_entry() {
        let project = tempfile::tempdir().unwrap();
        let entry = project.path().join("src/main.ts");
        std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
        std::fs::write(&entry, "console.log('hello');").unwrap();

        assert_eq!(resolve_entry_file(Some(project.path())).unwrap(), entry);
    }

    #[test]
    fn directory_input_resolves_perry_toml_entry() {
        let project = tempfile::tempdir().unwrap();
        let entry = project.path().join("app/index.ts");
        std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
        std::fs::write(&entry, "console.log('hello');").unwrap();
        std::fs::write(
            project.path().join("perry.toml"),
            "entry = \"app/index.ts\"\n",
        )
        .unwrap();

        assert_eq!(resolve_entry_file(Some(project.path())).unwrap(), entry);
    }

    #[test]
    fn configured_missing_entry_does_not_fall_back() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(dir.path().join("src")).expect("src");
        std::fs::write(dir.path().join("src/main.ts"), "console.log('fallback')\n").expect("main");
        std::fs::write(
            dir.path().join("perry.toml"),
            "[project]\nentry = \"src/missing.ts\"\n",
        )
        .expect("config");

        let err = resolve_entry_file(Some(dir.path())).expect_err("missing entry must fail");
        assert!(err.to_string().contains("src/missing.ts"));
    }

    #[test]
    fn malformed_config_does_not_fall_back() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(dir.path().join("src")).expect("src");
        std::fs::write(dir.path().join("src/main.ts"), "console.log('fallback')\n").expect("main");
        std::fs::write(dir.path().join("perry.toml"), "[project\nentry = 1\n").expect("config");

        let err = resolve_entry_file(Some(dir.path())).expect_err("invalid config must fail");
        assert!(err.to_string().contains("Invalid"));
    }

    #[test]
    fn android_x86_64_uses_its_cross_runtime() {
        assert_eq!(
            rust_target_triple(Some("android-x86_64")),
            Some("x86_64-linux-android")
        );
    }
}
