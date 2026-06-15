//! Device/simulator detection + interactive picker prompts.

use super::*;

/// A detected simulator or device
pub struct DeviceInfo {
    pub udid: String,
    pub name: String,
}

/// Detect booted iOS simulators via `xcrun simctl list`
pub fn detect_booted_simulators() -> Result<Vec<DeviceInfo>> {
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
pub fn detect_booted_visionos_simulators() -> Result<Vec<DeviceInfo>> {
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
pub fn detect_booted_watch_simulators() -> Result<Vec<DeviceInfo>> {
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
pub fn detect_booted_tv_simulators() -> Result<Vec<DeviceInfo>> {
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
pub fn detect_ios_devices() -> Result<Vec<DeviceInfo>> {
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
pub fn detect_android_devices() -> Result<Vec<DeviceInfo>> {
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
            devices.push(DeviceInfo { udid: serial, name });
        }
    }

    Ok(devices)
}

/// True if the adb device with this serial is a Wear OS watch, i.e. its
/// `ro.build.characteristics` property contains `watch`. Used to keep
/// `perry run wearos` from selecting a paired phone connected over the same
/// adb. Returns `false` if the property can't be read (treat as non-watch).
pub fn is_wear_os_device(serial: &str) -> bool {
    Command::new("adb")
        .args(["-s", serial, "shell", "getprop", "ro.build.characteristics"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("watch"))
        .unwrap_or(false)
}

/// Pick a device from a list using dialoguer, or auto-select if non-interactive
pub fn pick_device(devices: &[DeviceInfo], label: &str) -> Result<String> {
    let names: Vec<String> = devices
        .iter()
        .map(|d| format!("{} ({})", d.name, d.udid))
        .collect();
    let idx = pick_from_list(&names, &format!("Select {}", label))?;
    Ok(devices[idx].udid.clone())
}

/// Interactive selection from a list of options
pub fn pick_from_list(items: &[String], prompt: &str) -> Result<usize> {
    if items.is_empty() {
        return Err(anyhow!("No options available"));
    }

    // Non-interactive: pick first
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
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
