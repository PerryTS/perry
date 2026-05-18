//! CLI-side glue for #849 compatibility reports.
//!
//! `perry-diagnostics` defines the payload type, the redactor, and a
//! `ReportSink` trait. This module:
//!
//! - Installs the sink at process startup (`install_sink`).
//! - Holds the in-process queue of `PendingReport`s.
//! - Owns the 30-day dedup cache at `~/.perry/.report-cache`.
//! - Drives the consent prompt (mode = `ask`).
//! - Drains the queue and forwards approved reports to Chirp (or
//!   `--show-pending-reports`) on shutdown.
//!
//! Privacy invariant: a `PendingReport` carries the raw snippet only
//! across this in-process boundary. We never persist raw snippets to disk
//! and never put them on the wire — the snippet is redacted in
//! `CompatibilityReport::build`, the redacted version is what flushes
//! out, and the cache only stores `snippet_hash` (already redaction-only).

use crate::commands::publish::{load_config, save_config};
use crate::telemetry::{
    self, generate_client_id, load_telemetry_config, save_telemetry_config, CompatibilityReports,
    TelemetryConfig,
};
use perry_diagnostics::compat_report::{CompatibilityReport, PendingReport, ReportSink};
use serde::{Deserialize, Serialize};
use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// One-time-per-process flag so the "ask" prompt only fires once even if
/// the compiler emits 50 unsupported-feature diagnostics for the same
/// file. After the user answers, subsequent reports either flow or are
/// dropped according to the answer.
fn consent_decision_for_session() -> &'static Mutex<Option<SessionConsent>> {
    static INSTANCE: OnceLock<Mutex<Option<SessionConsent>>> = OnceLock::new();
    INSTANCE.get_or_init(|| Mutex::new(None))
}

/// What the user (or env) decided for this process run. `None` means we
/// haven't asked yet — the next qualifying report triggers the prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionConsent {
    /// `y` — yes, just this once. Don't persist.
    AllowOnce,
    /// `a` — yes, always. Persist `on`.
    AllowAlways,
    /// `n` — no, this time. Don't persist.
    DenyOnce,
    /// `N` — never. Persist `off`.
    DenyAlways,
    /// Mode was already `on` at session start (no prompt needed).
    AlreadyOn,
    /// Mode was already `off` (no prompt — we don't even install the sink).
    AlreadyOff,
}

impl SessionConsent {
    fn allows_send(&self) -> bool {
        matches!(
            self,
            SessionConsent::AllowOnce | SessionConsent::AllowAlways | SessionConsent::AlreadyOn
        )
    }
}

/// Pending-report queue. Pushed to by the diagnostic chokepoint, drained
/// at flush time.
fn pending_queue() -> &'static Mutex<Vec<PendingReport>> {
    static INSTANCE: OnceLock<Mutex<Vec<PendingReport>>> = OnceLock::new();
    INSTANCE.get_or_init(|| Mutex::new(Vec::new()))
}

/// Counters surfaced by `perry doctor`.
#[derive(Default, Debug, Clone, Copy)]
pub(crate) struct ReportCounters {
    pub queued: usize,
    pub sent: usize,
    pub suppressed_by_dedup: usize,
}

fn counters() -> &'static Mutex<ReportCounters> {
    static INSTANCE: OnceLock<Mutex<ReportCounters>> = OnceLock::new();
    INSTANCE.get_or_init(|| Mutex::new(ReportCounters::default()))
}

/// Snapshot of the live counters for `perry doctor` output.
pub(crate) fn current_counters() -> ReportCounters {
    counters().lock().map(|g| *g).unwrap_or_default()
}

struct QueueSink;

impl ReportSink for QueueSink {
    fn submit(&self, pending: PendingReport) {
        if let Ok(mut q) = pending_queue().lock() {
            // Throttle: cap the queue at 100 in-process. We don't want a
            // pathological loop emitting unbounded reports.
            if q.len() < 100 {
                q.push(pending);
                if let Ok(mut c) = counters().lock() {
                    c.queued += 1;
                }
            }
        }
    }
}

/// Read the active `compatibility_reports` mode for this process,
/// honouring env-level overrides (`PERRY_NO_TELEMETRY=1`, `CI=true`).
pub(crate) fn active_mode() -> CompatibilityReports {
    if telemetry::should_skip_telemetry() {
        return CompatibilityReports::Off;
    }
    load_telemetry_config()
        .map(|c| c.compatibility_reports)
        .unwrap_or(CompatibilityReports::Ask)
}

/// Install the diagnostic sink so HIR/codegen emission sites enqueue
/// `PendingReport`s. Idempotent — calling twice replaces the sink.
///
/// Mode `Off` skips installation entirely (zero overhead in the
/// emission path, since `has_report_sink()` returns false).
pub(crate) fn install_sink() {
    let mode = active_mode();
    if mode == CompatibilityReports::Off {
        return;
    }
    perry_diagnostics::compat_report::set_report_sink(Box::new(QueueSink));

    // If the user already opted in (`on`), pre-seed the session consent
    // so we never block on a prompt mid-compile.
    if mode == CompatibilityReports::On {
        if let Ok(mut g) = consent_decision_for_session().lock() {
            *g = Some(SessionConsent::AlreadyOn);
        }
    }
}

/// Drain the queue, prompt for consent if necessary, send approved
/// reports. Called from the CLI shutdown path right before
/// `telemetry::flush()`.
pub(crate) fn flush() {
    let mode = active_mode();
    if mode == CompatibilityReports::Off {
        // Empty the queue to keep `perry doctor`'s queued counter honest.
        if let Ok(mut q) = pending_queue().lock() {
            q.clear();
        }
        return;
    }

    let pending: Vec<PendingReport> = pending_queue()
        .lock()
        .map(|mut q| std::mem::take(&mut *q))
        .unwrap_or_default();
    if pending.is_empty() {
        return;
    }

    let consent = ensure_consent(mode, pending.first());
    if !consent.allows_send() {
        return;
    }

    let mut cache = ReportCache::load();
    let client_id = ensure_client_id();
    let perry_version = env!("CARGO_PKG_VERSION").to_string();
    let os = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    let node_target = std::env::var("PERRY_NODE_TARGET").unwrap_or_else(|_| "unknown".to_string());

    let mut to_send: Vec<CompatibilityReport> = Vec::new();
    for p in pending {
        let report = match CompatibilityReport::build(
            p.code,
            p.category,
            p.stage,
            &p.raw_snippet,
            client_id.clone(),
            perry_version.clone(),
            os.clone(),
            node_target.clone(),
            p.ts_feature.clone(),
            p.node_api.clone(),
        ) {
            Ok(r) => r,
            Err(_e) => {
                // Refused by redaction — drop, never fall back. (We
                // intentionally don't surface the error to avoid
                // confusing the user with a noisy line for an opt-in
                // background channel.)
                continue;
            }
        };

        if cache.contains_recent(&report.snippet_hash) {
            if let Ok(mut c) = counters().lock() {
                c.suppressed_by_dedup += 1;
            }
            continue;
        }
        cache.record(report.snippet_hash.clone());
        to_send.push(report);
    }

    cache.save();

    for report in to_send {
        send_compat_report(&report);
        if let Ok(mut c) = counters().lock() {
            c.sent += 1;
        }
    }
}

/// Render a single compatibility-report payload to stderr in the format
/// `perry doctor --show-pending-reports` expects. Returns the redacted
/// reports (drains the queue) so the doctor command can list them.
pub(crate) fn drain_for_display() -> Vec<CompatibilityReport> {
    let pending: Vec<PendingReport> = pending_queue()
        .lock()
        .map(|mut q| std::mem::take(&mut *q))
        .unwrap_or_default();
    let client_id = ensure_client_id();
    let perry_version = env!("CARGO_PKG_VERSION").to_string();
    let os = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    let node_target = std::env::var("PERRY_NODE_TARGET").unwrap_or_else(|_| "unknown".to_string());

    let mut out = Vec::new();
    for p in pending {
        if let Ok(r) = CompatibilityReport::build(
            p.code,
            p.category,
            p.stage,
            &p.raw_snippet,
            client_id.clone(),
            perry_version.clone(),
            os.clone(),
            node_target.clone(),
            p.ts_feature,
            p.node_api,
        ) {
            out.push(r);
        }
    }
    out
}

/// Persisted dedup record. `seen_at` is a unix timestamp; entries older
/// than `CACHE_TTL_SECS` are pruned on load.
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
struct ReportCache {
    /// snippet_hash -> unix seconds when last seen
    seen: std::collections::HashMap<String, u64>,
}

const CACHE_TTL_SECS: u64 = 30 * 24 * 60 * 60; // 30 days

pub(crate) fn cache_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".perry")
        .join(".report-cache")
}

impl ReportCache {
    fn load() -> Self {
        let path = cache_path();
        let Ok(content) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        let mut cache: ReportCache = serde_json::from_str(&content).unwrap_or_default();
        cache.prune();
        cache
    }

    fn save(&self) {
        let path = cache_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(content) = serde_json::to_string(self) {
            let _ = std::fs::write(path, content);
        }
    }

    fn contains_recent(&self, hash: &str) -> bool {
        let now = unix_now();
        match self.seen.get(hash) {
            Some(&t) => now.saturating_sub(t) < CACHE_TTL_SECS,
            None => false,
        }
    }

    fn record(&mut self, hash: String) {
        self.seen.insert(hash, unix_now());
    }

    fn prune(&mut self) {
        let now = unix_now();
        self.seen
            .retain(|_, &mut t| now.saturating_sub(t) < CACHE_TTL_SECS);
    }
}

/// Public entry point for `perry doctor --clear-report-cache`. Returns
/// `true` if a cache file existed and was removed, `false` if there was
/// nothing to clear.
pub(crate) fn clear_cache() -> bool {
    let path = cache_path();
    if path.exists() {
        std::fs::remove_file(&path).is_ok()
    } else {
        false
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Ensure a client_id is present in config (reuses the same UUID as the
/// generic telemetry channel — same anonymous identifier across both
/// opt-ins).
fn ensure_client_id() -> String {
    let cfg = load_telemetry_config();
    if let Some(c) = cfg {
        if !c.client_id.is_empty() {
            return c.client_id;
        }
    }
    // Mint a new one and persist it.
    let new_id = generate_client_id();
    let mut tc = load_telemetry_config().unwrap_or_default();
    tc.client_id = new_id.clone();
    save_telemetry_config(&tc);
    new_id
}

/// Decide whether this session is allowed to send compat reports.
/// Memoized per process — we don't re-prompt for every diagnostic.
fn ensure_consent(mode: CompatibilityReports, sample: Option<&PendingReport>) -> SessionConsent {
    if let Ok(g) = consent_decision_for_session().lock() {
        if let Some(c) = *g {
            return c;
        }
    }

    let decision = match mode {
        CompatibilityReports::Off => SessionConsent::AlreadyOff,
        CompatibilityReports::On => SessionConsent::AlreadyOn,
        CompatibilityReports::Ask => prompt_consent(sample),
    };

    if let Ok(mut g) = consent_decision_for_session().lock() {
        *g = Some(decision);
    }
    decision
}

/// Four-choice interactive prompt per #849:
///
/// ```
///   [y] yes, just this once
///   [a] yes, always (don't ask again)
///   [n] no, this time
///   [N] never, don't ask again
/// ```
///
/// Default (Enter) = `n`. Non-TTY environments silently deny (== `n`).
fn prompt_consent(sample: Option<&PendingReport>) -> SessionConsent {
    if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        return SessionConsent::DenyOnce;
    }

    eprintln!();
    if let Some(p) = sample {
        let feature = p
            .ts_feature
            .clone()
            .or_else(|| p.node_api.clone())
            .unwrap_or_else(|| format!("{:?}", p.code));
        eprintln!("Perry doesn't yet fully support: {}", feature);
    }
    eprintln!();
    eprintln!("  Send an anonymous report to help prioritize?");
    eprintln!("  [y] yes, just this once");
    eprintln!("  [a] yes, always (don't ask again)");
    eprintln!("  [n] no, this time          (default)");
    eprintln!("  [N] never, don't ask again");
    eprintln!();

    use std::io::Write;
    eprint!("> ");
    let _ = std::io::stderr().flush();

    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return SessionConsent::DenyOnce;
    }
    let answer = line.trim();
    match answer {
        "y" | "Y" => SessionConsent::AllowOnce,
        "a" | "A" => {
            persist_mode(CompatibilityReports::On);
            SessionConsent::AllowAlways
        }
        "N" => {
            persist_mode(CompatibilityReports::Off);
            SessionConsent::DenyAlways
        }
        // Empty (Enter) or "n" — deny just this time
        _ => SessionConsent::DenyOnce,
    }
}

fn persist_mode(mode: CompatibilityReports) {
    let mut config = load_config();
    let mut t = config.telemetry.unwrap_or(TelemetryConfig {
        enabled: false,
        client_id: String::new(),
        compatibility_reports: CompatibilityReports::Ask,
    });
    if t.client_id.is_empty() {
        t.client_id = generate_client_id();
    }
    t.compatibility_reports = mode;
    config.telemetry = Some(t);
    let _ = save_config(&config);
}

/// POST a single compatibility report to Chirp. Best-effort; failures are
/// silently swallowed (this is opt-in background telemetry).
fn send_compat_report(report: &CompatibilityReport) {
    let body = match serde_json::to_value(report) {
        Ok(v) => v,
        Err(_) => return,
    };
    let client_id = report.client_id.clone();

    std::thread::spawn(move || {
        let client = match reqwest::blocking::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(3))
            .timeout(std::time::Duration::from_secs(5))
            .build()
        {
            Ok(c) => c,
            Err(_) => return,
        };
        let envelope = serde_json::json!({
            "event": "compat_report",
            "dims": body,
        });
        let _ = client
            .post("https://api.chirp247.com/api/v1/event")
            .header("Content-Type", "application/json")
            .header("X-Chirp-Key", "testkey123")
            .header("X-Chirp-Client", client_id)
            .json(&envelope)
            .send();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_diagnostics::compat_report::{ReportCategory, ReportStage};
    use perry_diagnostics::DiagnosticCode;

    #[test]
    fn cache_prunes_entries_older_than_30_days() {
        let mut cache = ReportCache::default();
        // Pretend this hash was seen 31 days ago.
        let thirty_one_days = 31 * 24 * 60 * 60;
        cache.seen.insert(
            "sha256:stale".into(),
            unix_now().saturating_sub(thirty_one_days),
        );
        cache.seen.insert("sha256:fresh".into(), unix_now());
        cache.prune();
        assert!(!cache.seen.contains_key("sha256:stale"));
        assert!(cache.seen.contains_key("sha256:fresh"));
    }

    #[test]
    fn cache_contains_recent_respects_ttl() {
        let mut cache = ReportCache::default();
        cache.record("sha256:x".into());
        assert!(cache.contains_recent("sha256:x"));
        assert!(!cache.contains_recent("sha256:y"));
    }

    #[test]
    fn queue_sink_pushes_pending_reports() {
        // Clear queue first
        if let Ok(mut q) = pending_queue().lock() {
            q.clear();
        }
        if let Ok(mut c) = counters().lock() {
            *c = ReportCounters::default();
        }
        let sink = QueueSink;
        sink.submit(PendingReport {
            code: DiagnosticCode::UnsupportedExpression,
            category: ReportCategory::GapCategorical,
            stage: ReportStage::HirLower,
            raw_snippet: "const x = 1;".into(),
            ts_feature: None,
            node_api: None,
        });
        assert_eq!(pending_queue().lock().unwrap().len(), 1);
        assert_eq!(current_counters().queued, 1);
    }
}
