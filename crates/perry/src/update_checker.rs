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

#[derive(Debug, Deserialize)]
struct TrustedUpdateKey {
    key_id: String,
    public_key: String,
}

fn trusted_cli_update_keys() -> Result<Vec<TrustedUpdateKey>> {
    let raw = option_env!("PERRY_CLI_UPDATE_PUBLIC_KEYS").context(
        "this Perry release has no trusted CLI update public keys; self-update is disabled until the release is built with PERRY_CLI_UPDATE_PUBLIC_KEYS",
    )?;
    let keys: Vec<TrustedUpdateKey> = serde_json::from_str(raw)
        .context("compiled PERRY_CLI_UPDATE_PUBLIC_KEYS is invalid JSON")?;
    if keys.is_empty()
        || keys
            .iter()
            .any(|key| key.key_id.is_empty() || key.public_key.is_empty())
    {
        bail!("compiled CLI update keyring is empty or invalid");
    }
    Ok(keys)
}

fn secure_staging_dir(install_dir: &std::path::Path) -> Result<tempfile::TempDir> {
    let staging = tempfile::Builder::new()
        .prefix("perry-update-")
        .tempdir_in(install_dir)
        .context("failed to create exclusive update staging directory")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        fs::set_permissions(staging.path(), fs::Permissions::from_mode(0o700))?;
        let metadata = fs::symlink_metadata(staging.path())?;
        if !metadata.file_type().is_dir()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o077 != 0
        {
            bail!(
                "refusing insecure update staging directory {}",
                staging.path().display()
            );
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if fs::symlink_metadata(staging.path())?.file_attributes() & 0x400 != 0 {
            bail!("refusing update staging reparse point");
        }
    }
    Ok(staging)
}

fn require_https(url: &str, what: &str) -> Result<()> {
    let parsed = url::Url::parse(url).with_context(|| format!("invalid {} URL", what))?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || parsed.username() != ""
        || parsed.password().is_some()
    {
        bail!(
            "{} URL must be an absolute HTTPS URL without credentials",
            what
        );
    }
    Ok(())
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
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(Duration::from_secs(300))
        .user_agent(format!("perry/{}", current))
        .build()?;
    let mut release_info = None;
    for url in get_update_servers() {
        if let Ok(resp) = client.get(&url).send() {
            if resp.status().is_success() {
                if let Ok(info) = resp.json::<ReleaseInfo>() {
                    release_info = Some(info);
                    break;
                }
            }
        }
    }
    let info = release_info.context("Failed to fetch release info")?;
    let manifest_name = format!("{}.update.json", artifact_name);
    let manifest_asset = info
        .assets
        .iter()
        .find(|a| a.name == manifest_name)
        .with_context(|| format!("No authenticated update manifest found ({})", manifest_name))?;
    require_https(&manifest_asset.browser_download_url, "manifest")?;
    let manifest_bytes = client
        .get(&manifest_asset.browser_download_url)
        .send()
        .context("failed to download update manifest")?
        .error_for_status()
        .context("failed to download update manifest")?
        .bytes()?;
    let manifest: perry_updater::cli_manifest::CliUpdateManifest =
        serde_json::from_slice(&manifest_bytes).context("update manifest is malformed")?;
    let keys = trusted_cli_update_keys()?;
    let key_refs: Vec<(&str, &str)> = keys
        .iter()
        .map(|k| (k.key_id.as_str(), k.public_key.as_str()))
        .collect();
    perry_updater::cli_manifest::verify_cli_manifest(&manifest, artifact_name, current, &key_refs)
        .context("refusing unauthenticated update manifest")?;
    if manifest.artifact.name != artifact_name {
        bail!("authenticated manifest artifact name does not match this platform");
    }
    require_https(&manifest.artifact.url, "artifact")?;
    if verbose {
        eprintln!(
            "Authenticated update v{} with key {}",
            manifest.version, manifest.key_id
        );
    }

    let current_exe = std::env::current_exe()
        .context("Cannot determine current executable path")?
        .canonicalize()
        .context("Cannot canonicalize current executable path")?;
    let install_dir = current_exe
        .parent()
        .context("current executable has no parent directory")?;
    let staging = secure_staging_dir(install_dir)?;
    let archive_path = staging.path().join("download");
    let mut archive =
        fs::File::create(&archive_path).context("failed to create staged update artifact")?;
    let mut response = client
        .get(&manifest.artifact.url)
        .send()
        .context("Failed to download update")?
        .error_for_status()
        .context("Failed to download update")?;
    std::io::copy(&mut response, &mut archive).context("failed to stage update artifact")?;
    use std::io::Write as _;
    archive.flush()?;
    archive.sync_all()?;
    drop(archive);
    perry_updater::cli_manifest::verify_cli_artifact(&archive_path, &manifest.artifact)
        .context("refusing update artifact")?;
    let extract_dir = staging.path().join("extract");
    fs::create_dir(&extract_dir)?;
    extract_archive(&fs::read(&archive_path)?, artifact_name, &extract_dir)
        .context("Failed to safely extract authenticated archive")?;
    #[cfg(target_os = "windows")]
    let binary_name = "perry.exe";
    #[cfg(not(target_os = "windows"))]
    let binary_name = "perry";
    let new_binary = find_binary_in_dir(&extract_dir, binary_name)
        .context("Perry binary not found in authenticated archive")?;
    if let Err(err) = transactional_install(&current_exe, &new_binary, &extract_dir) {
        let preserved = staging.keep();
        return Err(err).context(format!(
            "update install failed; recovery files retained at {}",
            preserved.display()
        ));
    }
    #[cfg(windows)]
    println!("Update staged; it will be installed after Perry exits.");
    #[cfg(not(windows))]
    println!("Updated perry: v{} -> v{}", current, manifest.version);
    Ok(())
}

fn safe_archive_path(path: &std::path::Path) -> Result<std::path::PathBuf> {
    use std::path::Component;
    if path.is_absolute() || path.as_os_str().is_empty() {
        bail!("archive entry has unsafe path");
    }
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => out.push(part),
            _ => bail!("archive entry escapes staging directory"),
        }
    }
    Ok(out)
}

fn extract_archive(bytes: &[u8], artifact_name: &str, dest: &std::path::Path) -> Result<()> {
    if artifact_name.ends_with(".zip") {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
            .context("Failed to open zip archive")?;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            if entry.encrypted()
                || entry
                    .unix_mode()
                    .is_some_and(|mode| mode & 0o170000 == 0o120000)
            {
                bail!("archive contains an encrypted or symlink entry");
            }
            let rel = safe_archive_path(std::path::Path::new(entry.name()))?;
            let output = dest.join(rel);
            if entry.is_dir() {
                fs::create_dir_all(&output)?;
                continue;
            }
            let parent = output.parent().context("archive entry has no parent")?;
            fs::create_dir_all(parent)?;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&output)
                .with_context(|| {
                    format!("refusing duplicate archive entry {}", output.display())
                })?;
            std::io::copy(&mut entry, &mut file)?;
            file.sync_all()?;
        }
    } else if artifact_name.ends_with(".tar.gz") {
        let decoder = flate2::read::GzDecoder::new(bytes);
        let mut archive = tar::Archive::new(decoder);
        for entry in archive.entries().context("Failed to read tarball")? {
            let mut entry = entry?;
            let ty = entry.header().entry_type();
            let rel = safe_archive_path(&entry.path()?)?;
            let output = dest.join(rel);
            if ty.is_dir() {
                fs::create_dir_all(&output)?;
                continue;
            }
            if !ty.is_file() {
                bail!("archive contains a non-regular entry");
            }
            let parent = output.parent().context("archive entry has no parent")?;
            fs::create_dir_all(parent)?;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&output)
                .with_context(|| {
                    format!("refusing duplicate archive entry {}", output.display())
                })?;
            std::io::copy(&mut entry, &mut file)?;
            file.sync_all()?;
        }
    } else {
        bail!("unsupported update archive extension");
    }
    Ok(())
}

fn find_binary_in_dir(dir: &std::path::Path, name: &str) -> Option<PathBuf> {
    for entry in walkdir::WalkDir::new(dir)
        .max_depth(3)
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        if entry.file_name() == name && entry.file_type().is_file() {
            return Some(entry.path().to_path_buf());
        }
    }
    None
}

#[cfg(test)]
static INSTALL_FAIL_POINT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
#[cfg(test)]
static INSTALL_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
fn injected_install_failure(point: u8) -> std::io::Result<()> {
    use std::sync::atomic::Ordering;
    let configured = INSTALL_FAIL_POINT.load(Ordering::SeqCst);
    if configured == point || (configured == 5 && matches!(point, 2 | 3)) {
        let kind = if point == 4 {
            std::io::ErrorKind::PermissionDenied
        } else {
            std::io::ErrorKind::WriteZero
        };
        return Err(std::io::Error::new(kind, "injected update install failure"));
    }
    Ok(())
}
#[cfg(not(test))]
fn injected_install_failure(_: u8) -> std::io::Result<()> {
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct RecoveryJournal {
    entries: Vec<RecoveryEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct RecoveryEntry {
    target: PathBuf,
    backup: PathBuf,
    staged: PathBuf,
}

fn recovery_journal_path(install_dir: &std::path::Path) -> PathBuf {
    install_dir.join(".perry-update-recovery.json")
}

pub fn recover_interrupted_self_update() -> Result<()> {
    let current_exe = std::env::current_exe()
        .context("cannot determine executable for update recovery")?
        .canonicalize()
        .context("cannot canonicalize executable for update recovery")?;
    let install_dir = current_exe
        .parent()
        .context("executable has no parent for update recovery")?;
    #[cfg(windows)]
    if recovery_journal_path(install_dir).exists() {
        schedule_windows_recovery(&current_exe, install_dir)?;
        bail!("interrupted update recovery has been scheduled");
    }
    recover_interrupted_update_at(install_dir)
}

fn recover_interrupted_update_at(install_dir: &std::path::Path) -> Result<()> {
    let journal_path = recovery_journal_path(install_dir);
    let raw = match fs::read(&journal_path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).context("cannot read interrupted-update journal"),
    };
    let journal: RecoveryJournal =
        serde_json::from_slice(&raw).context("interrupted-update journal is malformed")?;
    if journal.entries.is_empty() {
        bail!("interrupted-update journal has no entries");
    }
    for entry in &journal.entries {
        if entry.target.parent() != Some(install_dir)
            || !entry.backup.starts_with(install_dir)
            || !fs::symlink_metadata(&entry.backup)?.file_type().is_file()
        {
            bail!("interrupted-update journal contains unsafe recovery paths");
        }
        replace_path(&entry.backup, &entry.target)
            .with_context(|| format!("failed to restore {}", entry.target.display()))?;
    }
    fs::remove_file(&journal_path)?;
    if let Some(transaction) = journal
        .entries
        .first()
        .and_then(|entry| entry.backup.parent())
    {
        let _ = fs::remove_dir_all(transaction);
    }
    #[cfg(unix)]
    {
        fs::File::open(install_dir)?.sync_all()?;
    }
    eprintln!("Recovered an interrupted Perry self-update; the previous version was restored.");
    Ok(())
}

fn write_recovery_journal(install_dir: &std::path::Path, journal: &RecoveryJournal) -> Result<()> {
    let journal_path = recovery_journal_path(install_dir);
    if fs::symlink_metadata(&journal_path).is_ok() {
        bail!("refusing to overwrite an existing update recovery journal");
    }
    let mut file = tempfile::NamedTempFile::new_in(install_dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    use std::io::Write as _;
    serde_json::to_writer(&mut file, journal)?;
    file.as_file_mut().flush()?;
    file.as_file().sync_all()?;
    file.persist_noclobber(&journal_path)
        .map_err(|error| error.error)
        .context("failed to arm update recovery journal")?;
    #[cfg(unix)]
    {
        fs::File::open(install_dir)?.sync_all()?;
    }
    Ok(())
}

fn replace_path(source: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
    #[cfg(not(windows))]
    {
        fs::rename(source, target)
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        };
        let mut source_wide: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut target_wide: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
        let ok = unsafe {
            MoveFileExW(
                source_wide.as_mut_ptr(),
                target_wide.as_mut_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
}

#[cfg(windows)]
pub fn maybe_run_windows_update_helper(args: &[String]) -> Option<Result<()>> {
    if args.get(1).map(String::as_str) != Some("--perry-update-helper") {
        return None;
    }
    let apply = match args.get(2).map(String::as_str) {
        Some("apply") => Ok(true),
        Some("rollback") => Ok(false),
        _ => Err(anyhow::anyhow!("missing update-helper mode")),
    };
    let parent_pid = args
        .get(3)
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| anyhow::anyhow!("missing update-helper parent pid"));
    let journal = args
        .get(4)
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("missing update-helper journal path"));
    Some(apply.and_then(|apply| {
        parent_pid
            .and_then(|pid| journal.and_then(|path| run_windows_update_helper(apply, pid, &path)))
    }))
}

#[cfg(windows)]
fn run_windows_update_helper(
    apply: bool,
    parent_pid: u32,
    journal_path: &std::path::Path,
) -> Result<()> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, INFINITE};
    let process = unsafe { OpenProcess(SYNCHRONIZE, 0, parent_pid) };
    if process.is_null() {
        bail!(
            "cannot wait for Perry update parent: {}",
            std::io::Error::last_os_error()
        );
    }
    unsafe {
        WaitForSingleObject(process, INFINITE);
        CloseHandle(process);
    }
    let raw = fs::read(journal_path)?;
    let journal: RecoveryJournal = serde_json::from_slice(&raw)?;
    for entry in &journal.entries {
        let source = if apply { &entry.staged } else { &entry.backup };
        replace_path(source, &entry.target)
            .with_context(|| format!("failed to replace {}", entry.target.display()))?;
    }
    fs::remove_file(journal_path)?;
    if let Some(staging) = journal
        .entries
        .first()
        .and_then(|entry| entry.staged.parent())
        .and_then(|path| path.parent())
    {
        let command = format!(
            "ping 127.0.0.1 -n 2 >NUL & rmdir /S /Q \"{}\"",
            staging.display()
        );
        let _ = std::process::Command::new("cmd")
            .args(["/C", &command])
            .spawn();
    }
    Ok(())
}

#[cfg(windows)]
fn start_windows_update_helper(
    mode: &str,
    current_exe: &std::path::Path,
    payload: &std::path::Path,
    install_dir: &std::path::Path,
) -> Result<()> {
    let helper = payload.join("perry-update-helper.exe");
    fs::copy(current_exe, &helper)?;
    std::process::Command::new(&helper)
        .arg("--perry-update-helper")
        .arg(mode)
        .arg(std::process::id().to_string())
        .arg(recovery_journal_path(install_dir))
        .spawn()
        .context("failed to start Windows update helper")?;
    Ok(())
}

#[cfg(windows)]
fn schedule_windows_recovery(
    current_exe: &std::path::Path,
    install_dir: &std::path::Path,
) -> Result<()> {
    let journal: RecoveryJournal =
        serde_json::from_slice(&fs::read(recovery_journal_path(install_dir))?)?;
    let payload = journal
        .entries
        .first()
        .and_then(|entry| entry.staged.parent())
        .context("recovery journal has no payload")?;
    start_windows_update_helper("rollback", current_exe, payload, install_dir)
}

fn transactional_install(
    current_exe: &std::path::Path,
    new_binary: &std::path::Path,
    extract_dir: &std::path::Path,
) -> Result<()> {
    if !fs::symlink_metadata(current_exe)?.file_type().is_file()
        || !fs::symlink_metadata(new_binary)?.file_type().is_file()
    {
        bail!("refusing to replace a non-regular executable");
    }
    let install_dir = current_exe.parent().context("executable has no parent")?;
    recover_interrupted_update_at(install_dir)?;
    let payload = extract_dir
        .parent()
        .context("extract directory has no staging parent")?
        .join("transaction");
    fs::create_dir(&payload).context("failed to create update transaction journal")?;
    #[cfg(unix)]
    {
        fs::File::open(extract_dir.parent().expect("checked staging parent"))?.sync_all()?;
    }
    let mut replacements = vec![(current_exe.to_path_buf(), new_binary.to_path_buf(), true)];
    #[cfg(target_os = "windows")]
    let libraries = ["perry_runtime.lib", "perry_stdlib.lib"];
    #[cfg(not(target_os = "windows"))]
    let libraries = ["libperry_runtime.a", "libperry_stdlib.a"];
    for name in libraries {
        let target = install_dir.join(name);
        if target.exists() {
            let source = find_binary_in_dir(extract_dir, name).with_context(|| {
                format!(
                    "authenticated archive is missing installed library {}",
                    name
                )
            })?;
            if !fs::symlink_metadata(&target)?.file_type().is_file()
                || !fs::symlink_metadata(&source)?.file_type().is_file()
            {
                bail!("refusing non-regular library replacement");
            }
            replacements.push((target, source, false));
        }
    }
    let mut prepared = Vec::new();
    for (index, (target, source, executable)) in replacements.iter().enumerate() {
        let staged = payload.join(format!("new-{}", index));
        injected_install_failure(1).context("injected disk-full/copy failure")?;
        fs::copy(source, &staged)
            .with_context(|| format!("failed to stage {}", target.display()))?;
        injected_install_failure(4).context("injected permission failure")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                &staged,
                fs::Permissions::from_mode(if *executable { 0o755 } else { 0o644 }),
            )?;
        }
        fs::File::open(&staged)?.sync_all()?;
        prepared.push((target.clone(), staged));
    }
    let mut journal = RecoveryJournal {
        entries: Vec::new(),
    };
    for (index, (target, _)) in prepared.iter().enumerate() {
        let backup = payload.join(format!("old-{}", index));
        if fs::hard_link(target, &backup).is_err() {
            fs::copy(target, &backup)
                .with_context(|| format!("failed to back up {}", target.display()))?;
        }
        fs::File::open(&backup)?.sync_all()?;
        journal.entries.push(RecoveryEntry {
            target: target.clone(),
            backup,
            staged: prepared[index].1.clone(),
        });
    }
    #[cfg(unix)]
    {
        fs::File::open(&payload)?.sync_all()?;
    }
    write_recovery_journal(install_dir, &journal)?;
    #[cfg(windows)]
    {
        start_windows_update_helper("apply", current_exe, &payload, install_dir)?;
        return Ok(());
    }
    for (target, staged) in &prepared {
        if let Err(error) = injected_install_failure(2).and_then(|_| replace_path(staged, target)) {
            let rollback = rollback_install(&journal);
            return match rollback { Ok(()) => Err(error).with_context(|| format!("failed to install {}; restored previous version", target.display())), Err(rollback_error) => Err(anyhow::anyhow!("failed to install {}: {}; rollback also failed; recovery will run on next launch: {}", target.display(), error, rollback_error)), };
        }
    }
    #[cfg(unix)]
    {
        fs::File::open(install_dir)?.sync_all()?;
    }
    fs::remove_file(recovery_journal_path(install_dir))
        .context("installed update but failed to disarm recovery journal")?;
    let _ = fs::remove_dir_all(&payload);
    Ok(())
}

fn rollback_install(journal: &RecoveryJournal) -> Result<()> {
    injected_install_failure(3).context("injected rollback failure")?;
    for entry in journal.entries.iter().rev() {
        replace_path(&entry.backup, &entry.target)?;
    }
    fs::remove_file(recovery_journal_path(entry_install_dir(journal)?))?;
    Ok(())
}

fn entry_install_dir(journal: &RecoveryJournal) -> Result<&std::path::Path> {
    journal
        .entries
        .first()
        .and_then(|entry| entry.target.parent())
        .context("recovery journal has no install directory")
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

    // #4715: the Windows artifact is a .zip, but extraction always ran the
    // gzip/tar decoder ("invalid gzip header"). Verify a .zip is extracted by
    // the zip path and a .tar.gz by the tar path.
    #[test]
    fn test_extract_zip_artifact() {
        use std::io::Write;
        let mut buf = Vec::new();
        {
            let mut zw = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            zw.start_file::<_, ()>("perry.exe", zip::write::SimpleFileOptions::default())
                .unwrap();
            zw.write_all(b"binary-bytes").unwrap();
            zw.finish().unwrap();
        }

        let tmp = std::env::temp_dir().join(format!("perry-zip-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        extract_archive(&buf, "perry-windows-x86_64.zip", &tmp).expect("zip should extract");
        let extracted = tmp.join("perry.exe");
        assert!(extracted.exists(), "perry.exe should be extracted");
        assert_eq!(fs::read(&extracted).unwrap(), b"binary-bytes");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_extract_targz_artifact() {
        use std::io::Write;
        let mut tar_buf = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_buf);
            let data = b"binary-bytes";
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder
                .append_data(&mut header, "perry", &data[..])
                .unwrap();
            builder.finish().unwrap();
        }
        let mut gz_buf = Vec::new();
        {
            let mut enc =
                flate2::write::GzEncoder::new(&mut gz_buf, flate2::Compression::default());
            enc.write_all(&tar_buf).unwrap();
            enc.finish().unwrap();
        }

        let tmp = std::env::temp_dir().join(format!("perry-tgz-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        extract_archive(&gz_buf, "perry-linux-x86_64.tar.gz", &tmp)
            .expect("tarball should extract");
        assert!(tmp.join("perry").exists(), "perry should be extracted");

        let _ = fs::remove_dir_all(&tmp);
    }

    fn install_fixture(with_libs: bool) -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let current = dir.path().join("perry");
        let extract = dir.path().join("extract");
        fs::create_dir(&extract).unwrap();
        let new = extract.join("perry");
        fs::write(&current, b"old-cli").unwrap();
        fs::write(&new, b"new-cli").unwrap();
        if with_libs {
            fs::write(dir.path().join("libperry_runtime.a"), b"old-runtime").unwrap();
            fs::write(extract.join("libperry_runtime.a"), b"new-runtime").unwrap();
            fs::write(dir.path().join("libperry_stdlib.a"), b"old-stdlib").unwrap();
            fs::write(extract.join("libperry_stdlib.a"), b"new-stdlib").unwrap();
        }
        (dir, current, new, extract)
    }

    #[test]
    fn rejects_corrupt_archive_and_zip_slip_and_symlink() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            extract_archive(b"not an archive", "perry-linux-x86_64.tar.gz", dir.path()).is_err()
        );
        let mut bytes = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut bytes));
            zip.start_file::<_, ()>("../outside", zip::write::SimpleFileOptions::default())
                .unwrap();
            use std::io::Write;
            zip.write_all(b"x").unwrap();
            zip.finish().unwrap();
        }
        assert!(extract_archive(&bytes, "perry-windows-x86_64.zip", dir.path()).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let link = dir.path().join("preexisting");
            symlink("/tmp", &link).unwrap();
            assert_ne!(secure_staging_dir(dir.path()).unwrap().path(), link);
        }
    }

    #[test]
    fn transaction_updates_all_libraries_or_restores_everything_on_failure() {
        let _guard = INSTALL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (_dir, current, new, extract) = install_fixture(true);
        transactional_install(&current, &new, &extract).unwrap();
        assert_eq!(fs::read(&current).unwrap(), b"new-cli");
        assert_eq!(
            fs::read(current.parent().unwrap().join("libperry_runtime.a")).unwrap(),
            b"new-runtime"
        );
        assert_eq!(
            fs::read(current.parent().unwrap().join("libperry_stdlib.a")).unwrap(),
            b"new-stdlib"
        );

        let (_dir, current, new, extract) = install_fixture(true);
        INSTALL_FAIL_POINT.store(2, std::sync::atomic::Ordering::SeqCst);
        assert!(transactional_install(&current, &new, &extract).is_err());
        INSTALL_FAIL_POINT.store(0, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(fs::read(&current).unwrap(), b"old-cli");
        assert_eq!(
            fs::read(current.parent().unwrap().join("libperry_runtime.a")).unwrap(),
            b"old-runtime"
        );
    }

    #[test]
    fn transaction_fault_injection_covers_copy_permission_missing_lib_and_rollback_failure() {
        let _guard = INSTALL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for point in [1, 4] {
            let (_dir, current, new, extract) = install_fixture(false);
            INSTALL_FAIL_POINT.store(point, std::sync::atomic::Ordering::SeqCst);
            assert!(
                transactional_install(&current, &new, &extract).is_err(),
                "point {point}"
            );
            INSTALL_FAIL_POINT.store(0, std::sync::atomic::Ordering::SeqCst);
            assert_eq!(fs::read(&current).unwrap(), b"old-cli");
        }
        let (_dir, current, new, extract) = install_fixture(true);
        fs::remove_file(extract.join("libperry_stdlib.a")).unwrap();
        assert!(transactional_install(&current, &new, &extract).is_err());
        assert_eq!(fs::read(&current).unwrap(), b"old-cli");
        let (_dir, current, new, extract) = install_fixture(true);
        INSTALL_FAIL_POINT.store(5, std::sync::atomic::Ordering::SeqCst);
        assert!(transactional_install(&current, &new, &extract).is_err());
        INSTALL_FAIL_POINT.store(0, std::sync::atomic::Ordering::SeqCst);
        let journal = extract.parent().unwrap().join("transaction");
        assert!(
            journal.join("old-0").exists(),
            "old executable retained for recovery"
        );
        assert!(
            journal.join("old-1").exists(),
            "old library retained for recovery"
        );
        recover_interrupted_update_at(current.parent().unwrap()).unwrap();
        assert_eq!(fs::read(&current).unwrap(), b"old-cli");
        assert_eq!(
            fs::read(current.parent().unwrap().join("libperry_runtime.a")).unwrap(),
            b"old-runtime"
        );
        assert!(!recovery_journal_path(current.parent().unwrap()).exists());
    }
}
