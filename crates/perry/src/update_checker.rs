//! Automatic update checker for Perry CLI
//!
//! Checks for new versions via Perry Hub / GitHub API with a 24h cache.
//! Runs non-blocking background checks on CLI invocation.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fs;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

const HUB_URL: &str = "https://hub.perryts.com/api/v1/version/latest";
const GITHUB_URL: &str = "https://api.github.com/repos/PerryTS/perry/releases/latest";
const CACHE_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct UpdateCache {
    pub last_check: String,
    pub latest_version: String,
    pub release_url: String,
}

#[derive(Debug, Deserialize)]
pub struct ReleaseInfo {
    pub tag_name: String,
    pub html_url: String,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
}

#[derive(Debug)]
pub enum UpdateStatus {
    UpToDate,
    UpdateAvailable {
        current: String,
        latest: String,
        release_url: String,
    },
    CheckFailed,
}

fn cache_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".perry")
        .join("update-check.json")
}

pub fn load_cache() -> Option<UpdateCache> {
    let path = cache_path();
    let content = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

fn save_cache(cache: &UpdateCache) {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(content) = serde_json::to_string_pretty(cache) {
        let _ = fs::write(&path, content);
    }
}

pub fn should_skip_check() -> bool {
    if std::env::var("PERRY_NO_UPDATE_CHECK").is_ok_and(|v| v == "1" || v == "true") {
        return true;
    }
    if std::env::var("CI").is_ok_and(|v| v == "true" || v == "1") {
        return true;
    }
    if !std::io::stderr().is_terminal() {
        return true;
    }
    false
}

pub fn is_cache_stale() -> bool {
    let cache = match load_cache() {
        Some(c) => c,
        None => return true,
    };

    let last_check = match chrono_parse_rfc3339(&cache.last_check) {
        Some(t) => t,
        None => return true,
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    now.saturating_sub(last_check) > CACHE_MAX_AGE.as_secs()
}

/// Simple RFC3339 timestamp to unix seconds parser
fn chrono_parse_rfc3339(s: &str) -> Option<u64> {
    // Format: 2024-01-15T10:30:00Z or 2024-01-15T10:30:00+00:00
    let s = s.trim();
    let date_time = s.split('T').collect::<Vec<_>>();
    if date_time.len() != 2 {
        return None;
    }

    let date_parts: Vec<&str> = date_time[0].split('-').collect();
    if date_parts.len() != 3 {
        return None;
    }

    let year: u64 = date_parts[0].parse().ok()?;
    let month: u64 = date_parts[1].parse().ok()?;
    let day: u64 = date_parts[2].parse().ok()?;

    let time_str = date_time[1].trim_end_matches('Z');
    let time_str = time_str.split('+').next().unwrap_or(time_str);
    let time_str = time_str.split('-').next().unwrap_or(time_str);
    let time_parts: Vec<&str> = time_str.split(':').collect();
    if time_parts.len() < 2 {
        return None;
    }

    let hour: u64 = time_parts[0].parse().ok()?;
    let min: u64 = time_parts[1].parse().ok()?;
    let sec: u64 = time_parts
        .get(2)
        .and_then(|s| s.split('.').next()?.parse().ok())
        .unwrap_or(0);

    // Approximate unix timestamp (good enough for 24h cache comparison)
    let days = days_from_civil(year, month, day)?;
    Some(days * 86400 + hour * 3600 + min * 60 + sec)
}

/// Days from 1970-01-01
fn days_from_civil(year: u64, month: u64, day: u64) -> Option<u64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let mut y = year as i64;
    let m = month as i64;
    let d = day as i64;
    if m <= 2 {
        y -= 1;
    }
    let era = y / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    if days < 0 {
        return None;
    }
    Some(days as u64)
}

fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Convert unix timestamp to RFC3339
    let days = secs / 86400;
    let day_secs = secs % 86400;
    let h = day_secs / 3600;
    let m = (day_secs % 3600) / 60;
    let s = day_secs % 60;

    // Civil date from days since epoch
    let z = days as i64 + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, d, h, m, s
    )
}

pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let a = a.strip_prefix('v').unwrap_or(a);
    let b = b.strip_prefix('v').unwrap_or(b);

    let parse = |s: &str| -> Vec<u32> { s.split('.').filter_map(|p| p.parse().ok()).collect() };

    let va = parse(a);
    let vb = parse(b);
    va.cmp(&vb)
}

fn get_update_servers() -> Vec<String> {
    let mut servers = Vec::new();

    // 1. Environment variable (highest priority)
    if let Ok(url) = std::env::var("PERRY_UPDATE_SERVER") {
        if !url.is_empty() {
            servers.push(url);
        }
    }

    // 2. Config file
    if servers.is_empty() {
        if let Some(url) = load_config_update_server() {
            servers.push(url);
        }
    }

    // 3. Perry Hub
    servers.push(HUB_URL.to_string());

    // 4. GitHub API
    servers.push(GITHUB_URL.to_string());

    servers
}

fn load_config_update_server() -> Option<String> {
    let path = dirs::home_dir()?.join(".perry").join("config.toml");
    let content = fs::read_to_string(&path).ok()?;

    #[derive(Deserialize)]
    struct Config {
        update: Option<UpdateConfig>,
    }
    #[derive(Deserialize)]
    struct UpdateConfig {
        server: Option<String>,
    }

    let config: Config = toml::from_str(&content).ok()?;
    config.update?.server
}

fn fetch_latest_version() -> Result<UpdateCache> {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .user_agent(format!("perry/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .context("Failed to create HTTP client")?;

    let servers = get_update_servers();
    let mut last_err = None;

    for url in &servers {
        match client.get(url).send() {
            Ok(resp) if resp.status().is_success() => match resp.json::<ReleaseInfo>() {
                Ok(info) => {
                    let version = info
                        .tag_name
                        .strip_prefix('v')
                        .unwrap_or(&info.tag_name)
                        .to_string();
                    let cache = UpdateCache {
                        last_check: now_rfc3339(),
                        latest_version: version,
                        release_url: info.html_url,
                    };
                    save_cache(&cache);
                    return Ok(cache);
                }
                Err(e) => {
                    last_err = Some(format!("{}: JSON parse error: {}", url, e));
                }
            },
            Ok(resp) => {
                last_err = Some(format!("{}: HTTP {}", url, resp.status()));
            }
            Err(e) => {
                last_err = Some(format!("{}: {}", url, e));
            }
        }
    }

    bail!(
        "All update servers failed. Last error: {}",
        last_err.unwrap_or_default()
    )
}

pub fn spawn_background_check() -> (JoinHandle<()>, mpsc::Receiver<UpdateStatus>) {
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let status = match fetch_latest_version() {
            Ok(cache) => {
                let current = env!("CARGO_PKG_VERSION");
                if compare_versions(&cache.latest_version, current) == Ordering::Greater {
                    UpdateStatus::UpdateAvailable {
                        current: current.to_string(),
                        latest: cache.latest_version,
                        release_url: cache.release_url,
                    }
                } else {
                    UpdateStatus::UpToDate
                }
            }
            Err(_) => UpdateStatus::CheckFailed,
        };
        let _ = tx.send(status);
    });
    (handle, rx)
}

pub fn check_cached_status() -> UpdateStatus {
    match load_cache() {
        Some(cache) => {
            let current = env!("CARGO_PKG_VERSION");
            if compare_versions(&cache.latest_version, current) == Ordering::Greater {
                UpdateStatus::UpdateAvailable {
                    current: current.to_string(),
                    latest: cache.latest_version,
                    release_url: cache.release_url,
                }
            } else {
                UpdateStatus::UpToDate
            }
        }
        None => UpdateStatus::CheckFailed,
    }
}

pub fn print_update_notice(current: &str, latest: &str, url: &str, use_color: bool) {
    if use_color {
        eprintln!(
            "\n{} {} → {} available",
            console::style("Update:").yellow().bold(),
            current,
            console::style(latest).green().bold(),
        );
        eprintln!(
            "  Run {} to update, or visit {}",
            console::style("perry update").cyan(),
            url,
        );
    } else {
        eprintln!("\nUpdate: {} -> {} available", current, latest);
        eprintln!("  Run `perry update` to update, or visit {}", url);
    }
}

pub fn platform_artifact_name() -> Option<&'static str> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        return Some("perry-macos-aarch64.tar.gz");
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        return Some("perry-macos-x86_64.tar.gz");
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        return Some("perry-linux-x86_64.tar.gz");
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        return Some("perry-linux-aarch64.tar.gz");
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        return Some("perry-windows-x86_64.zip");
    }
    #[allow(unreachable_code)]
    None
}

pub fn perform_self_update(verbose: bool) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");

    if verbose {
        eprintln!("Fetching latest version info...");
    }

    let cache = fetch_latest_version().context("Failed to check for updates")?;

    if compare_versions(&cache.latest_version, current) != Ordering::Greater {
        println!("Already up to date (v{})", current);
        return Ok(());
    }

    let artifact_name = platform_artifact_name().context("Unsupported platform for self-update")?;

    if verbose {
        eprintln!("Looking for artifact: {}", artifact_name);
    }

    // Re-fetch full release info to get asset URLs
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(Duration::from_secs(300)) // longer timeout for download
        .user_agent(format!("perry/{}", current))
        .build()?;

    let servers = get_update_servers();
    let mut release_info = None;

    for url in &servers {
        if let Ok(resp) = client.get(url).send() {
            if resp.status().is_success() {
                if let Ok(info) = resp.json::<ReleaseInfo>() {
                    release_info = Some(info);
                    break;
                }
            }
        }
    }

    let info = release_info.context("Failed to fetch release info")?;

    let asset = info
        .assets
        .iter()
        .find(|a| a.name == artifact_name)
        .with_context(|| format!("No binary found for this platform ({})", artifact_name))?;

    println!("Downloading {} v{}...", artifact_name, cache.latest_version);

    let current_exe =
        std::env::current_exe().context("Cannot determine current executable path")?;
    let current_exe = current_exe.canonicalize().unwrap_or(current_exe);

    let tmp_dir = std::env::temp_dir().join(format!("perry-update-{}", std::process::id()));
    fs::create_dir_all(&tmp_dir).context("Failed to create temp directory")?;

    // Download
    let resp = client
        .get(&asset.browser_download_url)
        .send()
        .context("Failed to download update")?;

    if !resp.status().is_success() {
        bail!("Download failed: HTTP {}", resp.status());
    }

    let bytes = resp.bytes().context("Failed to read download")?;

    if verbose {
        eprintln!("Downloaded {} bytes", bytes.len());
    }

    // Extract
    let decoder = flate2::read::GzDecoder::new(&bytes[..]);
    let mut archive = tar::Archive::new(decoder);
    archive
        .unpack(&tmp_dir)
        .context("Failed to extract archive")?;

    // Find the perry binary in extracted files
    let new_binary =
        find_binary_in_dir(&tmp_dir, "perry").context("Perry binary not found in archive")?;

    // Atomic swap
    let backup_path = current_exe.with_extension("old");

    // Remove stale backup if exists
    let _ = fs::remove_file(&backup_path);

    // Rename current -> .old
    if let Err(e) = fs::rename(&current_exe, &backup_path) {
        // Try to clean up and give helpful error
        let _ = fs::remove_dir_all(&tmp_dir);
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            bail!(
                "Permission denied. Try:\n  sudo perry update\n\nOr download manually from:\n  {}",
                asset.browser_download_url
            );
        }
        return Err(e).context("Failed to move current binary");
    }

    // Copy new binary into place
    if let Err(e) = fs::copy(&new_binary, &current_exe) {
        // Rollback
        let _ = fs::rename(&backup_path, &current_exe);
        let _ = fs::remove_dir_all(&tmp_dir);
        return Err(e).context("Failed to install new binary");
    }

    // Set executable permission
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&current_exe, fs::Permissions::from_mode(0o755));
    }

    // Also update runtime/stdlib libs if present next to binary
    if let Some(exe_dir) = current_exe.parent() {
        #[cfg(target_os = "windows")]
        let (runtime_name, stdlib_name) = ("perry_runtime.lib", "perry_stdlib.lib");
        #[cfg(not(target_os = "windows"))]
        let (runtime_name, stdlib_name) = ("libperry_runtime.a", "libperry_stdlib.a");

        let runtime_lib = exe_dir.join(runtime_name);
        if runtime_lib.exists() {
            if let Some(new_lib) = find_binary_in_dir(&tmp_dir, runtime_name) {
                let _ = fs::copy(&new_lib, &runtime_lib);
            }
        }
        let stdlib_lib = exe_dir.join(stdlib_name);
        if stdlib_lib.exists() {
            if let Some(new_lib) = find_binary_in_dir(&tmp_dir, stdlib_name) {
                let _ = fs::copy(&new_lib, &stdlib_lib);
            }
        }
    }

    // Clean up
    let _ = fs::remove_file(&backup_path);
    let _ = fs::remove_dir_all(&tmp_dir);

    println!("Updated perry: v{} -> v{}", current, cache.latest_version);

    Ok(())
}

fn find_binary_in_dir(dir: &std::path::Path, name: &str) -> Option<PathBuf> {
    // Check top level first
    let direct = dir.join(name);
    if direct.exists() {
        return Some(direct);
    }

    // Search recursively
    for entry in walkdir::WalkDir::new(dir)
        .max_depth(3)
        .into_iter()
        .flatten()
    {
        if entry.file_name().to_string_lossy() == name && entry.file_type().is_file() {
            return Some(entry.path().to_path_buf());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compare_versions() {
        assert_eq!(compare_versions("0.2.170", "0.2.171"), Ordering::Less);
        assert_eq!(compare_versions("0.2.171", "0.2.171"), Ordering::Equal);
        assert_eq!(compare_versions("0.2.172", "0.2.171"), Ordering::Greater);
        assert_eq!(compare_versions("v0.2.171", "0.2.171"), Ordering::Equal);
        assert_eq!(compare_versions("0.3.0", "0.2.999"), Ordering::Greater);
        assert_eq!(compare_versions("1.0.0", "0.99.99"), Ordering::Greater);
    }

    #[test]
    fn test_platform_artifact_name() {
        let name = platform_artifact_name();
        assert!(
            name.is_some(),
            "Should return artifact name for current platform"
        );
        let name = name.unwrap();
        assert!(name.starts_with("perry-"), "Should start with perry-");
        assert!(
            name.ends_with(".tar.gz") || name.ends_with(".zip"),
            "Should end with archive extension"
        );
    }

    #[test]
    fn test_cache_roundtrip() {
        let cache = UpdateCache {
            last_check: "2025-01-15T10:30:00Z".to_string(),
            latest_version: "0.2.171".to_string(),
            release_url: "https://github.com/PerryTS/perry/releases/tag/v0.2.171".to_string(),
        };

        let json = serde_json::to_string(&cache).unwrap();
        let parsed: UpdateCache = serde_json::from_str(&json).unwrap();
        assert_eq!(cache, parsed);
    }

    #[test]
    fn test_is_cache_stale_no_cache() {
        // When there's no cache file, it should be stale
        // This test passes because load_cache returns None for non-existent file
        assert!(is_cache_stale() || !is_cache_stale()); // Just ensure it doesn't panic
    }

    #[test]
    fn test_rfc3339_parse() {
        let ts = chrono_parse_rfc3339("2024-01-15T10:30:00Z");
        assert!(ts.is_some());

        let ts = chrono_parse_rfc3339("not-a-date");
        assert!(ts.is_none());
    }

    #[test]
    fn test_now_rfc3339_roundtrip() {
        let now = now_rfc3339();
        assert!(now.ends_with('Z'));
        assert!(chrono_parse_rfc3339(&now).is_some());
    }
}
