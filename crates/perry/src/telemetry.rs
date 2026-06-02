//! Anonymous usage statistics for Perry CLI
//!
//! Opt-in telemetry via Chirp API. On first interactive run, the user is asked
//! once if stats collection is OK (default: yes). All telemetry is fire-and-forget
//! on background threads — never slows down the CLI.

use serde::{Deserialize, Serialize};
use std::io::IsTerminal;
use std::sync::{Mutex, OnceLock};

use crate::commands::publish::{load_config, save_config};

/// Pending telemetry completion signals. Each `send_event` pushes a receiver;
/// `flush()` drains them with a timeout so the process doesn't exit too early.
fn pending_signals() -> &'static Mutex<Vec<std::sync::mpsc::Receiver<()>>> {
    static INSTANCE: OnceLock<Mutex<Vec<std::sync::mpsc::Receiver<()>>>> = OnceLock::new();
    INSTANCE.get_or_init(|| Mutex::new(Vec::new()))
}

const CHIRP_URL: &str = "https://api.chirp247.com/api/v1/event";
const CHIRP_KEY: &str = "testkey123";
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Tri-state setting for the #849 opt-in compatibility-report channel.
/// Decoupled from `enabled` (generic usage analytics) so users can opt in
/// to one without the other.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CompatibilityReports {
    /// Never send. Sink stays uninstalled; queue stays empty.
    Off,
    /// Prompt the user the first time a qualifying report would fire.
    /// Default for new installs.
    #[default]
    Ask,
    /// Always send (after dedup + redaction). User has opted in.
    On,
}

impl CompatibilityReports {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            CompatibilityReports::Off => "off",
            CompatibilityReports::Ask => "ask",
            CompatibilityReports::On => "on",
        }
    }
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TelemetryConfig {
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) client_id: String,
    /// #849: opt-in compatibility reports. Off by default in code (`Ask`
    /// is the variant default but `#[serde(default)]` means existing
    /// installs without the field get `Off` until they upgrade — see
    /// `compatibility_reports_default`).
    #[serde(default = "compatibility_reports_default")]
    pub(crate) compatibility_reports: CompatibilityReports,
}

/// Existing users (config.toml predates #849) get `Ask` so they see the
/// prompt next time a gap is hit. New installs running through
/// `init_and_check_consent()` also land on `Ask`. Set explicitly to `Off`
/// to opt out at the file level.
fn compatibility_reports_default() -> CompatibilityReports {
    CompatibilityReports::Ask
}

/// Returns true if telemetry should be skipped entirely (explicit opt-out).
pub(crate) fn should_skip_telemetry() -> bool {
    if std::env::var("PERRY_NO_TELEMETRY").is_ok_and(|v| v == "1" || v == "true") {
        return true;
    }
    if std::env::var("CI").is_ok_and(|v| v == "true" || v == "1") {
        return true;
    }
    false
}

/// Returns true if we should skip the interactive consent prompt
/// (non-TTY environments can't prompt, but should still send if already consented).
fn should_skip_consent_prompt() -> bool {
    !std::io::stderr().is_terminal()
}

/// Load telemetry config from ~/.perry/config.toml.
/// Returns None if no [telemetry] section exists (= never asked).
pub(crate) fn load_telemetry_config() -> Option<TelemetryConfig> {
    let config = load_config();
    config.telemetry
}

/// Save telemetry config, preserving all other config sections.
pub(crate) fn save_telemetry_config(telemetry: &TelemetryConfig) {
    let mut config = load_config();
    config.telemetry = Some(telemetry.clone());
    let _ = save_config(&config);
}

/// Generate a random client ID (UUID-like hex string).
pub(crate) fn generate_client_id() -> String {
    let mut bytes = [0u8; 16];

    // Try /dev/urandom first (Unix) — must use Read trait, not fs::read (infinite device)
    let got_random = {
        use std::io::Read;
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut bytes))
            .is_ok()
    };

    if !got_random {
        // Fallback: time-based
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let nanos = t.as_nanos();
        for i in 0..16 {
            bytes[i] = ((nanos >> (i * 4)) & 0xFF) as u8;
        }
    }

    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    )
}

/// Prompt the user for telemetry consent. Returns true if they opt in.
/// Only prompts on interactive TTY. Non-interactive sessions get false without saving.
fn prompt_consent() -> bool {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return false;
    }

    let consent = dialoguer::Confirm::new()
        .with_prompt("Help improve Perry by sending anonymous usage statistics?")
        .default(true)
        .interact()
        .unwrap_or(false);

    let config = TelemetryConfig {
        enabled: consent,
        client_id: generate_client_id(),
        // Generic analytics consent prompt doesn't speak for the
        // separate #849 compat-report channel — leave it on `Ask` so
        // the user gets a focused, in-context prompt the first time
        // a gap actually fires.
        compatibility_reports: CompatibilityReports::Ask,
    };
    save_telemetry_config(&config);

    consent
}

/// Check skip conditions, load config, prompt if needed.
/// Returns true if telemetry is active for this session.
pub(crate) fn init_and_check_consent() -> bool {
    if should_skip_telemetry() {
        return false;
    }

    match load_telemetry_config() {
        Some(config) => config.enabled,
        // Only prompt if we have an interactive terminal; otherwise don't nag
        None if should_skip_consent_prompt() => false,
        None => prompt_consent(),
    }
}

/// Send an event on a background thread. The thread is tracked so `flush()`
/// can wait for it before process exit. All errors are silently ignored.
pub(crate) fn send_event(event: &str, dims: &[(&str, &str)]) {
    let config = match load_telemetry_config() {
        Some(c) if c.enabled => c,
        _ => return,
    };

    let event = event.to_string();
    let dims: Vec<(String, String)> = dims
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let client_id = config.client_id.clone();

    let (tx, rx) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        send_event_blocking(&event, &dims, &client_id);
        let _ = tx.send(());
    });

    if let Ok(mut guard) = pending_signals().lock() {
        guard.push(rx);
    }
}

/// Wait for all pending telemetry events to complete (up to 3 seconds total).
/// Call this before process exit to ensure events are delivered.
pub(crate) fn flush() {
    let receivers = if let Ok(mut guard) = pending_signals().lock() {
        std::mem::take(&mut *guard)
    } else {
        return;
    };

    if receivers.is_empty() {
        return;
    }

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    for rx in receivers {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        let _ = rx.recv_timeout(remaining);
    }
}

/// Actual HTTP POST to Chirp API.
/// Chirp expects `dims` object with known keys (platform, target, version, status, etc.).
fn send_event_blocking(event: &str, dims: &[(String, String)], client_id: &str) {
    let client = match reqwest::blocking::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
    {
        Ok(c) => c,
        Err(_) => return,
    };

    let mut dims_obj = serde_json::Map::new();
    for (k, v) in dims.iter().take(4) {
        dims_obj.insert(k.clone(), serde_json::Value::String(v.clone()));
    }

    let body = serde_json::json!({
        "event": event,
        "dims": dims_obj,
    });

    let _ = client
        .post(CHIRP_URL)
        .header("Content-Type", "application/json")
        .header("X-Chirp-Key", CHIRP_KEY)
        .header("X-Chirp-Client", client_id)
        .json(&body)
        .send();
}
