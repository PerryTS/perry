//! Entry-file resolution + target / device selection for `perry run`.

use super::*;

/// Check if we have the cross-compiled runtime libraries for a target.
/// Uses the same search logic as compile.rs find_library().
pub fn can_compile_locally(target: Option<&str>) -> bool {
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
        .join("../../target")
        .join(triple)
        .join("release/libperry_runtime.a");
    source_path.exists()
}

/// Map perry target names to Rust target triples
pub fn rust_target_triple(target: Option<&str>) -> Option<&'static str> {
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

/// Resolve the entry TypeScript file
pub fn resolve_entry_file(input: Option<&Path>) -> Result<PathBuf> {
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
pub fn read_perry_toml_entry() -> Option<PathBuf> {
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
